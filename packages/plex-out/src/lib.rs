use std::path::{Path, PathBuf};

mod error;

pub use error::Error;

pub type Result<T> = std::result::Result<T, Error>;

pub const STATE_FILE: &str = ".plexout.state.json";
pub const CONFIG_FILE: &str = "plexout.json";

pub struct PlexOut {
    _path: PathBuf,
}

impl PlexOut {
    pub async fn new(path: &Path) -> Result<Self> {
        Ok(Self {
            _path: path.to_owned(),
        })
    }
}
