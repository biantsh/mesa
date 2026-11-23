mod audio;
#[cfg(target_os = "macos")]
mod avfoundation;
pub mod code;
mod demux;
mod documents;
mod epub;
#[cfg(test)]
pub(crate) mod fixtures;
#[cfg(target_os = "linux")]
pub mod gstreamer;
#[cfg(target_os = "linux")]
pub mod heif;
#[cfg(target_os = "macos")]
mod imageio;
mod iwork;
mod llama;
mod model;
mod office;
mod opendocument;
pub(crate) mod outline;
mod picture;
mod pipeline;
mod prepare;
mod prime;
#[cfg(windows)]
mod process_group;
mod raw;
mod resample;
mod rtf;
mod scans;
pub mod standin;
#[cfg(windows)]
mod video;
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub mod watchdog;
#[cfg(windows)]
mod wic;

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::vectors::{Located, Record};

pub(crate) use documents::entity;
pub use llama::Locations;
pub use model::{ModelState, ModelStatus};
pub use office::shown_sheets;
pub(crate) use opendocument::is_encrypted;
#[cfg(target_os = "linux")]
pub use picture::decode_within_limits;
pub use pipeline::{GENTLE_SLOWDOWN, Pipeline};
pub use prepare::{MOST_PIECES, Prepared, ReadText, Unit, passages, prepare, sample};
#[cfg(target_os = "linux")]
pub use raw::{EXTENSIONS as RAW_EXTENSIONS, decode as raw_photo};

#[cfg(target_os = "macos")]
pub fn video_frame(path: &std::path::Path) -> Option<Vec<u8>> {
    let mut frame = None;
    avfoundation::frames_at(path, &[0.1], &mut |unit| {
        if let prepare::Prepared::Image(bytes) = unit.input {
            frame = Some(bytes);
        }
        false
    })
    .ok()?;
    frame
}

pub const MODEL_NAME: &str = "google/embeddinggemma-2, Q8_0, llama.cpp, 140 image tokens, 256 dimensions; pictures, video frames, 30 s of sound, passages of text";
pub const IMAGE_TOKENS: u32 = 140;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Device {
    Auto,
    Cpu,
}

#[derive(Clone, Debug)]
pub struct Item {
    pub path: PathBuf,
    pub words_only: bool,
}

fn display_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

#[derive(Debug, PartialEq)]
pub enum Outcome {
    Vectors(Vec<Located>, Option<String>, PageTexts),
    NoVector(String),
    Words(Option<String>, PageTexts),
}

pub type PageTexts = Vec<(u64, String)>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

pub trait Embedder: Send + Sync {
    fn embed(
        &self,
        items: &[Item],
        stop: &(dyn Fn() -> bool + Sync),
        done: &mut dyn FnMut(usize, Outcome),
    ) -> Result<(), Error>;

    fn embed_query(&self, _text: &str) -> Result<Record, Error> {
        Err(Error("Searching isn't available.".into()))
    }

    fn embed_example(&self, path: &Path, from: usize, to: usize) -> Result<(Vec<Record>, usize), Error> {
        let mut outcome = None;
        self.embed(
            &[Item {
                path: path.to_owned(),
                words_only: false,
            }],
            &|| false,
            &mut |_, made| outcome = Some(made),
        )?;
        match outcome {
            Some(Outcome::Vectors(vectors, ..)) => {
                let (picked, slots) = prepare::chosen(vectors.len(), from, to);
                Ok((picked.into_iter().map(|index| vectors[index].record).collect(), slots))
            }
            Some(Outcome::NoVector(why)) => Err(Error(format!("{} can't be searched with: {why}", display_name(path)))),
            Some(Outcome::Words(..)) | None => {
                Err(Error(format!("the model gave no answer for {}", display_name(path))))
            }
        }
    }

    fn embed_example_each(
        &self,
        path: &Path,
        from: usize,
        to: usize,
        each: &mut dyn FnMut(Vec<Record>),
    ) -> Result<usize, Error> {
        let (records, slots) = self.embed_example(path, from, to)?;
        each(records);
        Ok(slots)
    }

    fn warm_up(&self) {}

    fn read_text(&self, _path: &Path) -> Option<String> {
        None
    }

    fn reads_text(&self) -> bool {
        true
    }

    fn release(&self) {}

    fn set_device(&self, _device: Device) -> Result<(), Error> {
        Ok(())
    }

    fn set_eco(&self, _eco: bool) {}

    fn gpu(&self) -> Option<String> {
        None
    }

    fn shared_gpu(&self) -> Option<String> {
        None
    }

    fn model_status(&self) -> ModelStatus {
        ModelStatus::Ready
    }

    fn retry_model_setup(&self) {}
}

pub fn for_this_platform(data_dir: &Path, resource_dir: Option<&Path>, device: Device) -> Arc<dyn Embedder> {
    let mut server = llama::LlamaServer::new(Locations::new(data_dir, resource_dir));
    let reading = !(cfg!(test) && std::env::var_os("MESA_REAL_NO_OCR").is_some());
    if std::env::var_os("MESA_MODELS_DIR").is_none_or(|dir| dir.is_empty()) {
        server = server.download_from(model::Sources::ours());
    }
    if reading && std::env::var_os("MESA_MODELS_DIR").is_none_or(|dir| dir.is_empty()) {
        server = server.download_too(model::Group {
            dir: crate::ocr::folder(data_dir),
            files: model::OCR_FILES.to_vec(),
            sources: model::Sources {
                packed: Some(model::OCR_PACKED_SOURCE.to_owned()),
                plain: String::new(),
            },
            optional: true,
        });
    }
    server.choose_device(device);
    let pipeline = Pipeline::new(server);
    Arc::new(if reading {
        pipeline.reading_text(crate::ocr::Lazy::new(
            crate::ocr::folder(data_dir),
            device == Device::Auto,
        ))
    } else {
        pipeline
    })
}

#[cfg(test)]
pub mod testing {
    use super::*;

    pub struct Everything;

    impl Embedder for Everything {
        fn embed(
            &self,
            items: &[Item],
            _: &(dyn Fn() -> bool + Sync),
            done: &mut dyn FnMut(usize, Outcome),
        ) -> Result<(), Error> {
            for i in 0..items.len() {
                done(
                    i,
                    Outcome::Vectors(
                        vec![Located {
                            locator: 0,
                            record: crate::vectors::random_record(),
                        }],
                        None,
                        Vec::new(),
                    ),
                );
            }
            Ok(())
        }

        fn embed_query(&self, _: &str) -> Result<Record, Error> {
            Ok(crate::vectors::random_record())
        }
    }
}
