use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
    fmt,
    future::ready,
    io::ErrorKind,
    path::{Path, PathBuf},
    result,
    sync::Arc,
};

use anyhow::{anyhow, bail};
use async_recursion::async_recursion;
use futures::future::join_all;
use plex_api::{
    MyPlexBuilder,
    device::DeviceConnection,
    library::{
        Episode, FromMetadata, Item, Library as PlexLibrary, MediaItem, MetadataItem, Movie,
        Playlist, Season, Show, Video,
    },
    media_container::server::library::MetadataType,
};
use scoped_futures::{ScopedBoxFuture, ScopedFutureExt};
use tokio::{
    fs::{read_dir, remove_dir, remove_dir_all, remove_file},
    sync::{Mutex, RwLockMappedWriteGuard, RwLockWriteGuard, Semaphore},
};
use tracing::{debug, error, info, instrument, trace, warn};

use crate::{
    Collection, DEFAULT_PROFILE, DEFAULT_PROFILES, FileType, Inner, Library, OutputStyle, Result,
    ServerConnection, TransferState, VideoStats,
    config::{Config, ServerConfig, SyncItem, TranscodeProfile},
    state::{
        CollectionState, DownloadState, LibraryState, LibraryType, PlaylistState, SeasonState,
        ServerState, ShowState, VideoState,
    },
    sync::{OpMutex, OpReadGuard, OpWriteGuard, Timeout},
    util::safe,
    wrappers,
};

pub enum ItemType {
    Playlist,
    MovieCollection,
    ShowCollection,
    Show,
    Season,
    Episode,
    Movie,
    Unknown,
}

pub trait Progress: Unpin + Sized + Send + Sync {
    fn progress(&mut self, position: u64);

    fn length(&mut self, length: u64);

    fn finished(self) {}

    fn failed(self, #[expect(unused)] error: anyhow::Error) {}
}

pub trait DownloadProgress: Clone {
    fn transcode_started(
        &self,
        video_part: &wrappers::VideoPart,
    ) -> impl Future<Output = impl Progress + Clone + 'static>;

    fn download_started(
        &self,
        video_part: &wrappers::VideoPart,
    ) -> impl Future<Output = impl Progress + Clone + 'static>;

    fn download_failed(
        &self,
        #[expect(unused)] video_part: &wrappers::VideoPart,
        #[expect(unused)] error: anyhow::Error,
    ) -> impl Future<Output = ()> {
        ready(())
    }
}

pub struct SyncItemInfo {
    pub(super) server: Server,
    pub id: String,
    pub item_type: ItemType,
    pub title: String,
    pub transcode_profile: Option<String>,
    pub only_unplayed: bool,
}

impl SyncItemInfo {
    async fn show_stats(show: wrappers::Show) -> VideoStats {
        let mut stats = VideoStats::default();

        for season in show.seasons().await {
            stats += Self::season_stats(season).await;
        }

        stats
    }

    async fn season_stats(season: wrappers::Season) -> VideoStats {
        let mut stats = VideoStats::default();

        for video in season.episodes().await {
            stats += video.stats().await;
        }

        stats
    }

    pub async fn stats(&self) -> VideoStats {
        if let Some(playlist) = self.server.playlist(&self.id).await {
            let mut stats = VideoStats::default();

            for video in playlist.videos().await {
                stats += video.stats().await;
            }

            stats
        } else if let Some(collection) = self.server.collection(&self.id).await {
            let mut stats = VideoStats::default();

            match collection {
                Collection::Movie(c) => {
                    for video in c.movies().await {
                        stats += video.stats().await;
                    }
                }
                Collection::Show(c) => {
                    for show in c.shows().await {
                        stats += Self::show_stats(show).await
                    }
                }
            }
            stats
        } else if let Some(show) = self.server.show(&self.id).await {
            Self::show_stats(show).await
        } else if let Some(season) = self.server.season(&self.id).await {
            Self::season_stats(season).await
        } else if let Some(video) = self.server.video(&self.id).await {
            video.stats().await
        } else {
            VideoStats::default()
        }
    }
}

#[derive(Clone)]
pub struct Server {
    pub(crate) id: String,
    pub(crate) inner: Arc<Inner>,
    connection: Arc<Mutex<Option<plex_api::Server>>>,
    pub(crate) transcode_requests: Arc<Semaphore>,
    pub(crate) transcode_permits: Arc<Semaphore>,
}

impl fmt::Debug for Server {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Server").field("id", &self.id).finish()
    }
}

