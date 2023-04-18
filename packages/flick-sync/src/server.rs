use std::{collections::HashSet, fmt, path::Path, sync::Arc};

use async_recursion::async_recursion;
use plex_api::{
    device::DeviceConnection,
    library::{Collection, Episode, Item, MetadataItem, Movie, Playlist, Season, Show, Video},
    media_container::server::library::MetadataType,
    MyPlexBuilder,
};
use tracing::{info, instrument, trace, warn};

use crate::{
    config::SyncItem,
    state::{
        CollectionState, LibraryState, LibraryType, PlaylistState, SeasonState, ServerState,
        ShowState, VideoDetail, VideoState,
    },
    wrappers, Error, Inner, Library, Result, ServerConnection,
};

#[derive(Clone)]
pub struct Server {
    pub(crate) id: String,
    pub(crate) inner: Arc<Inner>,
}

impl fmt::Debug for Server {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Server").field("id", &self.id).finish()
    }
}

impl Server {
    /// The FlickSync identifier for this server.
    pub fn id(&self) -> &str {
        &self.id
    }

    pub async fn max_transcodes(&self) -> usize {
        let config = self.inner.config.read().await;
        let server_config = config.servers.get(&self.id).unwrap();
        server_config.max_transcodes.unwrap_or(2)
    }

    pub async fn videos(&self) -> Vec<wrappers::Video> {
        let state = self.inner.state.read().await;
        state
            .servers
            .get(&self.id)
            .unwrap()
            .videos
            .iter()
            .map(|(id, vs)| match vs.detail {
                VideoDetail::Movie(_) => wrappers::Video::Movie(wrappers::Movie {
                    server: self.id.clone(),
                    id: *id,
                    inner: self.inner.clone(),
                }),
                VideoDetail::Episode(_) => wrappers::Video::Episode(wrappers::Episode {
                    server: self.id.clone(),
                    id: *id,
                    inner: self.inner.clone(),
                }),
            })
            .collect()
    }

    pub async fn libraries(&self) -> Vec<wrappers::Library> {
        let state = self.inner.state.read().await;
        state
            .servers
            .get(&self.id)
            .unwrap()
            .libraries
            .iter()
            .map(|(id, ls)| match ls.library_type {
                LibraryType::Movie => wrappers::Library::Movie(wrappers::MovieLibrary {
                    server: self.id.clone(),
                    id: *id,
                    inner: self.inner.clone(),
                }),
                LibraryType::Show => wrappers::Library::Show(wrappers::ShowLibrary {
                    server: self.id.clone(),
                    id: *id,
                    inner: self.inner.clone(),
                }),
            })
            .collect()
    }

    pub async fn playlists(&self) -> Vec<wrappers::Playlist> {
        let state = self.inner.state.read().await;
        state
            .servers
            .get(&self.id)
            .unwrap()
            .playlists
            .keys()
            .map(|id| wrappers::Playlist {
                server: self.id.clone(),
                id: *id,
                inner: self.inner.clone(),
            })
            .collect()
    }

    /// Connects to the Plex API for this server.
    #[instrument(level = "trace", skip(self), fields(server = self.id))]
    pub async fn connect(&self) -> Result<plex_api::Server> {
        let mut servers = self.inner.servers.lock().await;
        if let Some(server) = servers.get(&self.id) {
            return Ok(server.clone());
        }

        let config = self.inner.config.read().await;
        let state = self.inner.state.read().await;

        let server_config = config.servers.get(&self.id).unwrap();

        let mut client = self.inner.client().await;

        match &server_config.connection {
            ServerConnection::MyPlex { username: _, id } => {
                let token = state
                    .servers
                    .get(&self.id)
                    .ok_or_else(|| Error::ServerNotAuthenticated)?
                    .token
                    .clone();

                let myplex = MyPlexBuilder::default()
                    .set_client(client)
                    .set_token(token)
                    .build()
                    .await?;

                let manager = myplex.device_manager()?;
                for device in manager.devices().await? {
                    if device.identifier() == id {
                        match device.connect().await? {
                            DeviceConnection::Server(server) => {
                                trace!(url=%server.client().api_url,
                                    "Connected to server"
                                );
                                servers.insert(self.id.clone(), server.as_ref().clone());
                                return Ok(*server);
                            }
                            _ => panic!("Unexpected client connection"),
                        }
                    }
                }

                Err(Error::MyPlexServerNotFound)
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
                servers.insert(self.id.clone(), server.clone());

                Ok(server)
            }
        }
    }

