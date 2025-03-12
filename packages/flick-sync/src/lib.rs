#![deny(unreachable_pub)]
use std::{
    collections::{HashMap, HashSet},
    ops::Deref,
    path::{Path, PathBuf},
    sync::Arc,
};

mod config;
mod error;
mod schema;
mod server;
mod state;
mod util;
mod wrappers;

pub use config::ServerConnection;
use config::{Config, ServerConfig, TranscodeProfile};
pub use error::Error;
use lazy_static::lazy_static;
pub use plex_api;
use plex_api::{HttpClient, HttpClientBuilder, transcode::VideoTranscodeOptions};
use serde_json::to_string_pretty;
pub use server::{ItemType, Server, SyncItemInfo};
use state::{ServerState, State};
use tokio::{
    fs::{read_dir, remove_dir_all, remove_file, write},
    sync::{Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard},
};
use tracing::{debug, info, warn};
use uuid::Uuid;
pub use wrappers::*;

use crate::{
    config::{H264Profile, OutputStyle},
    schema::MigratableStore,
};

pub type Result<T = ()> = std::result::Result<T, Error>;

pub const STATE_FILE: &str = ".flicksync.state.json";
pub const CONFIG_FILE: &str = "flicksync.json";

lazy_static! {
    static ref DEFAULT_PROFILES: HashMap<String, Option<TranscodeProfile>> = {
        let mut map = HashMap::new();
        map.insert("original".to_string(), None);
        map.insert(
            "720p".to_string(),
            Some(TranscodeProfile {
                bitrate: Some(2000),
                dimensions: Some((1280, 720)),
                audio_channels: Some(2),
                h264_profiles: Some(vec![
                    H264Profile::Baseline,
                    H264Profile::Main,
                    H264Profile::High,
                ]),
                ..Default::default()
            }),
        );
        map.insert(
            "1080p".to_string(),
            Some(TranscodeProfile {
                bitrate: Some(6000),
                dimensions: Some((1920, 1080)),
                audio_channels: Some(2),
                h264_profiles: Some(vec![
                    H264Profile::Baseline,
                    H264Profile::Main,
                    H264Profile::High,
                ]),
                ..Default::default()
            }),
        );
        map
    };
}

struct Inner {
    config: RwLock<Config>,
    state: RwLock<State>,
    path: RwLock<PathBuf>,
    servers: Mutex<HashMap<String, Server>>,
}

impl Inner {
    async fn output_style(&self) -> OutputStyle {
        self.config.read().await.output_style
    }

    async fn transcode_options(&self, profile: Option<String>) -> Option<VideoTranscodeOptions> {
        if let Some(ref profile) = profile {
            let config = self.config.read().await;
            if let Some(profile) = config.profiles.get(profile) {
                return Some(profile.options());
            }

            match DEFAULT_PROFILES.get(profile) {
                Some(Some(profile)) => Some(profile.options()),
                Some(None) => None,
                _ => {
                    warn!("Unknown transcode profile {profile}, falling back to defaults.");
                    Some(Default::default())
                }
            }
        } else {
            Some(Default::default())
        }
    }

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
        let config = self.config.read().await;
        let state = self.state.read().await;
        HttpClientBuilder::default()
            .set_x_plex_platform(
                config
                    .device
                    .clone()
                    .unwrap_or_else(|| "Generic".to_string()),
            )
            .set_x_plex_client_identifier(state.client_id)
            .set_x_plex_product("FlickSync")
            .build()
            .unwrap()
    }
}

#[derive(Clone)]
pub struct FlickSync {
    inner: Arc<Inner>,
}

impl FlickSync {
    pub async fn client_id(&self) -> Uuid {
        self.inner.state.read().await.client_id
    }

    pub async fn max_downloads(&self) -> usize {
        let config = self.inner.config.read().await;
        config.max_downloads.unwrap_or(2)
    }