#[async_recursion]
async fn prune_directory(path: &Path, expected_files: &HashSet<PathBuf>) -> bool {
    let mut reader = match read_dir(&path).await {
        Ok(reader) => reader,
        Err(e) => {
            error!(error=?e, path=%path.display(), "Failed to read directory");
            return false;
        }
    };

    let mut should_prune = true;

    loop {
        match reader.next_entry().await {
            Ok(Some(entry)) => {
                let path: PathBuf = entry.path();
                match entry.file_type().await {
                    Ok(file_type) => {
                        if file_type.is_dir() {
                            if !prune_directory(&path, expected_files).await {
                                should_prune = false;
                            }
                        } else if !expected_files.contains(&path) {
                            match remove_file(&path).await {
                                Ok(()) => {
                                    debug!(path = %path.display(), "Deleted unknown file");
                                }
                                Err(e) => {
                                    if e.kind() != ErrorKind::NotFound {
                                        error!(error=?e, path=%path.display(), "Failed to delete unknown file");
                                        should_prune = false;
                                    }
                                }
                            }
                        } else {
                            should_prune = false;
                        }
                    }
                    Err(e) => {
                        error!(error=?e, path=%path.display(), "Failed to read file type");
                    }
                }
            }
            Ok(None) => {
                break;
            }
            Err(e) => {
                error!(error=?e, path=%path.display(), "Failed to read directory");
                return false;
            }
        }
    }

    if should_prune {
        match remove_dir(&path).await {
            Ok(()) => {
                debug!(path = %path.display(), "Deleted unknown directory");
                return true;
            }
            Err(e) => {
                error!(error=?e, path=%path.display(), "Failed to delete unknown directory");
            }
        }
    }

    false
}

impl Server {
    pub(crate) fn new(id: &str, inner: &Arc<Inner>, config: &ServerConfig) -> Self {
        Self {
            id: id.to_owned(),
            inner: inner.clone(),
            connection: Arc::new(Mutex::new(None)),
            transcode_requests: Arc::new(Semaphore::new(1)),
            transcode_permits: Arc::new(Semaphore::new(config.max_transcodes.unwrap_or(2))),
        }
    }

    pub async fn delete(self) -> Result {
        #[expect(unused)]
        let guard = self.try_lock_write().await?;

        let mut servers = self.inner.servers.lock().await;
        let mut config = self.inner.config.write().await;
        let mut state = self.inner.state.write().await;

        config.servers.remove(&self.id);
        self.inner.persist_config(&config).await?;

        servers.remove(&self.id);

        state.servers.remove(&self.id);
        self.inner.persist_state(&state).await?;

        Ok(())
    }

    pub(crate) async fn try_lock_write(&self) -> result::Result<OpWriteGuard, Timeout> {
        OpMutex::try_lock_write_key(self.id.clone()).await
    }

    pub(crate) async fn try_lock_write_key(
        &self,
        key: &str,
    ) -> result::Result<OpWriteGuard, Timeout> {
        OpMutex::try_lock_write_key(format!("{}/{key}", self.id)).await
    }

    pub(crate) async fn try_lock_read_key(
        &self,
        key: &str,
    ) -> result::Result<OpReadGuard, Timeout> {
        OpMutex::try_lock_read_key(format!("{}/{key}", self.id)).await
    }

    /// The FlickSync identifier for this server.
    pub fn id(&self) -> &str {
        &self.id
    }

    pub async fn list_syncs(&self) -> Vec<SyncItemInfo> {
        let config = self.inner.config.read().await;
        let server_config = config.servers.get(&self.id).unwrap();

        let mut results: Vec<SyncItemInfo> = Vec::new();

        for sync in server_config.syncs.values() {
            let (title, item_type) = if let Some(playlist) = self.playlist(&sync.id).await {
                (playlist.title().await, ItemType::Playlist)
            } else if let Some(collection) = self.collection(&sync.id).await {
                match collection {
                    Collection::Movie(c) => (c.title().await, ItemType::MovieCollection),
                    Collection::Show(c) => (c.title().await, ItemType::ShowCollection),
                }
            } else if let Some(show) = self.show(&sync.id).await {
                (show.title().await, ItemType::Show)
            } else if let Some(season) = self.season(&sync.id).await {
                (season.title().await, ItemType::Season)
            } else if let Some(video) = self.video(&sync.id).await {
                match video {
                    wrappers::Video::Episode(_) => (video.title().await, ItemType::Episode),
                    _ => (video.title().await, ItemType::Movie),
                }
            } else {
                ("Unknown".to_string(), ItemType::Unknown)
            };

            results.push(SyncItemInfo {
                server: self.clone(),
                id: sync.id.clone(),
                item_type,
                title,
                transcode_profile: sync.transcode_profile.clone(),
                only_unplayed: sync.only_unplayed,
            });
        }

        results
    }

    pub async fn transcode_profile(&self) -> String {
        let config = self.inner.config.read().await;
        let server_config = config.servers.get(&self.id).unwrap();
        server_config
            .transcode_profile
            .clone()
            .unwrap_or_else(|| DEFAULT_PROFILE.to_string())
    }

    pub async fn connection(&self) -> ServerConnection {
        let config = self.inner.config.read().await;
        let server_config = config.servers.get(&self.id).unwrap();
        server_config.connection.clone()
    }

    pub async fn update_connection(&self, auth_token: &str, server: plex_api::Server) -> Result {
        let mut connection = self.connection.lock().await;
        let mut state = self.inner.state.write().await;

        let server_state = state.servers.entry(self.id.to_owned()).or_default();
        server_state.token = auth_token.to_owned();
        server_state.name = server.media_container.friendly_name;

        self.inner.persist_state(&state).await?;
        *connection = None;

        Ok(())
    }

