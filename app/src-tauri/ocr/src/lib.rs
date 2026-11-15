mod detect;
mod filter;
pub mod geometry;
mod net;
mod recognize;
mod resize;

use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

pub use geometry::{Picture, Point};
use net::{Net, Place};
use recognize::Recognizer;

const TEXT_SURE: f32 = 0.5;
const MIN_SIDE: usize = 8;
const DETECTOR_SIZES: usize = 16;
const RECOGNIZER_SIZES: usize = 6;

pub const DETECTOR: &str = "PP-OCRv6_det_tiny.onnx";
pub const FILTER: &str = "mesa_line_filter.onnx";
pub const RECOGNIZER: &str = "PP-OCRv6_rec_tiny.onnx";

pub fn folder(data_dir: &Path) -> PathBuf {
    data_dir.join("models").join("ocr")
}

pub fn present(dir: &Path) -> bool {
    files().iter().all(|file| dir.join(file).is_file())
}

fn saved_folder(dir: &Path) -> PathBuf {
    let name = dir
        .file_name()
        .map_or_else(|| "ocr".into(), |n| n.to_string_lossy().into_owned());
    dir.with_file_name(format!("{name}-compiled"))
}

pub fn files() -> [&'static str; 3] {
    [DETECTOR, FILTER, RECOGNIZER]
}

pub struct Ocr {
    detector: Net,
    filter: Net,
    skip_at: f32,
    recognizer: Recognizer,
}

impl Ocr {
    pub fn load(dir: &Path) -> Result<Self, String> {
        Self::load_at(dir, Place::Gpu)
    }

    pub fn load_on_processor(dir: &Path) -> Result<Self, String> {
        Self::load_at(dir, Place::Cpu)
    }

    fn load_at(dir: &Path, place: Place) -> Result<Self, String> {
        for file in files() {
            if !dir.join(file).is_file() {
                return Err(format!("{file} isn't in {}", dir.display()));
            }
        }
        let saved = saved_folder(dir);
        net::keep_only(&saved, &files());
        let detector = Net::load(&dir.join(DETECTOR), place, DETECTOR_SIZES, Some(&saved))?;
        let filter = Net::load(&dir.join(FILTER), Place::Cpu, 1, None)?;
        let skip_at = filter::check(&filter)?;
        let recognizer = Recognizer::new(Net::load(&dir.join(RECOGNIZER), place, RECOGNIZER_SIZES, Some(&saved))?)?;
        Ok(Self {
            detector,
            filter,
            skip_at,
            recognizer,
        })
    }

    pub fn prepare_all(&self, threads: usize) -> Result<usize, String> {
        if !net::CORE_ML || !self.detector.on_gpu() {
            return Ok(0);
        }
        let mut jobs: Vec<(&Net, Vec<usize>)> = detect::common_sizes()
            .into_iter()
            .map(|shape| (&self.detector, shape.to_vec()))
            .collect();
        jobs.extend(
            recognize::shapes()
                .into_iter()
                .map(|shape| (self.recognizer.net(), shape.to_vec())),
        );
        let (next, made, failure) = (AtomicUsize::new(0), AtomicUsize::new(0), Mutex::new(None::<String>));
        std::thread::scope(|scope| {
            for _ in 0..threads.max(1) {
                scope.spawn(|| {
                    while let Some((net, shape)) = jobs.get(next.fetch_add(1, Ordering::Relaxed)) {
                        match net.save(shape) {
                            Ok(true) => {
                                made.fetch_add(1, Ordering::Relaxed);
                            }
                            Ok(false) => {}
                            Err(why) => {
                                failure
                                    .lock()
                                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                                    .get_or_insert(why);
                            }
                        }
                    }
                });
            }
        });
        match failure.into_inner().unwrap_or_else(|poisoned| poisoned.into_inner()) {
            Some(why) => Err(why),
            None => Ok(made.into_inner()),
        }
    }

    pub fn prepare_common(&self) -> Result<(), String> {
        let jobs: Vec<(&Net, [usize; 4])> = std::iter::once((&self.detector, detect::first_look()))
            .chain(
                recognize::shapes()
                    .into_iter()
                    .map(|shape| (self.recognizer.net(), shape)),
            )
            .collect();
        if !net::CORE_ML {
            return jobs.iter().try_for_each(|(net, shape)| net.prepare(shape));
        }
        std::thread::scope(|scope| {
            let made: Vec<_> = jobs
                .iter()
                .map(|&(net, shape)| scope.spawn(move || net.prepare(&shape)))
                .collect();
            made.into_iter().try_for_each(|thread| {
                thread
                    .join()
                    .unwrap_or_else(|_| Err("preparing went wrong inside Mesa".into()))
            })
        })
    }

    pub fn detect(&self, picture: Picture) -> Result<Vec<[Point; 4]>, String> {
        detect::lines(&self.detector, picture)
    }

    pub fn read(&self, picture: Picture) -> Result<Vec<String>, String> {
        let found = self.detect(picture)?;
        Ok(self.read_each(picture, &found)?.into_iter().flatten().collect())
    }