    /// Adds an item to sync based on its rating key.
    pub async fn add_sync(&self, rating_key: u32, transcode_profile: Option<String>) -> Result {
        let mut config = self.inner.config.write().await;

        if let Some(ref profile) = transcode_profile {
            if !config.profiles.contains_key(profile) {
                return Err(Error::UnknownProfile(profile.to_owned()));
            }
        }

        let server_config = config.servers.get_mut(&self.id).unwrap();
        server_config.syncs.insert(
            rating_key,
            SyncItem {
                id: rating_key,
                transcode_profile,
            },
        );

        self.inner.persist_config(&config).await
    }

    /// Updates the state for the synced items
    #[instrument(level = "trace", skip(self), fields(server = self.id))]
    pub async fn update_state(&self) -> Result {
        info!("Updating item metadata");
        let server = self.connect().await?;

        let config = self.inner.config.read().await;
        let server_config = config.servers.get(&self.id).unwrap();

        {
            let mut state = self.inner.state.write().await;

            let server_state = state.servers.entry(self.id.clone()).or_default();
            server_state.name = server.media_container.friendly_name.clone();

            {
                // Scope the write lock on the path.
                let root = self.inner.path.write().await;

                let mut state_sync = StateSync {
                    server_state,
                    server,
                    root: &root,
                    seen_items: Default::default(),
                    seen_libraries: Default::default(),
                };

                for item in server_config.syncs.values() {
                    if let Err(e) = state_sync.add_item_by_key(item, item.id).await {
                        warn!(error=?e, "Failed to update item");
                    }
                }

                state_sync.prune_unseen().await?;
            }

            self.inner.persist_state(&state).await?;
        }

        self.update_thumbnails().await?;
        self.verify_downloads().await
    }