    pub async fn name(&self) -> String {
        let state = self.inner.state.read().await;
        state.servers.get(&self.id).unwrap().name.clone()
    }

    pub async fn video(&self, id: &str) -> Option<wrappers::Video> {
        let state = self.inner.state.read().await;
        state
            .servers
            .get(&self.id)
            .unwrap()
            .videos
            .get(id)
            .map(|vs| wrappers::Video::wrap(self, vs))
    }

    pub async fn videos(&self) -> Vec<wrappers::Video> {
        let state = self.inner.state.read().await;
        state
            .servers
            .get(&self.id)
            .unwrap()
            .videos
            .values()
            .map(|vs| wrappers::Video::wrap(self, vs))
            .collect()
    }

    pub async fn show(&self, id: &str) -> Option<wrappers::Show> {
        let state = self.inner.state.read().await;
        state
            .servers
            .get(&self.id)
            .unwrap()
            .shows
            .get(id)
            .map(|ls| wrappers::Show::wrap(self, ls))
    }

    pub async fn shows(&self) -> Vec<wrappers::Show> {
        let state = self.inner.state.read().await;
        state
            .servers
            .get(&self.id)
            .unwrap()
            .shows
            .values()
            .map(|ls| wrappers::Show::wrap(self, ls))
            .collect()
    }

    pub async fn season(&self, id: &str) -> Option<wrappers::Season> {
        let state = self.inner.state.read().await;
        state
            .servers
            .get(&self.id)
            .unwrap()
            .seasons
            .get(id)
            .map(|ls| wrappers::Season::wrap(self, ls))
    }

    pub async fn seasons(&self) -> Vec<wrappers::Season> {
        let state = self.inner.state.read().await;
        state
            .servers
            .get(&self.id)
            .unwrap()
            .seasons
            .values()
            .map(|ls| wrappers::Season::wrap(self, ls))
            .collect()
    }

    pub async fn library(&self, id: &str) -> Option<wrappers::Library> {
        let state = self.inner.state.read().await;
        state
            .servers
            .get(&self.id)
            .unwrap()
            .libraries
            .get(id)
            .map(|ls| wrappers::Library::wrap(self, ls))
    }

    pub async fn libraries(&self) -> Vec<wrappers::Library> {
        let state = self.inner.state.read().await;
        state
            .servers
            .get(&self.id)
            .unwrap()
            .libraries
            .values()
            .map(|ls| wrappers::Library::wrap(self, ls))
            .collect()
    }

    pub async fn playlist(&self, id: &str) -> Option<wrappers::Playlist> {
        let state = self.inner.state.read().await;
        state
            .servers
            .get(&self.id)
            .unwrap()
            .playlists
            .get(id)
            .map(|state| wrappers::Playlist::wrap(self, state))
    }

    pub async fn playlists(&self) -> Vec<wrappers::Playlist> {
        let state = self.inner.state.read().await;
        state
            .servers
            .get(&self.id)
            .unwrap()
            .playlists
            .values()
            .map(|state| wrappers::Playlist::wrap(self, state))
            .collect()
    }

    pub async fn collection(&self, id: &str) -> Option<wrappers::Collection> {
        let state = self.inner.state.read().await;
        let server_state = state.servers.get(&self.id).unwrap();

        server_state.collections.get(id).map(|cs| {
            let library = server_state.libraries.get(&cs.library).unwrap();

            match library.library_type {
                LibraryType::Movie => {
                    wrappers::Collection::Movie(wrappers::MovieCollection::wrap(self, cs))
                }
                LibraryType::Show => {
                    wrappers::Collection::Show(wrappers::ShowCollection::wrap(self, cs))
                }
            }
        })
    }

    pub async fn collections(&self) -> Vec<wrappers::Collection> {
        let state = self.inner.state.read().await;
        let server_state = state.servers.get(&self.id).unwrap();

        server_state
            .collections
            .values()
            .map(|cs| {
                let library = server_state.libraries.get(&cs.library).unwrap();

                match library.library_type {
                    LibraryType::Movie => {
                        wrappers::Collection::Movie(wrappers::MovieCollection::wrap(self, cs))
                    }
                    LibraryType::Show => {
                        wrappers::Collection::Show(wrappers::ShowCollection::wrap(self, cs))
                    }
                }
            })
            .collect()
    }

