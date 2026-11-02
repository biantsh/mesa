use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::Duration,
};

use crossbeam_channel::{RecvTimeoutError, bounded, unbounded};

use super::{
    Device, Embedder, Error, Item, ModelStatus, Outcome, documents,
    prepare::{self, Prepared, Unit},
};
use crate::{
    index::lock,
    vectors::{self, Located, Record},
};

pub trait Backend: Send + Sync + 'static {
    fn start(&self) -> Result<(), Error>;

    fn embed_batch(&self, batch: &[&Prepared]) -> Result<Vec<Result<Vec<f32>, String>>, Error>;

    fn stop(&self) {}

    fn set_device(&self, _device: Device) -> Result<(), Error> {
        Ok(())
    }

    fn gpu(&self) -> Option<String> {
        None
    }

    fn model_status(&self) -> ModelStatus {
        ModelStatus::Ready
    }

    fn retry_setup(&self) -> bool {
        false
    }

    fn max_batch(&self) -> usize {
        8
    }

    fn in_flight(&self) -> usize {
        2
    }
}

pub struct Pipeline<B: Backend> {
    backend: Arc<B>,
    preparers: usize,
}

impl<B: Backend> Pipeline<B> {
    pub fn new(backend: B) -> Self {
        let cores = thread::available_parallelism().map_or(4, |n| n.get());
        Self {
            backend: Arc::new(backend),
            preparers: (cores / 2).clamp(2, 8),
        }
    }
}

enum Event {
    Unit {
        item: usize,
        locator: u64,
        output: Result<Vec<f32>, String>,
    },
    Read {
        item: usize,
        units: usize,
        error: Option<String>,
    },
}

#[derive(Default)]
struct Partial {
    units: Option<usize>,
    answered: usize,
    vectors: Vec<Located>,
    failure: Option<String>,
}

impl Partial {
    fn take(&mut self, event: Event) -> Option<Outcome> {
        match event {
            Event::Unit { locator, output, .. } => {
                self.answered += 1;
                match output {
                    Ok(values) => match vectors::record_from(&values) {
                        Some(record) => self.vectors.push(Located { locator, record }),
                        None => {
                            self.failure
                                .get_or_insert_with(|| "the model gave no usable vector for it".into());
                        }
                    },
                    Err(why) => {
                        self.failure.get_or_insert(why);
                    }
                }
            }
            Event::Read { units, error, .. } => {
                self.units = Some(units);
                if let Some(why) = error {
                    self.failure.get_or_insert(why);
                }
            }
        }
        if self.units != Some(self.answered) {
            return None;
        }
        let mut vectors = std::mem::take(&mut self.vectors);
        vectors.sort_by_key(|located| located.locator);
        Some(if vectors.is_empty() {
            Outcome::NoVector(
                self.failure
                    .take()
                    .unwrap_or_else(|| "there is nothing in it for the model to read".into()),
            )
        } else {
            Outcome::Vectors(vectors)
        })
    }
}