    pub fn read_each(&self, picture: Picture, boxes: &[[Point; 4]]) -> Result<Vec<Option<String>>, String> {
        let mut pieces = Vec::with_capacity(boxes.len());
        for &corners in boxes {
            let piece =
                geometry::cut_out(picture, corners).filter(|piece| piece.width >= MIN_SIDE && piece.height >= MIN_SIDE);
            let read = match piece {
                Some(piece) => (filter::skip_likelihood(&self.filter, piece.view())? < self.skip_at).then_some(piece),
                None => None,
            };
            pieces.push(read);
        }
        let views: Vec<Picture> = pieces.iter().flatten().map(geometry::Owned::view).collect();
        let mut read = self.recognizer.read_all(&views)?.into_iter();
        Ok(pieces
            .iter()
            .map(|piece| {
                piece.as_ref()?;
                let (text, sure) = read.next()?;
                (sure >= TEXT_SURE && !text.trim().is_empty()).then(|| text.trim().to_owned())
            })
            .collect())
    }
}

pub struct Lazy {
    dir: PathBuf,
    loaded: Mutex<Loaded>,
    complained: AtomicBool,
}

struct Loaded {
    gpu: bool,
    fell_back: bool,
    ocr: Option<Arc<Ocr>>,
}

impl Loaded {
    fn on_gpu(&self) -> bool {
        self.gpu && !self.fell_back
    }
}

impl Lazy {
    pub fn new(dir: PathBuf, gpu: bool) -> Self {
        Self {
            dir,
            loaded: Mutex::new(Loaded {
                gpu,
                fell_back: false,
                ocr: None,
            }),
            complained: AtomicBool::new(false),
        }
    }

    pub fn present(&self) -> bool {
        self.lock().ocr.is_some() || present(&self.dir)
    }

    pub fn gpu(&self) -> bool {
        self.lock().on_gpu()
    }

    pub fn set_gpu(&self, gpu: bool) -> bool {
        let mut loaded = self.lock();
        if loaded.gpu == gpu {
            return false;
        }
        *loaded = Loaded {
            gpu,
            fell_back: false,
            ocr: None,
        };
        self.complained.store(false, Ordering::Relaxed);
        true
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Loaded> {
        self.loaded.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn get(&self) -> Option<Arc<Ocr>> {
        let mut loaded = self.lock();
        if let Some(ocr) = &loaded.ocr {
            return Some(ocr.clone());
        }
        let ocr = if loaded.on_gpu() {
            Ocr::load(&self.dir)
        } else {
            Ocr::load_on_processor(&self.dir)
        };
        match ocr {
            Ok(ocr) => Some(loaded.ocr.insert(Arc::new(ocr)).clone()),
            Err(why) => {
                if !self.complained.swap(true, Ordering::Relaxed) {
                    eprintln!("Mesa: the text in pictures can't be read: {why}");
                }
                None
            }
        }
    }

    pub fn prepare(&self) {
        if !self.present() {
            return;
        }
        let Some(ocr) = self.get() else { return };
        let started = std::time::Instant::now();
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get() / 2);
        let made = match catch_unwind(AssertUnwindSafe(|| ocr.prepare_all(threads))) {
            Ok(Ok(made)) => made,
            Ok(Err(why)) => {
                eprintln!("Mesa: what reads the text in pictures couldn't be kept for next time: {why}");
                0
            }
            Err(_) => {
                eprintln!("Mesa: keeping what reads the text in pictures went wrong inside Mesa");
                0
            }
        };
        let failed = match catch_unwind(AssertUnwindSafe(|| ocr.prepare_common())) {
            Ok(Ok(())) => None,
            Ok(Err(why)) => Some(why),
            Err(_) => Some("it went wrong inside Mesa".into()),
        };
        match failed {
            None => eprintln!(
                "Mesa: the text in pictures is ready to be read ({made} sizes made, {} ms)",
                started.elapsed().as_millis()
            ),
            Some(why) if self.fall_back(&ocr) => {
                eprintln!(
                    "Mesa: the text in pictures can't be read on the graphics card, so the processor is used: {why}"
                );
                self.get();
            }
            Some(why) => eprintln!("Mesa: what reads the text in pictures couldn't be made ready: {why}"),
        }
    }

    fn fall_back(&self, tried: &Arc<Ocr>) -> bool {
        let mut loaded = self.lock();
        let current = loaded.ocr.as_ref().is_some_and(|ocr| Arc::ptr_eq(ocr, tried));
        if !current || !loaded.on_gpu() {
            return false;
        }
        loaded.fell_back = true;
        loaded.ocr = None;
        true
    }

    pub fn text(&self, picture: Picture) -> Option<String> {
        let ocr = self.get()?;
        match catch_unwind(AssertUnwindSafe(|| ocr.read(picture))) {
            Ok(Ok(lines)) if !lines.is_empty() => Some(lines.join("\n")),
            Ok(Ok(_)) => None,
            Ok(Err(why)) => {
                eprintln!("Mesa: the text in a picture couldn't be read: {why}");
                None
            }
            Err(_) => {
                eprintln!("Mesa: reading the text in a picture went wrong inside Mesa");
                None
            }
        }
    }
}
