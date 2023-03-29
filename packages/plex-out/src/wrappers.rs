use std::{path::PathBuf, sync::Arc};

use async_std::fs::{create_dir_all, File};
use async_trait::async_trait;
use plex_api::MetadataItem;

use crate::{
    state::{
        CollectionState, LibraryState, PlaylistState, SeasonState, ServerState, ShowState,
        ThumbnailState, VideoDetail, VideoState,
    },
    Inner, Result, Server,
};

fn safe<S: AsRef<str>>(str: S) -> String {
    str.as_ref()
        .chars()
        .map(|x| match x {
            '#' | '%' | '{' | '}' | '\\' | '/' | '<' | '>' | '*' | '?' | '$' | '!' | '"' | '\''
            | ':' | '@' | '+' | '`' | '|' | '=' => '_',
            _ => x,
        })
        .collect()
}

#[async_trait]
trait StateWrapper<S> {
    async fn connect(&self) -> Result<plex_api::Server>;

    async fn with_server_state<F, R>(&self, cb: F) -> R
    where
        F: Send + FnOnce(&ServerState) -> R;

    async fn with_state<F, R>(&self, cb: F) -> R
    where
        F: Send + FnOnce(&S) -> R;

    async fn update_state<F>(&self, cb: F) -> Result
    where
        F: Send + FnOnce(&mut S);
}

macro_rules! state_wrapper {
    ($typ:ident, $st_typ:ident, $prop:ident) => {
        #[async_trait]
        impl StateWrapper<$st_typ> for $typ {
            async fn connect(&self) -> Result<plex_api::Server> {
                let server = Server {
                    id: self.server.clone(),
                    inner: self.inner.clone(),
                };

                server.connect().await
            }

            async fn with_server_state<F, R>(&self, cb: F) -> R
            where
                F: Send + FnOnce(&ServerState) -> R,
            {
                let state = self.inner.state.read().await;
                cb(&state.servers.get(&self.server).unwrap())
            }

            async fn with_state<F, R>(&self, cb: F) -> R
            where
                F: Send + FnOnce(&$st_typ) -> R,
            {
                self.with_server_state(|ss| cb(ss.$prop.get(&self.id).unwrap()))
                    .await
            }

            async fn update_state<F>(&self, cb: F) -> Result
            where
                F: Send + FnOnce(&mut $st_typ),
            {
                let mut state = self.inner.state.write().await;
                let server_state = state.servers.get_mut(&self.server).unwrap();
                cb(server_state.$prop.get_mut(&self.id).unwrap());
                self.inner.persist_state(&state).await
            }
        }
    };
}

macro_rules! thumbnail_methods {
    () => {
        pub async fn thumbnail(&self) -> ThumbnailState {
            self.with_state(|s| s.thumbnail.clone()).await
        }

        pub async fn update_thumbnail(&self) -> Result {
            if self.thumbnail().await.is_none() {
                let server = self.connect().await?;
                let item = server.item_by_id(self.id).await?;
                log::debug!("Updating thumbnail for {}", item.title());
                if let Some(ref thumb) = item.metadata().thumb {
                    let root = self.inner.path.read().await;
                    let path = self.file_path("jpg").await;
                    let target = root.join(&path);

                    if let Some(parent) = target.parent() {
                        create_dir_all(parent).await?;
                    }

                    let file = File::create(root.join(&path)).await?;
                    server
                        .transcode_artwork(thumb, 320, 320, Default::default(), file)
                        .await?;

                    let state = ThumbnailState::Downloaded {
                        last_updated: item.metadata().updated_at.unwrap(),
                        path,
                    };

                    self.update_state(|s| s.thumbnail = state).await?;
                    log::trace!("Thumbnail for {} successfully updated", item.title());
                }
            }

            Ok(())
        }
    };
}

macro_rules! parent {
    ($meth:ident, $typ:ident, $($pprop:tt)*) => {
        pub async fn $meth(&self) -> $typ {
            self.with_state(|ss| $typ {
                server: self.server.clone(),
                id: ss.$($pprop)*,
                inner: self.inner.clone(),
            })
            .await
        }
    };
}

macro_rules! children {
    ($meth:ident, $prop:ident, $typ:ident, $($pprop:tt)*) => {
        pub async fn $meth(&self) -> Vec<$typ> {
            self.with_server_state(|ss| {
                ss.$prop
                    .iter()
                    .filter_map(|(id, s)| {
                        if s.$($pprop)* == self.id {
                            Some($typ {
                                server: self.server.clone(),
                                id: *id,
                                inner: self.inner.clone(),
                            })
                        } else {
                            None
                        }
                    })
                    .collect()
            })
            .await
        }
    };
}

#[derive(Clone)]
pub struct Show {
    pub(crate) server: String,
    pub(crate) id: u32,
    pub(crate) inner: Arc<Inner>,
}

