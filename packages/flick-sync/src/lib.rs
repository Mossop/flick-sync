#![deny(unreachable_pub)]
use std::{
    collections::{HashMap, HashSet},
    io::{self, ErrorKind},
    ops::Deref,
    path::{Path, PathBuf},
    result,
    sync::Arc,
};

mod config;
mod schema;
mod server;
mod state;
mod sync;
mod util;
mod wrappers;

use anyhow::bail;
use config::{Config, ServerConfig, TranscodeProfile};
use lazy_static::lazy_static;
pub use plex_api;
use plex_api::{
    HttpClient, HttpClientBuilder, media_container::server::library::ContainerFormat,
    transcode::VideoTranscodeOptions,
};
use serde_json::to_string_pretty;
use state::{ServerState, State};
use time::OffsetDateTime;
use tokio::{
    fs::{read_dir, remove_dir_all, remove_file, rename, write},
    sync::{Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard, Semaphore},
};
use tracing::{debug, error, info, instrument, warn};
use uuid::Uuid;

pub use crate::{
    config::ServerConnection,
    server::{DownloadProgress, ItemType, Progress, Server, SyncItemInfo},
    state::{LibraryType, PlaybackState},
    sync::{LockedFile, LockedFileAsyncRead, LockedFileRead, Timeout},
    wrappers::*,
};
use crate::{
    config::{H264Profile, OutputStyle},
    schema::MigratableStore,
};

pub type Result<T = ()> = anyhow::Result<T>;

pub const STATE_FILE: &str = ".flicksync.state.json";
pub const CONFIG_FILE: &str = "flicksync.json";

pub(crate) const DEFAULT_PROFILE: &str = "720p";

lazy_static! {
    static ref DEFAULT_PROFILES: HashMap<String, Option<TranscodeProfile>> = {
        let mut map = HashMap::new();
        map.insert("original".to_string(), None);
        map.insert(
            "720p".to_string(),
            Some(TranscodeProfile {
                bitrate: Some(4000),
                dimensions: Some((1280, 720)),
                audio_channels: Some(2),
                h264_profiles: Some(vec![
                    H264Profile::Baseline,
                    H264Profile::Main,
                    H264Profile::High,
                ]),
                h264_level: Some("51".to_string()),
                containers: Some(vec![ContainerFormat::Mp4]),
                ..Default::default()
            }),
        );
        map.insert(
            "1080p".to_string(),
            Some(TranscodeProfile {
                bitrate: Some(10000),
                dimensions: Some((1920, 1080)),
                audio_channels: Some(2),
                h264_profiles: Some(vec![
                    H264Profile::Baseline,
                    H264Profile::Main,
                    H264Profile::High,
                ]),
                h264_level: Some("51".to_string()),
                containers: Some(vec![ContainerFormat::Mp4]),
                ..Default::default()
            }),
        );
        map
    };
}

async fn safe_write(
    path: impl AsRef<Path>,
    data: impl AsRef<[u8]>,
) -> result::Result<(), io::Error> {
    let mut temp_file = path.as_ref().to_owned();
    let Some(file_name) = temp_file.file_name() else {
        return write(path, data).await;
    };

    let mut file_name = file_name.to_owned();
    file_name.push(".temp");
    temp_file.set_file_name(file_name);

    write(&temp_file, data).await?;

    rename(temp_file, path).await
}

struct Inner {
    config: RwLock<Config>,
    state: RwLock<State>,
    path: PathBuf,
    servers: Mutex<HashMap<String, Server>>,
    download_permits: Arc<Semaphore>,
}

impl Inner {
    async fn output_style(&self) -> OutputStyle {
        self.config.read().await.output_style
    }

    async fn transcode_options(&self, profile: &str) -> Option<VideoTranscodeOptions> {
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
    }

    async fn persist_config(&self, config: &RwLockWriteGuard<'_, Config>) -> Result {
        let str = to_string_pretty(&config.deref())?;
        safe_write(self.path.join(CONFIG_FILE), str).await?;

        Ok(())
    }

    async fn persist_state(&self, state: &RwLockWriteGuard<'_, State>) -> Result {
        let str = to_string_pretty(&state.deref())?;
        safe_write(self.path.join(STATE_FILE), str).await?;

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

    pub async fn new(path: &Path) -> Result<Self> {
        let config = Config::read_or_default(&path.join(CONFIG_FILE)).await?;
        let state = State::read_or_default(&path.join(STATE_FILE)).await?;

        Ok(Self {
            inner: Arc::new(Inner {
                download_permits: Arc::new(Semaphore::new(config.max_downloads.unwrap_or(4))),
                config: RwLock::new(config),
                state: RwLock::new(state),
                path: path.to_owned(),
                servers: Default::default(),
            }),
        })
    }

    pub fn root(&self) -> &Path {
        &self.inner.path
    }

    pub async fn transcode_profiles(&self) -> Vec<String> {
        let config = self.inner.config.read().await;
        DEFAULT_PROFILES
            .keys()
            .chain(config.profiles.keys())
            .cloned()
            .collect()
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
            bail!("Server already exists");
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

    pub async fn on_deck(&self) -> Vec<Video> {
        #[allow(clippy::mutable_key_type)]
        let mut items: HashSet<Video> = HashSet::new();
        let now = OffsetDateTime::now_utc();

        for server in self.servers().await {
            for video in server.videos().await {
                match video.playback_state().await {
                    PlaybackState::Played => {
                        if let Some(last_played) = video.last_played().await {
                            if (now - last_played).whole_days() <= 7 {
                                if let Some(next) = video.next_video().await {
                                    if next.playback_state().await == PlaybackState::Unplayed
                                        && next.is_downloaded().await
                                    {
                                        items.insert(next);
                                    }
                                }
                            }
                        }
                    }
                    PlaybackState::InProgress { .. } => {
                        if video.is_downloaded().await {
                            items.insert(video);
                        }
                    }
                    _ => {}
                }
            }
        }

        items.into_iter().collect()
    }

    pub async fn server(&self, id: &str) -> Option<Server> {
        let mut servers = self.inner.servers.lock().await;
        if let Some(server) = servers.get(id) {
            return Some(server.clone());
        }

        let config = self.inner.config.read().await;
        let server_config = config.servers.get(id)?;

        let server = Server::new(id, &self.inner, server_config);
        servers.insert(id.to_owned(), server.clone());

        Some(server)
    }

    pub async fn servers(&self) -> Vec<Server> {
        let mut servers = self.inner.servers.lock().await;

        let config = self.inner.config.read().await;
        config
            .servers
            .iter()
            .map(|(id, server_config)| {
                servers.get(id).cloned().unwrap_or_else(|| {
                    let server = Server::new(id, &self.inner, server_config);
                    servers.insert(id.to_owned(), server.clone());
                    server
                })
            })
            .collect()
    }

    #[instrument(skip_all)]
    pub async fn prune_root(&self) {
        info!("Pruning root filesystem");

        let config: RwLockReadGuard<'_, Config> = self.inner.config.read().await;

        let servers: HashSet<String> = config.servers.keys().cloned().collect();

        let root = self.inner.path.clone();

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
                                        if e.kind() != ErrorKind::NotFound {
                                            error!(error=?e, path=%path.display(), "Failed to delete unknown file");
                                        }
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
