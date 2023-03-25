use std::{
    io::ErrorKind,
    path::{Path, PathBuf},
    sync::Arc,
};

mod config;
mod error;
mod state;

use config::Config;
pub use error::Error;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::{from_str, to_string_pretty};
use state::State;
use tokio::{
    fs::{read_to_string, write},
    sync::RwLock,
};

pub type Result<T> = std::result::Result<T, Error>;

pub const STATE_FILE: &str = ".plexout.state.json";
pub const CONFIG_FILE: &str = "plexout.json";

struct Inner {
    _config: RwLock<Config>,
    _state: RwLock<State>,
    _path: RwLock<PathBuf>,
}

pub struct PlexOut {
    _inner: Arc<Inner>,
}

async fn read_or_default<S>(path: &Path) -> Result<S>
where
    S: Serialize + DeserializeOwned + Default,
{
    match read_to_string(path).await {
        Ok(str) => Ok(from_str(&str)?),
        Err(e) => {
            if e.kind() == ErrorKind::NotFound {
                let val = S::default();
                let str = to_string_pretty(&val)?;
                write(path, str).await?;
                Ok(val)
            } else {
                Err(Error::from(e))
            }
        }
    }
}

impl PlexOut {
    pub async fn new(path: &Path) -> Result<Self> {
        let config: Config = read_or_default(&path.join(CONFIG_FILE)).await?;
        let state: State = read_or_default(&path.join(STATE_FILE)).await?;

        Ok(Self {
            _inner: Arc::new(Inner {
                _config: RwLock::new(config),
                _state: RwLock::new(state),
                _path: RwLock::new(path.to_owned()),
            }),
        })
    }
}
