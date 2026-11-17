use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};

use crate::{
    browse::{Categories, Category},
    embed::{self, Device, Embedder, GENTLE_SLOWDOWN, Item, ModelState, ModelStatus},
    index::lock,
    survey::{PerType, Sample, Survey},
};

const BUDGET: Duration = Duration::from_secs(6);
const ORDER: [Category; 4] = [Category::Image, Category::Document, Category::Audio, Category::Video];
const FINALIZE_LIMIT: Duration = Duration::from_secs(15);
const START_LIMIT: Duration = Duration::from_secs(120);
const RETRY_AFTER: Duration = Duration::from_secs(10 * 60);
const OCR_WAIT: Duration = Duration::from_secs(90);
const SURVEY_FRESH: Duration = Duration::from_secs(10 * 60);
const FILE: &str = "costs-4.json";

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Cost {
    pub per_file: f64,
    pub per_byte: f64,
    #[serde(default)]
    pub with_text: bool,
}

type Costs = BTreeMap<String, PerType<Option<Cost>>>;

fn device_key(device: Device) -> &'static str {
    match device {
        Device::Auto => "gpu",
        Device::Cpu => "cpu",
    }
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Estimate {
    pub seconds: PerType<Option<f64>>,
    pub measuring: bool,
    pub gentle_slowdown: f64,
}

#[derive(Default)]
struct Measurement {
    cancel: AtomicBool,
    over: Mutex<bool>,
    ended: Condvar,
}

impl Measurement {
    fn finish(&self) {
        *lock(&self.over) = true;
        self.ended.notify_all();
    }

    fn wait(&self, limit: Duration) -> bool {
        let over = lock(&self.over);
        let (over, _) = self
            .ended
            .wait_timeout_while(over, limit, |over| !*over)
            .unwrap_or_else(|e| e.into_inner());
        *over
    }
}

static CURRENT: Mutex<Option<Arc<Measurement>>> = Mutex::new(None);

pub fn before_model_starts(state: &ModelState) {
    let Some(measurement) = lock(&CURRENT).clone() else {
        return;
    };
    state.set(ModelStatus::Finalizing);
    if !measurement.wait(FINALIZE_LIMIT) {
        measurement.cancel.store(true, Ordering::Relaxed);
    }
}

struct Kept {
    at: Instant,
    not_asked: Vec<String>,
    excluded: Vec<PathBuf>,
    survey: Survey,
}

impl Kept {
    fn is_good(&self, not_asked: &[String], excluded: &[PathBuf]) -> bool {
        self.at.elapsed() < SURVEY_FRESH && self.not_asked == not_asked && self.excluded == excluded
    }
}

pub struct Estimator {
    data_dir: PathBuf,
    resource_dir: Option<PathBuf>,
    costs: Mutex<Costs>,
    survey: Mutex<Option<Kept>>,
    failed_at: Mutex<Option<Instant>>,
    surveying: Mutex<()>,
}

impl Estimator {
    pub fn open(data_dir: &Path, resource_dir: Option<&Path>) -> Arc<Self> {
        let costs = fs::read_to_string(file(data_dir))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        embed::standin::remove(data_dir);
        Arc::new(Self {
            data_dir: data_dir.to_owned(),
            resource_dir: resource_dir.map(Path::to_owned),
            costs: Mutex::new(costs),
            survey: Mutex::new(None),
            failed_at: Mutex::new(None),
            surveying: Mutex::new(()),
        })
    }