    /// Connects to the Plex API for this server.
    #[instrument(level = "trace", skip(self), fields(server = self.id))]
    pub async fn connect(&self) -> Result<plex_api::Server> {
        let mut connection = self.connection.lock().await;

        if let Some(api) = connection.take() {
            if let Ok(api) = api.refresh().await {
                *connection = Some(api.clone());
                return Ok(api);
            }
        }

        let config = self.inner.config.read().await;
        let state = self.inner.state.read().await;

        let server_config = config.servers.get(&self.id).unwrap();

        let mut client = self.inner.client().await;

        match &server_config.connection {
            ServerConnection::MyPlex {
                user_id, device_id, ..
            } => {
                let token = state
                    .servers
                    .get(&self.id)
                    .ok_or_else(|| anyhow!("No longer authenticated."))?
                    .token
                    .clone();

                let myplex = MyPlexBuilder::default()
                    .set_client(client)
                    .set_token(token)
                    .set_test_token_auth(false)
                    .build()
                    .await?;

                let home = myplex.home()?;
                let myplex = home.switch_user(myplex, user_id.clone(), None).await?;

                let manager = myplex.device_manager()?;
                let device = match manager
                    .resources()
                    .await?
                    .into_iter()
                    .find(|d| d.identifier() == device_id)
                {
                    Some(d) => d,
                    None => bail!("Server not found"),
                };

                match device.connect().await? {
                    DeviceConnection::Server(server) => {
                        trace!(url=%server.client().api_url,
                            "Connected to server"
                        );
                        *connection = Some(server.as_ref().clone());
                        Ok(*server)
                    }
                    _ => panic!("Unexpected client connection"),
                }
            }
            ServerConnection::Direct { url } => {
                let token = state
                    .servers
                    .get(&self.id)
                    .map(|s| s.token.clone())
                    .unwrap_or_default();
                client = client.set_x_plex_token(token);

                let server = plex_api::Server::new(url, client).await?;
                trace!(url=%server.client().api_url,
                    "Connected to server",
                );
                *connection = Some(server.clone());

                Ok(server)
            }
        }
    }

    /// Adds an item to sync based on its rating key.
    pub async fn add_sync(
        &self,
        rating_key: &str,
        transcode_profile: Option<String>,
        only_unplayed: bool,
    ) -> Result {
        #[expect(unused)]
        let guard = self.try_lock_write().await?;

        let mut config = self.inner.config.write().await;

        if let Some(ref profile) = transcode_profile {
            if !config.profiles.contains_key(profile) && !DEFAULT_PROFILES.contains_key(profile) {
                bail!("Unknown profile: {profile}");
            }
        }

        let server_config = config.servers.get_mut(&self.id).unwrap();
        server_config.syncs.insert(
            rating_key.to_owned(),
            SyncItem {
                id: rating_key.to_owned(),
                transcode_profile,
                only_unplayed,
            },
        );

        self.inner.persist_config(&config).await
    }

    /// Removes an item to sync based on its rating key. Returns true if the item existed.
    pub async fn remove_sync(&self, rating_key: &str) -> Result<bool> {
        #[expect(unused)]
        let guard = self.try_lock_write().await?;

        let mut config = self.inner.config.write().await;

        let server_config = config.servers.get_mut(&self.id).unwrap();
        let contained = server_config.syncs.remove(rating_key).is_some();

        self.inner.persist_config(&config).await?;

        Ok(contained)
    }

    /// Updates the state for the synced items
    pub async fn update_state(&self, allow_video_deletion: bool) -> Result {
        info!("Updating item metadata");
        let plex_server = self.connect().await?;

        {
            #[expect(unused)]
            let guard = self.try_lock_write().await?;

            let config = self.inner.config.read().await.clone();
            let server_config = config.servers.get(&self.id).unwrap();

            {
                let mut state = self.inner.state.write().await;

                let server_state = state.servers.entry(self.id.clone()).or_default();
                server_state.name = plex_server.media_container.friendly_name.clone();
            }

            let mut state_sync = StateSync {
                config: &config,
                server_config,
                server: self,
                plex_server,
                root: &self.inner.path,
                seen_items: Default::default(),
                seen_libraries: Default::default(),
                transcode_profiles: Default::default(),
                allow_video_deletion,
            };

            for item in server_config.syncs.values() {
                if let Err(e) = state_sync.add_item_by_key(item, &item.id).await {
                    warn!(item=item.id, error=?e, "Failed to update item. Skipping.");
                }
            }

            if allow_video_deletion {
                state_sync.update_profiles().await?;
            }

            state_sync.fetch_collections().await?;

            if allow_video_deletion {
                state_sync.prune_unseen().await?;
            }

            let state = self.inner.state.write().await;
            self.inner.persist_state(&state).await?;
        }

        self.update_thumbnails(false).await;
        self.update_metadata(false).await;
        self.verify_downloads(allow_video_deletion).await;
        self.write_playlists().await;

        Ok(())
    }

    /// Writes out the playlist files for playlists and collections
    pub async fn write_playlists(&self) {
        if self.inner.output_style().await != OutputStyle::Standardized {
            return;
        }

        info!("Writing playlists");

        for collection in self.collections().await {
            if let Err(e) = collection.write_playlist().await {
                warn!(error=?e, "Failed to update playlist");
            }
        }

        for playlist in self.playlists().await {
            if let Err(e) = playlist.write_playlist().await {
                warn!(error=?e, "Failed to update playlist");
            }
        }
    }

    /// Rebuilds metadata files for the synced items
    pub async fn rebuild_metadata(&self) -> Result {
        info!("Rebuilding item metadata");

        self.update_thumbnails(true).await;
        self.update_metadata(true).await;
        self.write_playlists().await;

        Ok(())
    }