    /// Updates thumbnails for synced items.
    #[instrument(level = "trace", skip(self), fields(server = self.id))]
    pub async fn update_thumbnails(&self) -> Result {
        info!("Updating thumbnails");

        for library in self.libraries().await {
            for collection in library.collections().await {
                if let Err(e) = collection.update_thumbnail().await {
                    warn!(error=?e);
                }
            }

            match library {
                Library::Movie(l) => {
                    for video in l.movies().await {
                        if let Err(e) = video.update_thumbnail().await {
                            warn!(error=?e);
                        }
                    }
                }
                Library::Show(l) => {
                    for show in l.shows().await {
                        if let Err(e) = show.update_thumbnail().await {
                            warn!(error=?e);
                        }

                        for season in show.seasons().await {
                            for video in season.episodes().await {
                                if let Err(e) = video.update_thumbnail().await {
                                    warn!(error=?e);
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Verifies the presence of downloads for synced items.
    #[instrument(level = "trace", skip(self), fields(server = self.id))]
    pub async fn verify_downloads(&self) -> Result {
        info!("Verifying downloads");

        for library in self.libraries().await {
            match library {
                Library::Movie(l) => {
                    for video in l.movies().await {
                        for part in video.parts().await {
                            if let Err(e) = part.verify_download().await {
                                warn!(error=?e);
                            }
                        }
                    }
                }
                Library::Show(l) => {
                    for show in l.shows().await {
                        for season in show.seasons().await {
                            for video in season.episodes().await {
                                for part in video.parts().await {
                                    if let Err(e) = part.verify_download().await {
                                        warn!(error=?e);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

struct StateSync<'a> {
    server_state: &'a mut ServerState,
    server: plex_api::Server,
    root: &'a Path,

    seen_items: HashSet<u32>,
    seen_libraries: HashSet<u32>,
}

macro_rules! return_if_seen {
    ($self:expr, $typ:expr) => {
        if $self.seen_items.contains(&$typ.rating_key()) {
            return Ok(());
        }
        $self.seen_items.insert($typ.rating_key());
    };
}

impl<'a> StateSync<'a> {
    async fn add_movie(&mut self, sync: &SyncItem, movie: &Movie) -> Result {
        return_if_seen!(self, movie);

        let video = self
            .server_state
            .videos
            .entry(movie.rating_key())
            .or_insert_with(|| VideoState::from(sync, movie));

        video.update(sync, movie, &self.server, self.root).await;

        self.add_library(movie)?;

        Ok(())
    }

    async fn add_episode(&mut self, sync: &SyncItem, episode: &Episode) -> Result {
        return_if_seen!(self, episode);

        let video = self
            .server_state
            .videos
            .entry(episode.rating_key())
            .or_insert_with(|| VideoState::from(sync, episode));

        video.update(sync, episode, &self.server, self.root).await;

        Ok(())
    }

    fn add_season(&mut self, season: &Season) -> Result {
        return_if_seen!(self, season);

        self.server_state
            .seasons
            .entry(season.rating_key())
            .and_modify(|ss| ss.update(season))
            .or_insert_with(|| SeasonState::from(season));

        Ok(())
    }

    async fn add_show(&mut self, show: &Show) -> Result {
        return_if_seen!(self, show);

        let show_state = self
            .server_state
            .shows
            .entry(show.rating_key())
            .or_insert_with(|| ShowState::from(show));

        show_state.update(show, self.root).await;

        self.add_library(show)?;

        Ok(())
    }

    fn add_library<T>(&mut self, item: &T) -> Result<&mut LibraryState>
    where
        T: MetadataItem,
    {
        let library_id = item
            .metadata()
            .library_section_id
            .ok_or(Error::ItemIncomplete(
                item.rating_key(),
                "library ID was missing".to_string(),
            ))?;
        let library_title =
            item.metadata()
                .library_section_title
                .clone()
                .ok_or(Error::ItemIncomplete(
                    item.rating_key(),
                    "library title was missing".to_string(),
                ))?;

        let library_type = match item.metadata().metadata_type.as_ref().unwrap() {
            MetadataType::Movie => LibraryType::Movie,
            MetadataType::Show => LibraryType::Show,
            _ => panic!("Unknown library type"),
        };

        let library = self
            .server_state
            .libraries
            .entry(library_id)
            .and_modify(|l| l.title = library_title.clone())
            .or_insert_with(|| LibraryState {
                id: library_id,
                title: library_title.clone(),
                library_type,
            });
        self.seen_libraries.insert(library_id);

        Ok(library)
    }

    async fn add_collection<T>(
        &mut self,
        collection: &Collection<T>,
        items: HashSet<u32>,
    ) -> Result {
        return_if_seen!(self, collection);

        let collection_state = self
            .server_state
            .collections
            .entry(collection.rating_key())
            .or_insert_with(|| CollectionState::from(collection));
        collection_state.items = items;

        collection_state.update(collection, self.root).await;

        Ok(())
    }

    fn add_playlist(&mut self, playlist: &Playlist<Video>, videos: Vec<u32>) -> Result {
        return_if_seen!(self, playlist);

        let playlist_state = self
            .server_state
            .playlists
            .entry(playlist.rating_key())
            .and_modify(|ps| ps.update(playlist))
            .or_insert_with(|| PlaylistState::from(playlist));
        playlist_state.videos = videos;

        Ok(())
    }

    async fn prune_unseen(&mut self) -> Result {
        info!("Pruning old items");

        for video in self
            .server_state
            .videos
            .values_mut()
            .filter(|v| !self.seen_items.contains(&v.id))
        {
            video.delete(&self.server, self.root).await;
        }

        for collection in self
            .server_state
            .collections
            .values_mut()
            .filter(|v| !self.seen_items.contains(&v.id))
        {
            collection.delete(self.root).await;
        }

        for show in self
            .server_state
            .shows
            .values_mut()
            .filter(|v| !self.seen_items.contains(&v.id))
        {
            show.delete(self.root).await;
        }

        self.server_state
            .videos
            .retain(|k, _v| self.seen_items.contains(k));

        self.server_state
            .playlists
            .retain(|k, _v| self.seen_items.contains(k));

        self.server_state
            .collections
            .retain(|k, _v| self.seen_items.contains(k));

        self.server_state
            .shows
            .retain(|k, _v| self.seen_items.contains(k));

        self.server_state
            .seasons
            .retain(|k, _v| self.seen_items.contains(k));

        self.server_state
            .libraries
            .retain(|k, _v| self.seen_libraries.contains(k));

        Ok(())
    }

    async fn add_item_by_key(&mut self, sync: &SyncItem, key: u32) -> Result {
        match self.server.item_by_id(key).await {
            Ok(i) => self.add_item(sync, i).await,
            Err(plex_api::Error::ItemNotFound) => Err(Error::ItemNotFound(key)),
            Err(e) => Err(e.into()),
        }
    }

    #[async_recursion]
    #[instrument(level = "trace", skip(self, sync, item), fields(item = item.rating_key()))]
    async fn add_item(&mut self, sync: &SyncItem, item: Item) -> Result {
        match item {
            Item::Movie(movie) => self.add_movie(sync, &movie).await,

            Item::Show(show) => {
                self.add_show(&show).await?;

                for season in show.seasons().await? {
                    self.add_season(&season)?;

                    for episode in season.episodes().await? {
                        self.add_episode(sync, &episode).await?;
                    }
                }

                Ok(())
            }
            Item::Season(season) => {
                if !self
                    .seen_items
                    .contains(&season.metadata().parent.parent_rating_key.unwrap())
                {
                    let show = season.show().await?.ok_or_else(|| {
                        Error::ItemIncomplete(season.rating_key(), "show was missing".to_string())
                    })?;
                    self.add_show(&show).await?;
                }

                self.add_season(&season)?;

                for episode in season.episodes().await? {
                    self.add_episode(sync, &episode).await?;
                }

                Ok(())
            }
            Item::Episode(episode) => {
                if !self
                    .seen_items
                    .contains(&episode.metadata().parent.parent_rating_key.unwrap())
                {
                    let season = episode.season().await?.ok_or_else(|| {
                        Error::ItemIncomplete(
                            episode.rating_key(),
                            "season was missing".to_string(),
                        )
                    })?;

                    if !self
                        .seen_items
                        .contains(&season.metadata().parent.parent_rating_key.unwrap())
                    {
                        let show = season.show().await?.ok_or_else(|| {
                            Error::ItemIncomplete(
                                season.rating_key(),
                                "show was missing".to_string(),
                            )
                        })?;
                        self.add_show(&show).await?;
                    }

                    self.add_season(&season)?;
                }

                self.add_episode(sync, &episode).await
            }

            Item::MovieCollection(collection) => {
                let mut items = HashSet::new();
                let movies = collection.children().await?;
                for movie in movies {
                    let key = movie.rating_key();
                    match self.add_item(sync, Item::Movie(movie)).await {
                        Ok(()) => {
                            items.insert(key);
                        }
                        Err(e) => warn!(error=?e, "Failed to update item"),
                    }
                }

                self.add_collection(&collection, items).await
            }
            Item::ShowCollection(collection) => {
                let mut items = HashSet::new();
                let shows = collection.children().await?;
                for show in shows {
                    let key = show.rating_key();
                    match self.add_item(sync, Item::Show(show)).await {
                        Ok(()) => {
                            items.insert(key);
                        }
                        Err(e) => warn!(error=?e, "Failed to update item"),
                    }
                }

                self.add_collection(&collection, items).await
            }
            Item::VideoPlaylist(playlist) => {
                let mut items = Vec::new();
                let videos = playlist.children().await?;
                for video in videos {
                    let key = video.rating_key();
                    let result = match video {
                        Video::Episode(episode) => {
                            self.add_item(sync, Item::Episode(episode)).await
                        }
                        Video::Movie(movie) => self.add_item(sync, Item::Movie(movie)).await,
                    };

                    match result {
                        Ok(()) => {
                            items.push(key);
                        }
                        Err(e) => warn!(error=?e, "Failed to update item"),
                    }
                }

                self.add_playlist(&playlist, items)
            }
            _ => Err(Error::ItemNotSupported(item.rating_key())),
        }
    }
}
