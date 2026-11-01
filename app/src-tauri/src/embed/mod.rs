mod audio;
mod documents;
mod llama;
mod picture;
mod pipeline;
mod prepare;
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

use crate::vectors::Located;

pub use llama::Locations;
pub use pipeline::Pipeline;

pub const MODEL_NAME: &str = "google/embeddinggemma-2, Q8_0, llama.cpp, 140 image tokens, 256 dimensions; pictures, video frames, 30 s of sound, passages of text";
pub const IMAGE_TOKENS: u32 = 140;

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

    fn warm_up(&self) {}

    fn release(&self) {}
}

pub fn for_this_platform(data_dir: &Path, resource_dir: Option<&Path>) -> Arc<dyn Embedder> {
    #[cfg(windows)]
    {
        Arc::new(Pipeline::new(llama::LlamaServer::new(Locations::new(
            data_dir,
            resource_dir,
        ))))
    }
    #[cfg(not(windows))]
    {
        let _ = (data_dir, resource_dir);
        Arc::new(Unavailable)
    }
}

#[cfg(not(windows))]
struct Unavailable;

#[cfg(not(windows))]
impl Embedder for Unavailable {
    fn embed(&self, _: &[Item], _: &(dyn Fn() -> bool + Sync), _: &mut dyn FnMut(usize, Outcome)) -> Result<(), Error> {
        Err(Error("Embedding isn't available on this platform yet.".into()))
    }
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
    }
}