    pub fn one_survey_at_a_time(&self) -> std::sync::MutexGuard<'_, ()> {
        lock(&self.surveying)
    }

    pub fn recent_survey(&self, not_asked: &[String], excluded: &[PathBuf]) -> Option<Survey> {
        let kept = lock(&self.survey);
        let kept = kept.as_ref()?;
        kept.is_good(not_asked, excluded).then(|| kept.survey.clone())
    }

    pub fn counted(&self, types: Categories, excluded: &[PathBuf]) -> Option<Vec<(PathBuf, u64)>> {
        let kept = lock(&self.survey);
        let kept = kept.as_ref()?;
        if kept.at.elapsed() >= SURVEY_FRESH || kept.excluded != excluded || !kept.survey.not_asked.is_empty() {
            return None;
        }
        let of = |counts: &PerType<u64>| {
            crate::browse::CATEGORIES
                .iter()
                .filter(|&&kind| types.contains(kind))
                .map(|&kind| *counts.get(kind))
                .sum()
        };
        Some(
            kept.survey
                .by_folder
                .iter()
                .map(|(folder, counts)| (folder.clone(), of(counts)))
                .filter(|&(_, files)| files > 0)
                .collect(),
        )
    }

    pub fn keep(&self, survey: Survey, not_asked: Vec<String>, excluded: Vec<PathBuf>) {
        *lock(&self.survey) = Some(Kept {
            at: Instant::now(),
            not_asked,
            excluded,
            survey,
        });
    }

    pub fn measure_missing(self: &Arc<Self>, device: Device) {
        let Some(samples) = lock(&self.survey).as_ref().map(|kept| kept.survey.samples.clone()) else {
            return;
        };
        let wanted: Vec<Category> = ORDER
            .into_iter()
            .filter(|&kind| !samples.get(kind).is_empty() && self.cost(device, kind).is_none())
            .collect();
        if wanted.is_empty() || lock(&self.failed_at).is_some_and(|at| at.elapsed() < RETRY_AFTER) {
            return;
        }
        let measurement = {
            let mut current = lock(&CURRENT);
            if current.is_some() {
                return;
            }
            let measurement = Arc::new(Measurement::default());
            *current = Some(measurement.clone());
            measurement
        };
        let this = self.clone();
        let spawned = thread::Builder::new().name("estimate".into()).spawn({
            let measurement = measurement.clone();
            move || {
                if let Err(why) = this.measure_on_standin(&measurement, &samples, &wanted, device) {
                    eprintln!("Mesa: couldn't time the model for the estimate: {why}");
                    *lock(&this.failed_at) = Some(Instant::now());
                }
                *lock(&CURRENT) = None;
                measurement.finish();
            }
        });
        if spawned.is_err() {
            *lock(&CURRENT) = None;
        }
    }

    fn measure_on_standin(
        &self,
        measurement: &Measurement,
        samples: &PerType<Vec<Sample>>,
        wanted: &[Category],
        device: Device,
    ) -> Result<(), String> {
        let embedder = embed::standin::embedder(&self.data_dir, self.resource_dir.as_deref(), device)?;
        let ocr = crate::ocr::folder(&self.data_dir);
        let later = !crate::ocr::present(&ocr) && wanted.contains(&Category::Image);
        let first: Vec<Category> = wanted
            .iter()
            .copied()
            .filter(|&kind| !(later && kind == Category::Image))
            .collect();
        let keep = |kind, cost: Cost| {
            let with_text = kind == Category::Image && crate::ocr::present(&ocr);
            self.keep_cost(device, kind, Cost { with_text, ..cost });
        };
        let mut result = measure(&*embedder, samples, &first, BUDGET, &measurement.cancel, keep);
        if result.is_ok() && later {
            let waited = Instant::now();
            while !crate::ocr::present(&ocr)
                && waited.elapsed() < OCR_WAIT
                && !measurement.cancel.load(Ordering::Relaxed)
            {
                thread::sleep(Duration::from_millis(200));
            }
            if !measurement.cancel.load(Ordering::Relaxed) {
                result = measure(
                    &*embedder,
                    samples,
                    &[Category::Image],
                    BUDGET,
                    &measurement.cancel,
                    keep,
                );
            }
        }
        embedder.release();
        embed::standin::remove(&self.data_dir);
        result
    }

    fn cost(&self, device: Device, kind: Category) -> Option<Cost> {
        let cost = lock(&self.costs)
            .get(device_key(device))
            .and_then(|costs| *costs.get(kind))?;
        let stale =
            kind == Category::Image && !cost.with_text && crate::ocr::present(&crate::ocr::folder(&self.data_dir));
        (!stale).then_some(cost)
    }

    fn keep_cost(&self, device: Device, kind: Category, cost: Cost) {
        let mut costs = lock(&self.costs);
        *costs.entry(device_key(device).to_owned()).or_default().get_mut(kind) = Some(cost);
        let path = file(&self.data_dir);
        let _ = fs::create_dir_all(path.parent().unwrap_or(Path::new(".")));
        let _ = serde_json::to_vec_pretty(&*costs).map(|bytes| fs::write(path, bytes));
    }

    pub fn estimate(&self, device: Device) -> Estimate {
        let costs = lock(&self.costs).get(device_key(device)).cloned().unwrap_or_default();
        let kept = lock(&self.survey);
        let none = Survey::default();
        let survey = kept.as_ref().map_or(&none, |kept| &kept.survey);
        let mut seconds = PerType::default();
        for kind in ORDER {
            *seconds.get_mut(kind) = seconds_for(survey, kind, *costs.get(kind));
        }
        seconds.code = seconds_for(survey, Category::Code, *costs.get(Category::Document));
        Estimate {
            seconds,
            measuring: lock(&CURRENT).is_some(),
            gentle_slowdown: GENTLE_SLOWDOWN,
        }
    }
}

