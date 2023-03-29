use std::{collections::HashSet, path::Path, sync::Arc};

use async_recursion::async_recursion;
use plex_api::{
    device::DeviceConnection, Collection, Episode, Item, MetadataItem, MetadataType, Movie,
    MyPlexBuilder, Playlist, Season, Show, Video,
};

use crate::{
    state::{
        CollectionState, LibraryState, LibraryType, PlaylistState, SeasonState, ServerState,
        ShowState, VideoState,
    },
    wrappers, Error, Inner, Result, ServerConnection,
};

#[derive(Clone)]
pub struct Server {
    pub(crate) id: String,
    pub(crate) inner: Arc<Inner>,
}

impl Server {
    /// The PlexOut identifier for this server.
    pub fn id(&self) -> &str {
        &self.id
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
    pub async fn connect(&self) -> Result<plex_api::Server> {
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
                                log::trace!(
                                    "Connected to server {} via {}",
                                    self.id,
                                    server.client().api_url
                                );
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
                log::trace!(
                    "Connected to server {} via {}",
                    self.id,
                    server.client().api_url
                );

                Ok(server)
            }
        }
    }

    /// Adds an item to sync based on its rating key.
    pub async fn add_sync(&self, rating_key: u32) -> Result {
        let mut config = self.inner.config.write().await;

        let server_config = config.servers.get_mut(&self.id).unwrap();
        server_config.syncs.insert(rating_key);

        self.inner.persist_config(&config).await
    }

    /// Updates the state for the synced items
    pub async fn update_state(&self) -> Result {
        let server = self.connect().await?;

        let config = self.inner.config.read().await;
        let server_config = config.servers.get(&self.id).unwrap();

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

            for key in &server_config.syncs {
                if let Err(e) = state_sync.add_item_by_key(*key).await {
                    log::warn!("Failed to update item: {e}");
                }
            }

            state_sync.prune_unseen().await?;
        }

        self.inner.persist_state(&state).await
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
    async fn add_movie(&mut self, movie: &Movie) -> Result {
        return_if_seen!(self, movie);

        let video = self
            .server_state
            .videos
            .entry(movie.rating_key())
            .and_modify(|video| video.update(movie))
            .or_insert_with(|| VideoState::from(movie));

        if let Some(updated) = movie.metadata().updated_at {
            video
                .delete_stale(&self.server, self.root, Some(updated))
                .await;
        }

        self.add_library(movie)?;

        Ok(())
    }

    async fn add_episode(&mut self, episode: &Episode) -> Result {
        return_if_seen!(self, episode);

        let video = self
            .server_state
            .videos
            .entry(episode.rating_key())
            .and_modify(|video| video.update(episode))
            .or_insert_with(|| VideoState::from(episode));

        if let Some(updated) = episode.metadata().updated_at {
            video
                .delete_stale(&self.server, self.root, Some(updated))
                .await;
        }

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
            .and_modify(|ss| ss.update(show))
            .or_insert_with(|| ShowState::from(show));

        if let Some(updated) = show.metadata().updated_at {
            show_state
                .thumbnail
                .delete_stale(self.root, Some(updated))
                .await;
        }

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

    fn add_collection<T>(&mut self, collection: &Collection<T>, items: HashSet<u32>) -> Result {
        return_if_seen!(self, collection);

        let collection_state = self
            .server_state
            .collections
            .entry(collection.rating_key())
            .and_modify(|cs| cs.update(collection))
            .or_insert_with(|| CollectionState::from(collection));
        collection_state.items = items;

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
        log::info!("Pruning old items");

        for video in self
            .server_state
            .videos
            .values_mut()
            .filter(|v| !self.seen_items.contains(&v.id))
        {
            video.delete_stale(&self.server, self.root, None).await;
        }

        for collection in self
            .server_state
            .collections
            .values_mut()
            .filter(|v| !self.seen_items.contains(&v.id))
        {
            collection.thumbnail.delete_stale(self.root, None).await;
        }

        for playlist in self
            .server_state
            .playlists
            .values_mut()
            .filter(|v| !self.seen_items.contains(&v.id))
        {
            playlist.thumbnail.delete_stale(self.root, None).await;
        }

        for show in self
            .server_state
            .shows
            .values_mut()
            .filter(|v| !self.seen_items.contains(&v.id))
        {
            show.thumbnail.delete_stale(self.root, None).await;
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

    async fn add_item_by_key(&mut self, key: u32) -> Result {
        match self.server.item_by_id(key).await {
            Ok(i) => self.add_item(i).await,
            Err(plex_api::Error::ItemNotFound) => Err(Error::ItemNotFound(key)),
            Err(e) => Err(e.into()),
        }
    }

    #[async_recursion]
    async fn add_item(&mut self, item: Item) -> Result {
        match item {
            Item::Movie(movie) => {
                log::debug!("Syncing movie '{}' metadata", movie.title());

                self.add_movie(&movie).await
            }

            Item::Show(show) => {
                log::debug!("Syncing show '{}' metadata", show.title());

                self.add_show(&show).await?;

                for season in show.seasons().await? {
                    self.add_season(&season)?;

                    for episode in season.episodes().await? {
                        self.add_episode(&episode).await?;
                    }
                }

                Ok(())
            }
            Item::Season(season) => {
                log::debug!("Syncing season '{}' metadata", season.title());

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
                    self.add_episode(&episode).await?;
                }

                Ok(())
            }
            Item::Episode(episode) => {
                log::debug!("Syncing episode '{}' metadata", episode.title());

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

                self.add_episode(&episode).await
            }

            Item::MovieCollection(collection) => {
                log::debug!("Syncing collection '{}' metadata", collection.title());

                let mut items = HashSet::new();
                let movies = collection.children().await?;
                for movie in movies {
                    let key = movie.rating_key();
                    match self.add_item(Item::Movie(movie)).await {
                        Ok(()) => {
                            items.insert(key);
                        }
                        Err(e) => log::warn!("Failed to update item: {e}"),
                    }
                }

                self.add_collection(&collection, items)
            }
            Item::ShowCollection(collection) => {
                log::debug!("Syncing collection '{}' metadata", collection.title());

                let mut items = HashSet::new();
                let shows = collection.children().await?;
                for show in shows {
                    let key = show.rating_key();
                    match self.add_item(Item::Show(show)).await {
                        Ok(()) => {
                            items.insert(key);
                        }
                        Err(e) => log::warn!("Failed to update item: {e}"),
                    }
                }

                self.add_collection(&collection, items)
            }
            Item::VideoPlaylist(playlist) => {
                log::debug!("Syncing playlist '{}' metadata", playlist.title());

                let mut items = Vec::new();
                let videos = playlist.children().await?;
                for video in videos {
                    let key = video.rating_key();
                    let result = match video {
                        Video::Episode(episode) => self.add_item(Item::Episode(episode)).await,
                        Video::Movie(movie) => self.add_item(Item::Movie(movie)).await,
                    };

                    match result {
                        Ok(()) => {
                            items.push(key);
                        }
                        Err(e) => log::warn!("Failed to update item: {e}"),
                    }
                }

                self.add_playlist(&playlist, items)
            }
            _ => Err(Error::ItemNotSupported(item.rating_key())),
        }
    }
}
