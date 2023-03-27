use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use async_recursion::async_recursion;
use plex_api::{
    device::DeviceConnection, Collection, Episode, Item, MetadataItem, Movie, MyPlexBuilder,
    Playlist, Season, Show, Video,
};

use crate::{
    state::{
        CollectionState, LibraryContent, LibraryState, PlaylistState, ServerState, VideoState,
    },
    Error, Inner, Result, ServerConnection,
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

        let mut state_sync = StateSync {
            server_state,
            server,
            seen_items: Default::default(),
            seen_libraries: Default::default(),
        };

        for key in &server_config.syncs {
            if let Err(e) = state_sync.add_item_by_key(*key).await {
                log::warn!("Failed to update item: {e}");
            }
        }

        self.inner.persist_state(&state).await
    }
}

struct StateSync<'a> {
    server_state: &'a mut ServerState,
    server: plex_api::Server,

    seen_items: HashSet<u32>,
    seen_libraries: HashSet<u32>,
}

impl<'a> StateSync<'a> {
    fn add_seen<M: MetadataItem>(&mut self, item: &M) {
        self.seen_items.insert(item.rating_key());
    }

    fn add_movie(&mut self, movie: &Movie) -> Result {
        let key = movie.rating_key();
        if self.seen_items.contains(&key) {
            return Ok(());
        }
        self.add_seen(movie);

        self.server_state
            .videos
            .entry(key.to_string())
            .and_modify(|video| video.update_from_movie(movie))
            .or_insert_with(|| VideoState::from_movie(movie));

        let library = self.add_library(movie, || LibraryContent::Movies(HashSet::new()))?;
        library.add_movie(movie);

        Ok(())
    }

    fn add_episode(&mut self, show: &Show, season: &Season, episode: &Episode) -> Result {
        let key = episode.rating_key();
        if self.seen_items.contains(&key) {
            return Ok(());
        }
        self.add_seen(episode);
        self.add_seen(season);
        self.add_seen(show);

        self.server_state
            .videos
            .entry(key.to_string())
            .and_modify(|video| video.update_from_episode(episode))
            .or_insert_with(|| VideoState::from_episode(episode));

        let library = self.add_library(show, || LibraryContent::Shows(HashMap::new()))?;
        library.add_episode(show, season, episode);

        Ok(())
    }

    fn add_library<F, T>(&mut self, item: &T, cb: F) -> Result<&mut LibraryState>
    where
        T: MetadataItem,
        F: FnOnce() -> LibraryContent,
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

        let library = self
            .server_state
            .libraries
            .entry(library_id.to_string())
            .and_modify(|l| l.title = library_title.clone())
            .or_insert_with(|| LibraryState {
                id: library_id,
                title: library_title.clone(),
                collections: HashMap::new(),
                content: cb(),
                path: library_title,
            });
        self.seen_libraries.insert(library_id);

        Ok(library)
    }

    fn add_collection<T>(&mut self, collection: &Collection<T>, items: HashSet<u32>) -> Result {
        self.add_seen(collection);

        let library_id = collection
            .metadata()
            .library_section_id
            .ok_or(Error::ItemIncomplete(
                collection.rating_key(),
                "library ID was missing".to_string(),
            ))?;

        let library = self
            .server_state
            .libraries
            .get_mut(&library_id.to_string())
            .unwrap();
        let collection_state = library
            .collections
            .entry(collection.rating_key().to_string())
            .and_modify(|cs| cs.update_from_collection(collection))
            .or_insert_with(|| CollectionState::from_collection(collection));
        collection_state.items = items;

        Ok(())
    }

    fn add_playlist(&mut self, playlist: &Playlist<Video>, videos: Vec<u32>) -> Result {
        self.add_seen(playlist);

        let playlist_state = self
            .server_state
            .playlists
            .entry(playlist.rating_key().to_string())
            .and_modify(|ps| ps.update_from_playlist(playlist))
            .or_insert_with(|| PlaylistState::from_playlist(playlist));
        playlist_state.videos = videos;

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
            Item::Movie(movie) => self.add_movie(&movie),

            Item::Show(show) => {
                for season in show.seasons().await? {
                    for episode in season.episodes().await? {
                        self.add_episode(&show, &season, &episode)?;
                    }
                }

                Ok(())
            }
            Item::Season(season) => {
                let show = season.show().await?.ok_or_else(|| {
                    Error::ItemIncomplete(season.rating_key(), "show was missing".to_string())
                })?;

                for episode in season.episodes().await? {
                    self.add_episode(&show, &season, &episode)?;
                }

                Ok(())
            }
            Item::Episode(episode) => {
                let season = episode.season().await?.ok_or_else(|| {
                    Error::ItemIncomplete(episode.rating_key(), "season was missing".to_string())
                })?;

                let show = season.show().await?.ok_or_else(|| {
                    Error::ItemIncomplete(season.rating_key(), "show was missing".to_string())
                })?;

                self.add_episode(&show, &season, &episode)
            }

            Item::MovieCollection(collection) => {
                let mut items = HashSet::new();
                let movies = collection.children().await?;
                for movie in movies {
                    items.insert(movie.rating_key());
                    self.add_item(Item::Movie(movie)).await?;
                }

                self.add_collection(&collection, items)
            }
            Item::ShowCollection(collection) => {
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