state_wrapper!(Show, ShowState, shows);

impl Show {
    thumbnail_methods!();
    parent!(library, ShowLibrary, library);
    children!(seasons, seasons, Season, show);

    async fn file_path(&self, extension: &str) -> PathBuf {
        self.with_server_state(|ss| {
            let state = ss.shows.get(&self.id).unwrap();
            let library_title = &ss.libraries.get(&state.library).unwrap().title;
            PathBuf::from(safe(library_title))
                .join(safe(format!("{} ({})", state.title, state.year)))
                .join(safe(format!(
                    "{} ({}).{extension}",
                    state.title, state.year
                )))
        })
        .await
    }
}

#[derive(Clone)]
pub struct Season {
    pub(crate) server: String,
    pub(crate) id: u32,
    pub(crate) inner: Arc<Inner>,
}

state_wrapper!(Season, SeasonState, seasons);

impl Season {
    parent!(show, Show, show);

    pub async fn episodes(&self) -> Vec<Episode> {
        self.with_server_state(|ss| {
            ss.videos
                .iter()
                .filter_map(|(id, s)| {
                    if let VideoDetail::Episode(ref detail) = s.detail {
                        if detail.season == self.id {
                            Some(Episode {
                                server: self.server.clone(),
                                id: *id,
                                inner: self.inner.clone(),
                            })
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
                .collect()
        })
        .await
    }
}

#[derive(Clone)]
pub struct Episode {
    pub(crate) server: String,
    pub(crate) id: u32,
    pub(crate) inner: Arc<Inner>,
}

state_wrapper!(Episode, VideoState, videos);

impl Episode {
    thumbnail_methods!();
    parent!(season, Season, episode_state().season);

    pub async fn show(&self) -> Show {
        self.season().await.show().await
    }

    pub async fn library(&self) -> ShowLibrary {
        self.show().await.library().await
    }

    async fn file_path(&self, extension: &str) -> PathBuf {
        self.with_server_state(|ss| {
            let state = ss.videos.get(&self.id).unwrap();
            let ep_state = state.episode_state();
            let season = ss.seasons.get(&ep_state.season).unwrap();
            let show = ss.shows.get(&season.show).unwrap();
            let library_title = &ss.libraries.get(&show.library).unwrap().title;

            PathBuf::from(safe(library_title))
                .join(safe(format!("{} ({})", show.title, show.year)))
                .join(safe(format!(
                    "S{:02}E{:02} {}.{extension}",
                    season.index, ep_state.index, state.title
                )))
        })
        .await
    }
}

#[derive(Clone)]
pub struct Movie {
    pub(crate) server: String,
    pub(crate) id: u32,
    pub(crate) inner: Arc<Inner>,
}

state_wrapper!(Movie, VideoState, videos);

impl Movie {
    thumbnail_methods!();
    parent!(library, MovieLibrary, movie_state().library);

    async fn file_path(&self, extension: &str) -> PathBuf {
        self.with_server_state(|ss| {
            let state = ss.videos.get(&self.id).unwrap();
            let m_state = state.movie_state();
            let library_title = &ss.libraries.get(&m_state.library).unwrap().title;

            PathBuf::from(safe(library_title))
                .join(safe(format!("{} ({})", state.title, m_state.year)))
                .join(safe(format!(
                    "{} ({}).{extension}",
                    state.title, m_state.year
                )))
        })
        .await
    }
}

#[derive(Clone)]
pub enum Video {
    Movie(Movie),
    Episode(Episode),
}

impl Video {
    pub async fn library(&self) -> Library {
        match self {
            Self::Movie(v) => Library::Movie(v.library().await),
            Self::Episode(v) => Library::Show(v.library().await),
        }
    }

    pub async fn thumbnail(&self) -> ThumbnailState {
        match self {
            Self::Movie(v) => v.thumbnail().await,
            Self::Episode(v) => v.thumbnail().await,
        }
    }

    pub async fn update_thumbnail(&self) -> Result {
        match self {
            Self::Movie(v) => v.update_thumbnail().await,
            Self::Episode(v) => v.update_thumbnail().await,
        }
    }
}

#[derive(Clone)]
pub struct Playlist {
    pub(crate) server: String,
    pub(crate) id: u32,
    pub(crate) inner: Arc<Inner>,
}

state_wrapper!(Playlist, PlaylistState, playlists);

impl Playlist {
    thumbnail_methods!();

    pub async fn videos(&self) -> Vec<Video> {
        self.with_server_state(|ss| {
            let ps = ss.playlists.get(&self.id).unwrap();
            ps.videos
                .iter()
                .map(|id| match ss.videos.get(id).unwrap().detail {
                    VideoDetail::Movie(_) => Video::Movie(Movie {
                        server: self.server.clone(),
                        id: *id,
                        inner: self.inner.clone(),
                    }),
                    VideoDetail::Episode(_) => Video::Episode(Episode {
                        server: self.server.clone(),
                        id: *id,
                        inner: self.inner.clone(),
                    }),
                })
                .collect()
        })
        .await
    }

    async fn file_path(&self, extension: &str) -> PathBuf {
        self.with_server_state(|ss| {
            let state = ss.playlists.get(&self.id).unwrap();

            PathBuf::from(safe(format!("{}.{extension}", state.title)))
        })
        .await
    }
}

#[derive(Clone)]
pub struct MovieCollection {
    pub(crate) server: String,
    pub(crate) id: u32,
    pub(crate) inner: Arc<Inner>,
}

state_wrapper!(MovieCollection, CollectionState, collections);

impl MovieCollection {
    thumbnail_methods!();
    parent!(library, MovieLibrary, library);

    pub async fn movies(&self) -> Vec<Movie> {
        self.with_state(|cs| {
            cs.items
                .iter()
                .map(|id| Movie {
                    server: self.server.clone(),
                    id: *id,
                    inner: self.inner.clone(),
                })
                .collect()
        })
        .await
    }

    async fn file_path(&self, extension: &str) -> PathBuf {
        self.with_server_state(|ss| {
            let state = ss.collections.get(&self.id).unwrap();
            let library_title = &ss.libraries.get(&state.library).unwrap().title;

            PathBuf::from(safe(library_title)).join(safe(format!("{}.{extension}", state.title)))
        })
        .await
    }
}

#[derive(Clone)]
pub struct ShowCollection {
    pub(crate) server: String,
    pub(crate) id: u32,
    pub(crate) inner: Arc<Inner>,
}

state_wrapper!(ShowCollection, CollectionState, collections);

impl ShowCollection {
    thumbnail_methods!();
    parent!(library, ShowLibrary, library);

    pub async fn shows(&self) -> Vec<Show> {
        self.with_state(|cs| {
            cs.items
                .iter()
                .map(|id| Show {
                    server: self.server.clone(),
                    id: *id,
                    inner: self.inner.clone(),
                })
                .collect()
        })
        .await
    }

    async fn file_path(&self, extension: &str) -> PathBuf {
        self.with_server_state(|ss| {
            let state = ss.collections.get(&self.id).unwrap();
            let library_title = &ss.libraries.get(&state.library).unwrap().title;

            PathBuf::from(safe(library_title)).join(safe(format!("{}.{extension}", state.title)))
        })
        .await
    }
}

#[derive(Clone)]
pub enum Collection {
    Movie(MovieCollection),
    Show(ShowCollection),
}

impl Collection {
    pub async fn thumbnail(&self) -> ThumbnailState {
        match self {
            Self::Movie(c) => c.thumbnail().await,
            Self::Show(c) => c.thumbnail().await,
        }
    }

    pub async fn update_thumbnail(&self) -> Result {
        match self {
            Self::Movie(c) => c.update_thumbnail().await,
            Self::Show(c) => c.update_thumbnail().await,
        }
    }
}

#[derive(Clone)]
pub struct MovieLibrary {
    pub(crate) server: String,
    pub(crate) id: u32,
    pub(crate) inner: Arc<Inner>,
}

state_wrapper!(MovieLibrary, LibraryState, libraries);

impl MovieLibrary {
    children!(collections, collections, MovieCollection, library);

    pub async fn movies(&self) -> Vec<Movie> {
        self.with_server_state(|ss| {
            ss.videos
                .iter()
                .filter_map(|(id, s)| {
                    if let VideoDetail::Movie(ref detail) = s.detail {
                        if detail.library == self.id {
                            Some(Movie {
                                server: self.server.clone(),
                                id: *id,
                                inner: self.inner.clone(),
                            })
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
                .collect()
        })
        .await
    }
}

#[derive(Clone)]
pub struct ShowLibrary {
    pub(crate) server: String,
    pub(crate) id: u32,
    pub(crate) inner: Arc<Inner>,
}

state_wrapper!(ShowLibrary, LibraryState, libraries);

impl ShowLibrary {
    children!(collections, collections, ShowCollection, library);
    children!(shows, shows, Show, library);
}

#[derive(Clone)]
pub enum Library {
    Movie(MovieLibrary),
    Show(ShowLibrary),
}

impl Library {
    pub async fn collections(&self) -> Vec<Collection> {
        match self {
            Self::Movie(l) => l
                .collections()
                .await
                .into_iter()
                .map(Collection::Movie)
                .collect(),
            Self::Show(l) => l
                .collections()
                .await
                .into_iter()
                .map(Collection::Show)
                .collect(),
        }
    }
}
