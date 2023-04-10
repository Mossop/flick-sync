#![deny(unreachable_pub)]
use std::{
    collections::HashMap,
    io::ErrorKind,
    ops::Deref,
    path::{Path, PathBuf},
    sync::Arc,
};

mod config;
mod error;
mod server;
mod state;
mod wrappers;

pub use config::ServerConnection;
use config::{Config, ServerConfig};
pub use error::Error;
pub use plex_api;
use plex_api::{HttpClient, HttpClientBuilder};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::{from_str, to_string_pretty};
pub use server::Server;
use state::{ServerState, State};
use tokio::{
    fs::{read_to_string, write},
    sync::{Mutex, RwLock, RwLockWriteGuard},
};
pub use wrappers::*;

pub type Result<T = ()> = std::result::Result<T, Error>;

pub const STATE_FILE: &str = ".flicksync.state.json";
pub const CONFIG_FILE: &str = "flicksync.json";

struct Inner {
    config: RwLock<Config>,
    state: RwLock<State>,
    path: RwLock<PathBuf>,
    servers: Mutex<HashMap<String, plex_api::Server>>,
}

impl Inner {
    async fn persist_config(&self, config: &RwLockWriteGuard<'_, Config>) -> Result {
        let path = self.path.read().await;

        let str = to_string_pretty(&config.deref())?;
        write(path.join(CONFIG_FILE), str).await?;

        Ok(())
    }

    async fn persist_state(&self, state: &RwLockWriteGuard<'_, State>) -> Result {
        let path = self.path.read().await;

        let str = to_string_pretty(&state.deref())?;
        write(path.join(STATE_FILE), str).await?;

        Ok(())
    }

    async fn client(&self) -> HttpClient {
        let state = self.state.read().await;
        HttpClientBuilder::generic()
            .set_x_plex_client_identifier(state.client_id.clone())
            .build()
            .unwrap()
    }
}

#[derive(Clone)]
pub struct FlickSync {
    inner: Arc<Inner>,
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

impl FlickSync {
    pub async fn new(path: &Path) -> Result<Self> {
        let config: Config = read_or_default(&path.join(CONFIG_FILE)).await?;
        let state: State = read_or_default(&path.join(STATE_FILE)).await?;

        Ok(Self {
            inner: Arc::new(Inner {
                config: RwLock::new(config),
                state: RwLock::new(state),
                path: RwLock::new(path.to_owned()),
                servers: Default::default(),
            }),
        })
    }

    /// Adds a new server
    pub async fn add_server(
        &self,
        id: &str,
        server: plex_api::Server,
        connection: ServerConnection,
    ) -> Result {
        let mut state = self.inner.state.write().await;
        let mut config = self.inner.config.write().await;

        if config.servers.contains_key(id) {
            return Err(Error::ServerExists);
        }

        state.servers.insert(
            id.to_owned(),
            ServerState {
                token: server.client().x_plex_token().to_owned(),
                name: server.media_container.friendly_name,
                ..Default::default()
            },
        );

        config.servers.insert(
            id.to_owned(),
            ServerConfig {
                connection,
                syncs: Default::default(),
            },
        );

        self.inner.persist_config(&config).await?;
        self.inner.persist_state(&state).await?;

        Ok(())
    }

    pub async fn server(&self, id: &str) -> Option<Server> {
        let config = self.inner.config.read().await;
        if config.servers.contains_key(id) {
            Some(Server {
                id: id.to_owned(),
                inner: self.inner.clone(),
            })
        } else {
            None
        }
    }

    pub async fn servers(&self) -> Vec<Server> {
        let config = self.inner.config.read().await;
        config
            .servers
            .keys()
            .map(|id| Server {
                id: id.to_owned(),
                inner: self.inner.clone(),
            })
            .collect()
    }

    pub async fn client(&self) -> HttpClient {
        self.inner.client().await
    }
}
