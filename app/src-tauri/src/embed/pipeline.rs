use std::{
    path::Path,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use crossbeam_channel::{RecvTimeoutError, bounded, unbounded};

use super::{
    Device, Embedder, Error, Item, ModelStatus, Outcome, documents,
    prepare::{self, Prepared, Unit},
};
use crate::{
    index::lock,
    ocr,
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

const REST_AFTER_A_BATCH: f64 = 3.0;
pub const GENTLE_SLOWDOWN: f64 = 1.0 + REST_AFTER_A_BATCH;
const BATCH_TIME: Duration = Duration::from_secs(1);
const UNIT_TIME_WEIGHT: f64 = 0.3;
const PARK_FALLBACK: Duration = Duration::from_millis(200);

#[derive(Default)]
struct Parking {
    lock: Mutex<()>,
    changed: Condvar,
}

impl Parking {
    fn wake(&self) {
        let _held = lock(&self.lock);
        self.changed.notify_all();
    }

    fn wait_unless(&self, go_on: impl Fn() -> bool) {
        let held = lock(&self.lock);
        if go_on() {
            return;
        }
        drop(self.changed.wait_timeout(held, PARK_FALLBACK));
    }

    fn waker(&self) -> Waker<'_> {
        Waker(self)
    }
}

struct Waker<'a>(&'a Parking);

impl Drop for Waker<'_> {
    fn drop(&mut self) {
        self.0.wake();
    }
}

pub struct Pipeline<B: Backend> {
    backend: Arc<B>,
    preparers: usize,
    eco: AtomicBool,
    parking: Parking,
    searching: AtomicUsize,
    unit_time: Mutex<Option<f64>>,
    ocr: Option<Arc<ocr::Lazy>>,
    ocr_preparing: Arc<AtomicUsize>,
}

struct Searching<'a> {
    count: &'a AtomicUsize,
    parking: &'a Parking,
}

impl Drop for Searching<'_> {
    fn drop(&mut self) {
        self.count.fetch_sub(1, Ordering::SeqCst);
        self.parking.wake();
    }
}

impl<B: Backend> Pipeline<B> {
    pub fn new(backend: B) -> Self {
        let cores = thread::available_parallelism().map_or(4, |n| n.get());
        Self {
            backend: Arc::new(backend),
            preparers: (cores / 2).clamp(2, 8),
            eco: AtomicBool::new(false),
            parking: Parking::default(),
            searching: AtomicUsize::new(0),
            unit_time: Mutex::new(None),
            ocr: None,
            ocr_preparing: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn reading_text(mut self, ocr: ocr::Lazy) -> Self {
        self.ocr = Some(Arc::new(ocr));
        self
    }

    fn search_first(&self) -> Searching<'_> {
        self.searching.fetch_add(1, Ordering::SeqCst);
        Searching {
            count: &self.searching,
            parking: &self.parking,
        }
    }

    fn batch_size(&self) -> usize {
        let most = self.backend.max_batch().max(1);
        match *lock(&self.unit_time) {
            Some(seconds) if seconds > 0.0 => ((BATCH_TIME.as_secs_f64() / seconds) as usize).clamp(1, most),
            _ => most,
        }
    }

    fn came_back(&self, inputs: usize, took: Duration) {
        let seconds = took.as_secs_f64() / inputs.max(1) as f64;
        let mut unit_time = lock(&self.unit_time);
        *unit_time = Some(unit_time.map_or(seconds, |before| {
            before * (1.0 - UNIT_TIME_WEIGHT) + seconds * UNIT_TIME_WEIGHT
        }));
    }

    fn preparers(&self) -> usize {
        if self.eco.load(Ordering::Relaxed) {
            1
        } else {
            self.preparers
        }
    }

    fn readers(&self) -> usize {
        if self.eco.load(Ordering::Relaxed) {
            1
        } else {
            self.most_readers()
        }
    }

    fn most_readers(&self) -> usize {
        (self.preparers / 2).max(1)
    }

    fn in_flight(&self) -> usize {
        if self.eco.load(Ordering::Relaxed) {
            1
        } else {
            self.backend.in_flight()
        }
    }

    fn rest(&self, took: Duration, cancel: &AtomicBool) {
        let mut left = took.mul_f64(REST_AFTER_A_BATCH);
        while !left.is_zero() && self.eco.load(Ordering::Relaxed) && !cancel.load(Ordering::Relaxed) {
            let slice = left.min(Duration::from_millis(50));
            thread::sleep(slice);
            left -= slice;
        }
    }
}

fn prepare_ocr(ocr: &ocr::Lazy, preparing: &AtomicUsize) {
    if ocr.ready() {
        return;
    }
    preparing.fetch_add(1, Ordering::SeqCst);
    ocr.prepare();
    preparing.fetch_sub(1, Ordering::SeqCst);
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
        words: String,
        text: bool,
    },
    Text {
        item: usize,
        words: Option<String>,
    },
}