    /// Updates thumbnails for synced items.
    #[instrument(level = "trace", skip(self), fields(server = self.id))]
    async fn update_thumbnails(&self, rebuild: bool) {
        info!("Updating thumbnails");

        for playlist in self.playlists().await {
            if let Err(e) = playlist.update_thumbnail(rebuild).await {
                warn!(error=?e);
            }
        }

        for library in self.libraries().await {
            for collection in library.collections().await {
                if let Err(e) = collection.update_thumbnail(rebuild).await {
                    warn!(error=?e);
                }
            }

            match library {
                Library::Movie(l) => {
                    for video in l.movies().await {
                        if let Err(e) = video.update_thumbnail(rebuild).await {
                            warn!(error=?e);
                        }
                    }
                }
                Library::Show(l) => {
                    for show in l.shows().await {
                        if let Err(e) = show.update_thumbnail(rebuild).await {
                            warn!(error=?e);
                        }

                        for season in show.seasons().await {
                            for video in season.episodes().await {
                                if let Err(e) = video.update_thumbnail(rebuild).await {
                                    warn!(error=?e);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// Updates metadata files for synced videos.
    #[instrument(level = "trace", skip(self), fields(server = self.id))]
    async fn update_metadata(&self, rebuild: bool) {
        info!("Updating metadata files");

        for library in self.libraries().await {
            match library {
                Library::Movie(l) => {
                    for video in l.movies().await {
                        if let Err(e) = video.update_metadata(rebuild).await {
                            warn!(error=?e);
                        }

                        if rebuild && self.inner.output_style().await == OutputStyle::Standardized {
                            for part in video.parts().await {
                                part.strip_metadata().await;
                            }
                        }
                    }
                }
                Library::Show(l) => {
                    for show in l.shows().await {
                        if let Err(e) = show.update_metadata(rebuild).await {
                            warn!(error=?e);
                        }

                        for season in show.seasons().await {
                            for video in season.episodes().await {
                                if let Err(e) = video.update_metadata(rebuild).await {
                                    warn!(error=?e);
                                }

                                if rebuild
                                    && self.inner.output_style().await == OutputStyle::Standardized
                                {
                                    for part in video.parts().await {
                                        part.strip_metadata().await;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// Verifies the presence of downloads for synced items.
    #[instrument(level = "trace", skip(self), fields(server = self.id))]
    async fn verify_downloads(&self, allow_video_deletion: bool) {
        let plex_server = match self.connect().await {
            Ok(ps) => ps,
            Err(e) => {
                warn!(error=%e, "Unable to connect to Plex");
                return;
            }
        };

        info!("Verifying downloads");

        for video in self.videos().await {
            for part in video.parts().await {
                if let Err(e) = part
                    .verify_download(&plex_server, allow_video_deletion)
                    .await
                {
                    warn!(error=?e);
                }
            }
        }
    }

    /// Attempts to transcode and download all missing items.
    #[instrument(level = "trace", skip(self, progress), fields(server = self.id))]
    pub async fn download<D>(&self, progress: D) -> Result<bool>
    where
        D: DownloadProgress,
    {
        let plex_server = match self.connect().await {
            Ok(ps) => ps,
            Err(e) => {
                warn!(error=%e, "Unable to connect to plex server");
                return Err(e);
            }
        };

        let mut jobs = Vec::new();

        for video in self.videos().await {
            for part in video.parts().await {
                match part.transfer_state().await {
                    TransferState::Transcoding => {
                        jobs.push(part.download(
                            plex_server.clone(),
                            progress.clone(),
                            None,
                            self.transcode_permits.clone().try_acquire_owned().ok(),
                            self.inner.download_permits.clone().try_acquire_owned().ok(),
                        ));
                    }
                    TransferState::Downloading => {
                        jobs.push(part.download(
                            plex_server.clone(),
                            progress.clone(),
                            None,
                            None,
                            self.inner.download_permits.clone().try_acquire_owned().ok(),
                        ));
                    }
                    TransferState::Waiting => {
                        jobs.push(part.download(
                            plex_server.clone(),
                            progress.clone(),
                            None,
                            None,
                            None,
                        ));
                    }
                    TransferState::Downloaded => {}
                }
            }
        }

        Ok(join_all(jobs).await.into_iter().all(|r| r))
    }

    /// Verifies the presence of downloads for synced items.
    #[instrument(level = "trace", skip(self), fields(server = self.id))]
    pub async fn prune(&self) -> Result {
        #[expect(unused)]
        let guard = self.try_lock_write().await?;
        info!("Pruning server filesystem");

        let output_standardized = self.inner.output_style().await == OutputStyle::Standardized;

        let mut expected_files: HashSet<PathBuf> = HashSet::new();

        let state = self.inner.state.read().await;

        let server_state = match state.servers.get(&self.id) {
            Some(s) => s,
            None => return Ok(()),
        };

        for playlist in server_state.playlists.values() {
            if let Some(file) = playlist.thumbnail.path() {
                expected_files.insert(self.inner.path.join(file));
            }
        }

        for collection in server_state.collections.values() {
            if let Some(file) = collection.thumbnail.path() {
                expected_files.insert(self.inner.path.join(file));
            }
        }

        if output_standardized {
            for collection in self.collections().await {
                expected_files.insert(
                    self.inner
                        .path
                        .join(collection.file_path(FileType::Playlist, "m3u").await),
                );
            }
        }

        for show in server_state.shows.values() {
            if let Some(file) = show.thumbnail.path() {
                expected_files.insert(self.inner.path.join(file));
            }

            if let Some(file) = show.metadata.path() {
                expected_files.insert(self.inner.path.join(file));
            }
        }

        for video in server_state.videos.values() {
            if let Some(file) = video.thumbnail.path() {
                expected_files.insert(self.inner.path.join(file));
            }

            if let Some(file) = video.metadata.path() {
                expected_files.insert(self.inner.path.join(file));
            }

            for part in video.parts.iter() {
                if let Some(file) = part.download.path() {
                    expected_files.insert(self.inner.path.join(file));
                }
            }
        }

        if output_standardized {
            for playlist in self.playlists().await {
                expected_files.insert(
                    self.inner
                        .path
                        .join(playlist.file_path(FileType::Playlist, "m3u").await),
                );
            }
        }

        let server_root = self.inner.path.join(safe(&self.id));

        if expected_files.is_empty() {
            debug!("Deleting empty server directory {}", server_root.display());
            if let Err(e) = remove_dir_all(&server_root).await {
                if e.kind() != ErrorKind::NotFound {
                    warn!(error=%e, "Failed to remove empty server directory.");
                }
            }
            return Ok(());
        }

        prune_directory(&server_root, &expected_files).await;

        Ok(())
    }
}

struct StateSync<'a> {
    config: &'a Config,
    server_config: &'a ServerConfig,
    plex_server: plex_api::Server,
    server: &'a Server,
    root: &'a Path,
    seen_items: HashSet<String>,
    seen_libraries: HashSet<String>,
    transcode_profiles: HashMap<String, HashSet<String>>,
    allow_video_deletion: bool,
}

macro_rules! return_if_seen {
    ($self:expr, $typ:expr) => {
        if $self.seen_items.contains($typ.rating_key()) {
            return Ok(());
        }
        $self.seen_items.insert($typ.rating_key().to_owned());
    };
}

impl StateSync<'_> {
    async fn server_state(&self) -> RwLockMappedWriteGuard<'_, ServerState> {
        RwLockWriteGuard::map(self.server.inner.state.write().await, |state| {
            state.servers.get_mut(&self.server.id).unwrap()
        })
    }

    async fn add_video<T: MediaItem + FromMetadata>(&mut self, sync: &SyncItem, video: &T) {
        if sync.only_unplayed && video.metadata().view_count.unwrap_or_default() > 0 {
            return;
        }

        let key = video.rating_key().to_owned();

        if !self.seen_items.contains(&key) {
            self.seen_items.insert(key.clone());

            let mut server_state = self.server_state().await;
            let video_state = server_state
                .videos
                .entry(key.clone())
                .or_insert_with(|| VideoState::from(video));

            video_state
                .update(
                    self.server,
                    video,
                    &self.plex_server,
                    self.root,
                    self.allow_video_deletion,
                )
                .await;
        }

        if let Some(ref profile) = sync.transcode_profile {
            let profiles = self.transcode_profiles.entry(key).or_default();
            profiles.insert(profile.clone());
        }
    }

    async fn add_movie(&mut self, sync: &SyncItem, movie: &Movie) -> Result {
        self.add_video(sync, movie).await;

        self.add_library(movie).await?;

        Ok(())
    }

    async fn add_episode(&mut self, sync: &SyncItem, episode: &Episode) -> Result {
        self.add_video(sync, episode).await;

        Ok(())
    }

    async fn add_season(&mut self, season: &Season) -> Result {
        return_if_seen!(self, season);

        let mut server_state = self.server_state().await;
        server_state
            .seasons
            .entry(season.rating_key().to_owned())
            .and_modify(|ss| ss.update(season))
            .or_insert_with(|| SeasonState::from(season));

        Ok(())
    }

    async fn add_show(&mut self, show: &Show) -> Result {
        return_if_seen!(self, show);

        self.add_library(show).await?;

        let mut server_state = self.server_state().await;
        let show_state = server_state
            .shows
            .entry(show.rating_key().to_owned())
            .or_insert_with(|| ShowState::from(show));

        show_state.update(show).await;

        Ok(())
    }

    async fn add_library<T>(&mut self, item: &T) -> Result
    where
        T: MetadataItem,
    {
        let library_id = item
            .metadata()
            .library_section_id
            .ok_or(anyhow!("Unexpected response from Plex server"))?
            .to_string();
        let library_title = item
            .metadata()
            .library_section_title
            .clone()
            .ok_or(anyhow!("Unexpected response from Plex server"))?;

        let library_type = match item.metadata().metadata_type.as_ref().unwrap() {
            MetadataType::Movie => LibraryType::Movie,
            MetadataType::Show => LibraryType::Show,
            _ => panic!("Unknown library type"),
        };

        self.seen_libraries.insert(library_id.clone());

        let mut server_state = self.server_state().await;
        server_state
            .libraries
            .entry(library_id.clone())
            .and_modify(|l| l.title = library_title.clone())
            .or_insert_with(|| LibraryState {
                id: library_id,
                title: library_title.clone(),
                library_type,
            });

        Ok(())
    }

    async fn add_playlist(&mut self, playlist: &Playlist<Video>, videos: Vec<String>) -> Result {
        return_if_seen!(self, playlist);

        let mut server_state = self.server_state().await;
        let playlist_state = server_state
            .playlists
            .entry(playlist.rating_key().to_owned())
            .and_modify(|ps| ps.update(playlist))
            .or_insert_with(|| PlaylistState::from(playlist));
        playlist_state.videos = videos;

        Ok(())
    }

    fn select_profile(&self, profiles: &HashSet<String>) -> Option<String> {
        let mut profile_list: Vec<(String, Option<TranscodeProfile>)> = profiles
            .iter()
            .filter_map(|name| {
                if let Some(profile) = self.config.profiles.get(name) {
                    Some((name.clone(), Some(profile.clone())))
                } else if let Some(profile) = DEFAULT_PROFILES.get(name) {
                    Some((name.clone(), profile.clone()))
                } else {
                    warn!("Unknown transcode profile: '{name}'");
                    None
                }
            })
            .collect();

        profile_list.sort_unstable_by(|(na, pa), (nb, pb)| {
            match (pa, pb) {
                (Some(_), None) => return Ordering::Greater,
                (None, Some(_)) => return Ordering::Less,
                (Some(a), Some(b)) => {
                    // Note reversed sort
                    match b.partial_cmp(a) {
                        Some(Ordering::Equal) | None => {
                            warn!("Unable to compare transcode profiles {na} and {nb}.");
                        }
                        Some(o) => {
                            return o;
                        }
                    }
                }
                _ => {}
            }

            na.cmp(nb)
        });

        profile_list.into_iter().next().map(|(name, _)| name)
    }

    async fn update_profiles(&mut self) -> Result {
        for (key, selected_profiles) in self.transcode_profiles.iter() {
            if let Ok(guard) =
                OpMutex::try_lock_write_key(format!("{}/{}", self.server.id, key)).await
            {
                let selected_profile = self
                    .select_profile(selected_profiles)
                    .or_else(|| self.server_config.transcode_profile.clone())
                    .unwrap_or_else(|| DEFAULT_PROFILE.to_string());

                let mut server_state = self.server_state().await;
                let video_state = server_state.videos.get_mut(key).unwrap();
                if video_state.transcode_profile.as_ref() != Some(&selected_profile) {
                    if video_state
                        .parts
                        .iter()
                        .any(|p| p.download != DownloadState::None)
                    {
                        info!(item=key, old=?video_state.transcode_profile, new=?selected_profile, "Transcode profile changed, deleting existing downloads.");

                        for part in video_state.parts.iter_mut() {
                            part.download
                                .delete(&guard, &self.plex_server, self.root)
                                .await;
                        }
                    }

                    video_state.transcode_profile = Some(selected_profile);
                }
            }
        }

        Ok(())
    }

    async fn fetch_collections(&mut self) -> Result {
        for library in self.plex_server.libraries() {
            match library {
                PlexLibrary::Movie(lib) => {
                    let collections = lib.collections().await?;
                    for collection in collections {
                        let children = collection.children().await?;

                        let available: Vec<String> = children
                            .iter()
                            .filter_map(|movie| {
                                if self.seen_items.contains(movie.rating_key()) {
                                    Some(movie.rating_key().to_owned())
                                } else {
                                    None
                                }
                            })
                            .collect();

                        if !available.is_empty() {
                            self.seen_items.insert(collection.rating_key().to_owned());

                            let mut server_state = self.server_state().await;
                            let collection_state = server_state
                                .collections
                                .entry(collection.rating_key().to_owned())
                                .or_insert_with(|| CollectionState::from(&collection));
                            collection_state.contents = available;

                            collection_state.update(&collection).await;
                        }
                    }
                }
                PlexLibrary::TV(lib) => {
                    let collections = lib.collections().await?;
                    for collection in collections {
                        let children = collection.children().await?;

                        let available: Vec<String> = children
                            .iter()
                            .filter_map(|show| {
                                if self.seen_items.contains(show.rating_key()) {
                                    Some(show.rating_key().to_owned())
                                } else {
                                    None
                                }
                            })
                            .collect();

                        if !available.is_empty() {
                            self.seen_items.insert(collection.rating_key().to_owned());

                            let mut server_state = self.server_state().await;
                            let collection_state = server_state
                                .collections
                                .entry(collection.rating_key().to_owned())
                                .or_insert_with(|| CollectionState::from(&collection));
                            collection_state.contents = available;

                            collection_state.update(&collection).await;
                        }
                    }
                }
                _ => {}
            }
        }

        Ok(())
    }

    async fn prune_map<'a, V, M, P>(&mut self, mut mapper: M, mut pre_delete: P)
    where
        M: FnMut(&mut ServerState) -> &mut HashMap<String, V>,
        P: for<'b> FnMut(&'b mut V, &'b OpWriteGuard) -> ScopedBoxFuture<'a, 'b, ()> + Send,
        V: Clone,
    {
        let unseen_items: Vec<(String, V)> = {
            let state = self.server_state().await;
            let map = RwLockMappedWriteGuard::map(state, &mut mapper);

            map.iter()
                .filter_map(|(k, v)| {
                    if self.seen_items.contains(k) {
                        None
                    } else {
                        Some((k.clone(), v.clone()))
                    }
                })
                .collect()
        };

        let mut items_to_delete = HashSet::new();
        for (key, mut item) in unseen_items {
            if let Ok(guard) = self.server.try_lock_write_key(&key).await {
                pre_delete(&mut item, &guard).await;
                items_to_delete.insert(key);
            }
        }

        let state = self.server_state().await;
        let mut map = RwLockMappedWriteGuard::map(state, mapper);
        map.retain(|k, _v| !items_to_delete.contains(k))
    }

    async fn prune_unseen(&mut self) -> Result {
        info!("Pruning old items");

        let plex_server = self.plex_server.clone();
        self.prune_map(
            |ss| &mut ss.videos,
            |video, guard| video.delete(guard, &plex_server, self.root).scope_boxed(),
        )
        .await;

        self.prune_map(
            |ss| &mut ss.shows,
            |show, guard| show.delete(guard, self.root).scope_boxed(),
        )
        .await;

        self.prune_map(
            |ss| &mut ss.collections,
            |collection, guard| collection.delete(guard, self.root).scope_boxed(),
        )
        .await;

        self.server_state()
            .await
            .playlists
            .retain(|k, _v| self.seen_items.contains(k));

        self.server_state()
            .await
            .seasons
            .retain(|k, _v| self.seen_items.contains(k));

        self.server_state()
            .await
            .libraries
            .retain(|k, _v| self.seen_libraries.contains(k));

        Ok(())
    }

    async fn add_item_by_key(&mut self, sync: &SyncItem, key: &str) -> Result {
        match self.plex_server.item_by_id(key).await {
            Ok(i) => self.add_item(sync, i).await,
            Err(plex_api::Error::ItemNotFound) => {
                warn!(item = key, "Sync item no longer appears to exist");
                Ok(())
            }
            Err(e) => Err(e.into()),
        }
    }

    async fn add_show_contents(&mut self, sync_item: &SyncItem, show: &Show) -> Result {
        self.add_show(show).await?;

        for season in show.seasons().await? {
            self.add_season(&season).await?;

            for episode in season.episodes().await? {
                self.add_episode(sync_item, &episode).await?;
            }
        }

        Ok(())
    }

    async fn add_episode_with_parents(
        &mut self,
        sync_item: &SyncItem,
        episode: &Episode,
    ) -> Result {
        let season = episode
            .season()
            .await?
            .ok_or_else(|| anyhow!("Unexpected response from Plex server"))?;

        let show = season
            .show()
            .await?
            .ok_or_else(|| anyhow!("Unexpected response from Plex server"))?;
        self.add_show(&show).await?;

        self.add_season(&season).await?;

        self.add_episode(sync_item, episode).await
    }

    #[instrument(level = "trace", skip(self, sync, item), fields(item = item.rating_key()))]
    async fn add_item(&mut self, sync: &SyncItem, item: Item) -> Result {
        match item {
            Item::Movie(movie) => self.add_movie(sync, &movie).await,

            Item::Show(show) => self.add_show_contents(sync, &show).await,

            Item::Season(season) => {
                let show = season
                    .show()
                    .await?
                    .ok_or_else(|| anyhow!("Unexpected response from Plex server"))?;

                self.add_show(&show).await?;

                self.add_season(&season).await?;

                for episode in season.episodes().await? {
                    self.add_episode(sync, &episode).await?;
                }

                Ok(())
            }

            Item::Episode(episode) => self.add_episode_with_parents(sync, &episode).await,

            Item::MovieCollection(collection) => {
                let movies = collection.children().await?;
                for movie in movies {
                    self.add_movie(sync, &movie).await?;
                }

                Ok(())
            }

            Item::ShowCollection(collection) => {
                let shows = collection.children().await?;
                for show in shows {
                    self.add_show_contents(sync, &show).await?;
                }

                Ok(())
            }

            Item::VideoPlaylist(playlist) => {
                let mut items = Vec::new();
                let videos = playlist.children().await?;
                for video in videos {
                    let key = video.rating_key().to_owned();
                    let result = match video {
                        Video::Episode(episode) => {
                            self.add_episode_with_parents(sync, &episode).await
                        }

                        Video::Movie(movie) => self.add_movie(sync, &movie).await,
                    };

                    match result {
                        Ok(()) => {
                            items.push(key);
                        }
                        Err(e) => warn!(error=?e, "Failed to update item"),
                    }
                }

                self.add_playlist(&playlist, items).await
            }
            _ => bail!("Item type not supported"),
        }
    }
}