    pub async fn new(path: &Path) -> Result<Self> {
        let config = Config::read_or_default(&path.join(CONFIG_FILE)).await?;
        let state = State::read_or_default(&path.join(STATE_FILE)).await?;

        Ok(Self {
            inner: Arc::new(Inner {
                config: RwLock::new(config),
                state: RwLock::new(state),
                path: RwLock::new(path.to_owned()),
                servers: Default::default(),
            }),
        })
    }

    pub async fn root(&self) -> PathBuf {
        self.inner.path.read().await.clone()
    }

    /// Adds a new server
    pub async fn add_server(
        &self,
        id: &str,
        server: plex_api::Server,
        auth_token: &str,
        connection: ServerConnection,
        transcode_profile: Option<String>,
    ) -> Result {
        let mut state = self.inner.state.write().await;
        let mut config = self.inner.config.write().await;

        if config.servers.contains_key(id) {
            return Err(Error::ServerExists);
        }

        state.servers.insert(
            id.to_owned(),
            ServerState {
                token: auth_token.to_owned(),
                name: server.media_container.friendly_name,
                ..Default::default()
            },
        );

        config.servers.insert(
            id.to_owned(),
            ServerConfig {
                connection,
                syncs: Default::default(),
                max_transcodes: None,
                transcode_profile,
            },
        );

        self.inner.persist_config(&config).await?;
        self.inner.persist_state(&state).await?;

        Ok(())
    }

    pub async fn server(&self, id: &str) -> Option<Server> {
        let mut servers = self.inner.servers.lock().await;
        if let Some(server) = servers.get(id) {
            return Some(server.clone());
        }

        let config = self.inner.config.read().await;
        if config.servers.contains_key(id) {
            let server = Server::new(id, &self.inner);
            servers.insert(id.to_owned(), server.clone());

            Some(server)
        } else {
            None
        }
    }

    pub async fn servers(&self) -> Vec<Server> {
        let mut servers = self.inner.servers.lock().await;

        let config = self.inner.config.read().await;
        config
            .servers
            .keys()
            .map(|id| {
                servers.get(id).cloned().unwrap_or_else(|| {
                    let server = Server::new(id, &self.inner);
                    servers.insert(id.to_owned(), server.clone());
                    server
                })
            })
            .collect()
    }

    pub async fn prune_root(&self) {
        info!("Pruning root filesystem");

        let servers: HashSet<String> = {
            let config: RwLockReadGuard<'_, Config> = self.inner.config.read().await;

            config.servers.keys().cloned().collect()
        };

        let root = self.inner.path.write().await;

        let mut reader = match read_dir(root.as_path()).await {
            Ok(reader) => reader,
            Err(e) => {
                tracing::error!(error=?e, path=%root.display(), "Failed to read directory");
                return;
            }
        };

        loop {
            match reader.next_entry().await {
                Ok(Some(entry)) => {
                    if let Some(str) = entry.file_name().to_str() {
                        if str == STATE_FILE || str == CONFIG_FILE || servers.contains(str) {
                            continue;
                        }
                    }

                    let path = entry.path();
                    match entry.file_type().await {
                        Ok(file_type) => {
                            if file_type.is_dir() {
                                match remove_dir_all(&path).await {
                                    Ok(()) => {
                                        debug!(path = %path.display(), "Deleted unknown directory");
                                    }
                                    Err(e) => {
                                        tracing::error!(error=?e, path=%path.display(), "Failed to delete unknown directory");
                                    }
                                }
                            } else {
                                match remove_file(&path).await {
                                    Ok(()) => {
                                        debug!(path = %path.display(), "Deleted unknown file");
                                    }
                                    Err(e) => {
                                        tracing::error!(error=?e, path=%path.display(), "Failed to delete unknown file");
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            tracing::error!(error=?e, path=%path.display(), "Failed to read file type");
                        }
                    }
                }
                Ok(None) => {
                    break;
                }
                Err(e) => {
                    tracing::error!(error=?e, path=%root.display(), "Failed to read directory");
                    break;
                }
            }
        }
    }

    pub async fn client(&self) -> HttpClient {
        self.inner.client().await
    }
}