impl<B: Backend> Embedder for Pipeline<B> {
    fn embed(
        &self,
        items: &[Item],
        stop: &(dyn Fn() -> bool + Sync),
        done: &mut dyn FnMut(usize, Outcome),
    ) -> Result<(), Error> {
        if items.is_empty() || stop() {
            return Ok(());
        }
        if items.iter().any(prepare::is_supported) {
            self.backend.start()?;
        }

        let backend = &*self.backend;
        let next = AtomicUsize::new(0);
        let cancel = AtomicBool::new(false);
        let failure: Mutex<Option<Error>> = Mutex::new(None);
        let queue = backend.max_batch() * backend.in_flight() * 2;
        let (ready_in, ready_out) = bounded::<(usize, Unit)>(queue);
        let (event_in, event_out) = unbounded::<Event>();

        thread::scope(|scope| {
            for _ in 0..self.preparers {
                let (ready_in, event_in) = (ready_in.clone(), event_in.clone());
                let (next, cancel) = (&next, &cancel);
                scope.spawn(move || {
                    while !cancel.load(Ordering::Relaxed) {
                        let item = next.fetch_add(1, Ordering::Relaxed);
                        let Some(file) = items.get(item) else { break };
                        let mut units = 0;
                        let result = prepare::prepare(file, &mut |unit| {
                            if cancel.load(Ordering::Relaxed) || ready_in.send((item, unit)).is_err() {
                                return false;
                            }
                            units += 1;
                            true
                        });
                        if cancel.load(Ordering::Relaxed) {
                            break;
                        }
                        if event_in
                            .send(Event::Read {
                                item,
                                units,
                                error: result.err(),
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                });
            }
            for _ in 0..backend.in_flight() {
                let (ready_out, event_in) = (ready_out.clone(), event_in.clone());
                let (cancel, failure) = (&cancel, &failure);
                scope.spawn(move || {
                    while !cancel.load(Ordering::Relaxed) {
                        let Ok(first) = ready_out.recv() else { break };
                        let mut batch = vec![first];
                        while batch.len() < backend.max_batch() {
                            match ready_out.try_recv() {
                                Ok(more) => batch.push(more),
                                Err(_) => break,
                            }
                        }
                        let inputs: Vec<&Prepared> = batch.iter().map(|(_, unit)| &unit.input).collect();
                        match backend.embed_batch(&inputs) {
                            Ok(outputs) => {
                                for ((item, unit), output) in batch.iter().zip(outputs) {
                                    let event = Event::Unit {
                                        item: *item,
                                        locator: unit.locator,
                                        output,
                                    };
                                    if event_in.send(event).is_err() {
                                        return;
                                    }
                                }
                            }
                            Err(error) => {
                                lock(failure).get_or_insert(error);
                                cancel.store(true, Ordering::Relaxed);
                                return;
                            }
                        }
                    }
                });
            }
            drop((ready_in, ready_out, event_in));

            let mut partial: Vec<Partial> = items.iter().map(|_| Partial::default()).collect();
            loop {
                match event_out.recv_timeout(Duration::from_millis(50)) {
                    Ok(event) => {
                        let item = match &event {
                            Event::Unit { item, .. } | Event::Read { item, .. } => *item,
                        };
                        if let Some(outcome) = partial[item].take(event) {
                            done(item, outcome);
                        }
                    }
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => break,
                }
                if stop() {
                    cancel.store(true, Ordering::Relaxed);
                    drop(event_out);
                    break;
                }
            }
        });

        match lock(&failure).take() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    fn embed_query(&self, text: &str) -> Result<Record, Error> {
        let prompt = Prepared::Text(documents::query_prompt(text.trim()));
        let output = self
            .backend
            .embed_batch(&[&prompt])?
            .pop()
            .ok_or_else(|| Error("the model gave no answer for the query".into()))?
            .map_err(Error)?;
        vectors::record_from(&output).ok_or_else(|| Error("the model gave no usable vector for the query".into()))
    }

    fn warm_up(&self) {
        let backend = self.backend.clone();
        let _ = thread::Builder::new().name("embedder-warm-up".into()).spawn(move || {
            if let Err(e) = backend.start() {
                eprintln!("Mesa: couldn't start the embedding model: {e}");
            }
        });
    }

    fn release(&self) {
        self.backend.stop();
    }

    fn set_device(&self, device: Device) -> Result<(), Error> {
        self.backend.set_device(device)
    }

    fn gpu(&self) -> Option<String> {
        self.backend.gpu()
    }

    fn model_status(&self) -> ModelStatus {
        self.backend.model_status()
    }

    fn retry_model_setup(&self) {
        if self.backend.retry_setup() {
            self.warm_up();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, time::Instant};

    use super::*;
    use crate::vectors::DIMS;

    #[derive(Default)]
    struct Fake {
        started: AtomicUsize,
        batches: Mutex<Vec<usize>>,
        delay: Duration,
        fail_after: Option<usize>,
        running: AtomicUsize,
        most_running: AtomicUsize,
        devices: Mutex<Vec<Device>>,
    }

    impl Backend for Fake {
        fn start(&self) -> Result<(), Error> {
            self.started.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        fn embed_batch(&self, batch: &[&Prepared]) -> Result<Vec<Result<Vec<f32>, String>>, Error> {
            let running = self.running.fetch_add(1, Ordering::SeqCst) + 1;
            self.most_running.fetch_max(running, Ordering::SeqCst);
            thread::sleep(self.delay);
            self.running.fetch_sub(1, Ordering::SeqCst);
            let mut batches = lock(&self.batches);
            batches.push(batch.len());
            if self.fail_after.is_some_and(|n| batches.len() > n) {
                return Err(Error("the model fell over".into()));
            }
            Ok(batch
                .iter()
                .map(|prepared| match prepared {
                    Prepared::Text(text) if text.contains("refuse") => Err("it wouldn't".into()),
                    Prepared::Text(text) => Ok(vec![text.len() as f32; DIMS + 10]),
                    Prepared::Image(bytes) | Prepared::Audio(bytes) => Ok(vec![bytes.len() as f32; DIMS + 10]),
                })
                .collect())
        }

        fn set_device(&self, device: Device) -> Result<(), Error> {
            lock(&self.devices).push(device);
            Ok(())
        }

        fn gpu(&self) -> Option<String> {
            Some("A fake GPU".into())
        }

        fn max_batch(&self) -> usize {
            4
        }
    }

    fn texts(tag: &str, contents: &[&str]) -> Vec<Item> {
        let dir = std::env::temp_dir().join(format!("mesa-pipeline-{tag}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        contents
            .iter()
            .enumerate()
            .map(|(i, content)| {
                let path = dir.join(format!("{i}.txt"));
                fs::write(&path, content).unwrap();
                Item { path }
            })
            .collect()
    }

    fn run(pipeline: &Pipeline<Fake>, items: &[Item]) -> (Vec<Option<Outcome>>, Result<(), Error>) {
        let mut outcomes: Vec<Option<Outcome>> = items.iter().map(|_| None).collect();
        let result = pipeline.embed(items, &|| false, &mut |index, outcome| {
            assert!(
                outcomes[index].replace(outcome).is_none(),
                "item {index} reported twice"
            );
        });
        (outcomes, result)
    }

    fn vectors_of(outcome: &Option<Outcome>) -> &Vec<Located> {
        let Some(Outcome::Vectors(vectors)) = outcome else {
            panic!("expected vectors, got {outcome:?}")
        };
        vectors
    }

    fn long_text(paragraphs: usize) -> String {
        vec!["word ".repeat(100).trim().to_owned(); paragraphs].join("\n\n")
    }

    #[test]
    fn every_item_gets_an_outcome() {
        let contents: Vec<String> = (1..=30).map(|n| "x".repeat(n)).collect();
        let items = texts("each", &contents.iter().map(String::as_str).collect::<Vec<_>>());
        let (outcomes, result) = run(&Pipeline::new(Fake::default()), &items);
        result.unwrap();
        assert!(outcomes.iter().all(|o| vectors_of(o).len() == 1));
    }

    #[test]
    fn passages_keep_their_order() {
        let items = texts("passages", &[&long_text(10), "short"]);
        let (outcomes, result) = run(&Pipeline::new(Fake::default()), &items);
        result.unwrap();
        let places: Vec<u64> = vectors_of(&outcomes[0]).iter().map(|v| v.locator).collect();
        assert_eq!(places, [0, 1, 2, 3]);
        assert_eq!(vectors_of(&outcomes[1]).len(), 1);
    }

    #[test]
    fn refused_input_only_fails_itself() {
        let items = texts("refused", &["fine", "please refuse this one", "also fine"]);
        let (outcomes, result) = run(&Pipeline::new(Fake::default()), &items);
        result.unwrap();
        assert_eq!(vectors_of(&outcomes[0]).len(), 1);
        assert!(matches!(&outcomes[1], Some(Outcome::NoVector(why)) if why == "it wouldn't"));
        assert_eq!(vectors_of(&outcomes[2]).len(), 1);
    }

    #[test]
    fn stop_ends_run() {
        let contents: Vec<String> = (0..400).map(|n| format!("document {n}")).collect();
        let items = texts("stop", &contents.iter().map(String::as_str).collect::<Vec<_>>());
        let pipeline = Pipeline::new(Fake {
            delay: Duration::from_millis(30),
            ..Default::default()
        });
        let stopped = AtomicBool::new(false);
        let mut after_stop = 0;
        let mut reported = 0;
        let started = Instant::now();
        pipeline
            .embed(&items, &|| stopped.load(Ordering::SeqCst), &mut |_, _| {
                if stopped.load(Ordering::SeqCst) {
                    after_stop += 1;
                }
                reported += 1;
                if reported == 10 {
                    stopped.store(true, Ordering::SeqCst);
                }
            })
            .unwrap();
        assert_eq!(after_stop, 0);
        assert!(reported < 400);
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "took {:?}",
            started.elapsed()
        );
    }
}