#[derive(Default)]
struct Partial {
    words_only: bool,
    units: Option<usize>,
    answered: usize,
    texts: Option<usize>,
    texts_read: usize,
    vectors: Vec<Located>,
    words: String,
    failure: Option<String>,
}

impl Partial {
    fn add_words(&mut self, words: &str) {
        if words.is_empty() {
            return;
        }
        if !self.words.is_empty() {
            self.words.push('\n');
        }
        self.words.push_str(words);
    }

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
            Event::Read {
                units,
                error,
                words,
                text,
                ..
            } => {
                self.units = Some(units);
                self.texts = Some(usize::from(text));
                self.add_words(&words);
                if let Some(why) = error {
                    self.failure.get_or_insert(why);
                }
            }
            Event::Text { words, .. } => {
                self.texts_read += 1;
                if let Some(words) = words {
                    self.add_words(&words);
                }
            }
        }
        if self.units != Some(self.answered) || self.texts != Some(self.texts_read) {
            return None;
        }
        if self.words_only {
            return Some(Outcome::Words(
                Some(std::mem::take(&mut self.words)).filter(|words| !words.is_empty()),
            ));
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
            Outcome::Vectors(
                vectors,
                Some(std::mem::take(&mut self.words)).filter(|words| !words.is_empty()),
            )
        })
    }
}