fn file(data_dir: &Path) -> PathBuf {
    data_dir.join("estimate").join(FILE)
}

fn seconds_for(survey: &Survey, kind: Category, cost: Option<Cost>) -> Option<f64> {
    let files = *survey.counts.get(kind);
    if files == 0 {
        return Some(0.0);
    }
    let cost = cost?;
    Some(match kind {
        Category::Image => cost.per_file * files as f64,
        _ => cost.per_byte * *survey.bytes.get(kind) as f64,
    })
}

fn measure(
    embedder: &dyn Embedder,
    samples: &PerType<Vec<Sample>>,
    wanted: &[Category],
    budget: Duration,
    cancel: &AtomicBool,
    mut keep: impl FnMut(Category, Cost),
) -> Result<(), String> {
    let Some(first) = wanted
        .iter()
        .find_map(|&kind| samples.get(kind).iter().min_by_key(|(_, size)| *size))
    else {
        return Ok(());
    };
    embedder
        .embed(
            &[Item {
                path: first.0.clone(),
                words_only: false,
            }],
            &|| cancel.load(Ordering::Relaxed),
            &mut |_, _| {},
        )
        .map_err(|e| e.0)?;
    let started = Instant::now();
    while matches!(embedder.model_status(), ModelStatus::Starting | ModelStatus::Preparing) {
        if cancel.load(Ordering::Relaxed) {
            return Ok(());
        }
        if started.elapsed() > START_LIMIT {
            return Err("the model took too long to get ready".into());
        }
        thread::sleep(Duration::from_millis(50));
    }

    for &kind in wanted {
        let mut files = samples.get(kind).clone();
        if files.is_empty() {
            continue;
        }
        files.sort_by_key(|&(_, size)| size);
        let items: Vec<Item> = files
            .iter()
            .map(|(path, _)| Item {
                path: path.clone(),
                words_only: false,
            })
            .collect();
        let started = Instant::now();
        let (mut done, mut bytes, mut last) = (0u64, 0u64, Duration::ZERO);
        embedder
            .embed(
                &items,
                &|| cancel.load(Ordering::Relaxed) || started.elapsed() > budget,
                &mut |i, _| {
                    done += 1;
                    bytes += files[i].1;
                    last = started.elapsed();
                },
            )
            .map_err(|e| e.0)?;
        if cancel.load(Ordering::Relaxed) {
            return Ok(());
        }
        if done > 0 {
            let seconds = last.as_secs_f64();
            keep(
                kind,
                Cost {
                    per_file: seconds / done as f64,
                    per_byte: seconds / bytes.max(1) as f64,
                    with_text: false,
                },
            );
        }
    }
    Ok(())
}
