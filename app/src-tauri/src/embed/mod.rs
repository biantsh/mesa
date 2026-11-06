mod audio;
mod documents;
mod llama;
mod model;
mod picture;
mod pipeline;
mod prepare;
mod prime;
#[cfg(windows)]
mod process_group;
mod resample;
#[cfg(windows)]
mod video;
#[cfg(windows)]
mod wic;

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::vectors::{Located, Record};

pub use llama::Locations;
pub use model::ModelStatus;
pub use pipeline::Pipeline;

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
}

#[derive(Debug, PartialEq)]
pub enum Outcome {
    Vectors(Vec<Located>),
    NoVector(String),
}

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

    fn warm_up(&self) {}

    fn release(&self) {}

    fn set_device(&self, _device: Device) -> Result<(), Error> {
        Ok(())
    }

    fn set_eco(&self, _eco: bool) -> Result<(), Error> {
        Ok(())
    }

    fn gpu(&self) -> Option<String> {
        None
    }

    fn model_status(&self) -> ModelStatus {
        ModelStatus::Ready
    }

    fn retry_model_setup(&self) {}
}

pub fn for_this_platform(data_dir: &Path, resource_dir: Option<&Path>, device: Device) -> Arc<dyn Embedder> {
    let mut server = llama::LlamaServer::new(Locations::new(data_dir, resource_dir));
    if std::env::var_os("MESA_MODELS_DIR").is_none_or(|dir| dir.is_empty()) {
        server = server.download_from(model::SOURCE);
    }
    server.choose_device(device);
    Arc::new(Pipeline::new(server))
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
                    Outcome::Vectors(vec![Located {
                        locator: 0,
                        record: crate::vectors::random_record(),
                    }]),
                );
            }
            Ok(())
        }

        fn embed_query(&self, _: &str) -> Result<Record, Error> {
            Ok(crate::vectors::random_record())
        }
    }
}