impl<B: Backend> Pipeline<B> {
    fn embed_units(
        &self,
        units: &std::sync::mpsc::Receiver<Unit>,
        batch_size: usize,
    ) -> Result<(Vec<Record>, Option<String>, usize), Error> {
        let (mut records, mut failure, mut pieces) = (Vec::new(), None, 0);
        while let Ok(first) = units.recv() {
            let mut batch = vec![first];
            batch.extend(units.try_iter().take(batch_size - 1));
            pieces += batch.len();
            let inputs: Vec<&Prepared> = batch.iter().map(|unit| &unit.input).collect();
            for output in self.backend.embed_batch(&inputs)? {
                match output {
                    Ok(numbers) => records.extend(vectors::record_from(&numbers)),
                    Err(why) => {
                        failure.get_or_insert(why);
                    }
                }
            }
        }
        Ok((records, failure, pieces))
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
        if items.iter().any(|item| !item.words_only && prepare::is_supported(item)) {
            self.backend.start()?;
        }

        let backend = &*self.backend;
        let next = AtomicUsize::new(0);
        let cancel = AtomicBool::new(false);
        let failure: Mutex<Option<Error>> = Mutex::new(None);
        let in_flight = backend.in_flight();
        let queue = backend.max_batch() * in_flight * 2;
        let (ready_in, ready_out) = bounded::<(usize, Unit)>(queue);
        let (event_in, event_out) = unbounded::<Event>();
        let (text_in, text_out) = bounded::<(usize, Vec<u8>, u32, u32)>(self.preparers);
        let reading = self.ocr.as_deref();
        let preparing = AtomicUsize::new(self.preparers);

        thread::scope(|scope| {
            for number in 0..self.preparers {
                let (ready_in, event_in, text_in) = (ready_in.clone(), event_in.clone(), text_in.clone());
                let (next, cancel, preparing) = (&next, &cancel, &preparing);
                scope.spawn(move || {
                    let _leaving = self.parking.waker();
                    while !cancel.load(Ordering::Relaxed) {
                        if number >= self.preparers() {
                            let all_taken = || next.load(Ordering::Relaxed) >= items.len();
                            if all_taken() {
                                break;
                            }
                            self.parking.wait_unless(|| {
                                cancel.load(Ordering::Relaxed) || number < self.preparers() || all_taken()
                            });
                            continue;
                        }
                        let item = next.fetch_add(1, Ordering::Relaxed);
                        let Some(file) = items.get(item) else { break };
                        let mut units = 0;
                        let mut words = String::new();
                        let mut text = false;
                        let mut read_text = |rgb: Vec<u8>, width: u32, height: u32| {
                            text = text_in.send((item, rgb, width, height)).is_ok();
                        };
                        let result = prepare::prepare(
                            file,
                            reading.map(|_| &mut read_text as &mut dyn FnMut(Vec<u8>, u32, u32)),
                            &mut |unit| {
                                if cancel.load(Ordering::Relaxed) {
                                    return false;
                                }
                                if file.words_only {
                                    return true;
                                }
                                if ready_in.send((item, unit)).is_err() {
                                    return false;
                                }
                                units += 1;
                                true
                            },
                            &mut words,
                        );
                        if cancel.load(Ordering::Relaxed) {
                            break;
                        }
                        if event_in
                            .send(Event::Read {
                                item,
                                units,
                                error: result.err(),
                                words,
                                text,
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                    preparing.fetch_sub(1, Ordering::Relaxed);
                });
            }
            for number in 0..reading.map_or(0, |_| self.most_readers()) {
                let (text_out, event_in) = (text_out.clone(), event_in.clone());
                let (cancel, preparing) = (&cancel, &preparing);
                let ocr = reading.expect("readers are only made with something to read with");
                scope.spawn(move || {
                    let _leaving = self.parking.waker();
                    while !cancel.load(Ordering::Relaxed) {
                        if number >= self.readers() {
                            let nothing_more = || preparing.load(Ordering::Relaxed) == 0 && text_out.is_empty();
                            if nothing_more() {
                                break;
                            }
                            self.parking.wait_unless(|| {
                                cancel.load(Ordering::Relaxed) || number < self.readers() || nothing_more()
                            });
                            continue;
                        }
                        let Ok((item, rgb, width, height)) = text_out.recv() else {
                            break;
                        };
                        let words = ocr.text(ocr::Picture {
                            rgb: &rgb,
                            width: width as usize,
                            height: height as usize,
                        });
                        if event_in.send(Event::Text { item, words }).is_err() {
                            break;
                        }
                    }
                });
            }
            for number in 0..in_flight {
                let (ready_out, event_in) = (ready_out.clone(), event_in.clone());
                let (cancel, failure, preparing) = (&cancel, &failure, &preparing);
                scope.spawn(move || {
                    let _leaving = self.parking.waker();
                    while !cancel.load(Ordering::Relaxed) {
                        if number >= self.in_flight() {
                            let nothing_more = || preparing.load(Ordering::Relaxed) == 0 && ready_out.is_empty();
                            if nothing_more() {
                                break;
                            }
                            self.parking.wait_unless(|| {
                                cancel.load(Ordering::Relaxed) || number < self.in_flight() || nothing_more()
                            });
                            continue;
                        }
                        let Ok(first) = ready_out.recv() else { break };
                        let mut batch = vec![first];
                        let size = self.batch_size();
                        while batch.len() < size {
                            match ready_out.try_recv() {
                                Ok(more) => batch.push(more),
                                Err(_) => break,
                            }
                        }
                        while self.searching.load(Ordering::SeqCst) > 0 && !cancel.load(Ordering::Relaxed) {
                            self.parking.wait_unless(|| {
                                cancel.load(Ordering::Relaxed) || self.searching.load(Ordering::SeqCst) == 0
                            });
                        }
                        if cancel.load(Ordering::Relaxed) {
                            break;
                        }
                        let inputs: Vec<&Prepared> = batch.iter().map(|(_, unit)| &unit.input).collect();
                        let started = Instant::now();
                        match backend.embed_batch(&inputs) {
                            Ok(outputs) => {
                                self.came_back(inputs.len(), started.elapsed());
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
                                self.rest(started.elapsed(), cancel);
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
            drop((ready_in, ready_out, event_in, text_in, text_out));

            let mut partial: Vec<Partial> = items
                .iter()
                .map(|item| Partial {
                    words_only: item.words_only,
                    ..Partial::default()
                })
                .collect();
            loop {
                match event_out.recv_timeout(Duration::from_millis(50)) {
                    Ok(event) => {
                        let item = match &event {
                            Event::Unit { item, .. } | Event::Read { item, .. } | Event::Text { item, .. } => *item,
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
                    self.parking.wake();
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

    fn embed_example(&self, path: &Path, max_pieces: usize) -> Result<Vec<Record>, Error> {
        let _first = self.search_first();
        let max = max_pieces.max(1);
        let batch_size = self.backend.max_batch().max(1);
        let (send, receive) = std::sync::mpsc::sync_channel::<Unit>(batch_size * 2);
        let (made, read) = std::thread::scope(|scope| {
            let reader = scope.spawn(move || {
                let mut sent = 0;
                prepare::prepare(
                    &Item {
                        path: path.to_owned(),
                        words_only: false,
                    },
                    None,
                    &mut |unit| {
                        sent += 1;
                        send.send(unit).is_ok() && sent < max
                    },
                    &mut String::new(),
                )
            });
            let made = self.embed_units(&receive, batch_size);
            drop(receive);
            (
                made,
                reader
                    .join()
                    .unwrap_or_else(|_| Err("reading it went wrong inside Mesa".into())),
            )
        });
        let (records, failure, pieces) = made?;
        let named = |why: &str| Error(format!("{} can't be searched with: {why}", super::display_name(path)));
        if pieces == 0 {
            return Err(named(
                &read
                    .err()
                    .unwrap_or_else(|| "there is nothing in it for the model to read".into()),
            ));
        }
        if records.is_empty() {
            return Err(named(
                &failure.unwrap_or_else(|| "the model gave no usable vector".into()),
            ));
        }
        Ok(records)
    }

    fn embed_query(&self, text: &str) -> Result<Record, Error> {
        let _first = self.search_first();
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
        let (ocr, preparing) = (self.ocr.clone(), self.ocr_preparing.clone());
        let _ = thread::Builder::new().name("embedder-warm-up".into()).spawn(move || {
            if let Err(e) = backend.start() {
                eprintln!("Mesa: couldn't start the embedding model: {e}");
            }
            if let Some(ocr) = ocr {
                if backend.gpu().is_none() {
                    ocr.set_gpu(false);
                }
                prepare_ocr(&ocr, &preparing);
            }
        });
    }

    fn release(&self) {
        self.backend.stop();
    }

    fn set_device(&self, device: Device) -> Result<(), Error> {
        *lock(&self.unit_time) = None;
        self.backend.set_device(device)?;
        if let Some(ocr) = &self.ocr
            && ocr.set_gpu(device == Device::Auto && self.backend.gpu().is_some())
        {
            prepare_ocr(ocr, &self.ocr_preparing);
        }
        Ok(())
    }

    fn set_eco(&self, eco: bool) {
        self.eco.store(eco, Ordering::Relaxed);
        self.parking.wake();
    }

    fn gpu(&self) -> Option<String> {
        self.backend.gpu()
    }

    fn model_status(&self) -> ModelStatus {
        match self.backend.model_status() {
            ModelStatus::Ready if self.ocr_preparing.load(Ordering::SeqCst) > 0 => ModelStatus::Preparing,
            status => status,
        }
    }

    fn reads_text(&self) -> bool {
        self.ocr.as_ref().is_some_and(|ocr| ocr.present())
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
        stays_put: bool,
        no_gpu: bool,
        per_input: Duration,
        log: Mutex<Vec<(Instant, Instant, bool)>>,
        sized: Mutex<Vec<(thread::ThreadId, usize)>>,
    }

    impl Backend for Fake {
        fn start(&self) -> Result<(), Error> {
            self.started.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        fn embed_batch(&self, batch: &[&Prepared]) -> Result<Vec<Result<Vec<f32>, String>>, Error> {
            let running = self.running.fetch_add(1, Ordering::SeqCst) + 1;
            self.most_running.fetch_max(running, Ordering::SeqCst);
            let begun = Instant::now();
            let search = batch
                .iter()
                .any(|prepared| matches!(prepared, Prepared::Text(text) if text.contains("hold everything")));
            let search_time = if search { Duration::from_secs(1) } else { Duration::ZERO };
            thread::sleep(self.delay + self.per_input * batch.len() as u32 + search_time);
            lock(&self.log).push((begun, Instant::now(), search));
            lock(&self.sized).push((thread::current().id(), batch.len()));
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
            if self.stays_put {
                return Err(Error("it can't be started there".into()));
            }
            lock(&self.devices).push(device);
            Ok(())
        }

        fn gpu(&self) -> Option<String> {
            (!self.no_gpu).then(|| "A fake GPU".into())
        }

        fn max_batch(&self) -> usize {
            4
        }
    }

    const ENOUGH: &str = "and words enough for the model";

    fn texts(tag: &str, contents: &[&str]) -> Vec<Item> {
        let dir = std::env::temp_dir().join(format!("mesa-pipeline-{tag}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        contents
            .iter()
            .enumerate()
            .map(|(i, content)| {
                let path = dir.join(format!("{i}.txt"));
                fs::write(&path, format!("{content}\n{ENOUGH}")).unwrap();
                Item {
                    path,
                    words_only: false,
                }
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
        let Some(Outcome::Vectors(vectors, _)) = outcome else {
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
