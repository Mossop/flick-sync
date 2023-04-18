use std::collections::{HashMap, HashSet};
use std::fmt;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use plex_api::library::MediaItem;
use plex_api::{
    library::{Collection, MetadataItem, Playlist, Season, Show},
    media_container::server::library::{Metadata, MetadataType},
    Server,
};
use serde::{Deserialize, Serialize};
use time::{Date, OffsetDateTime};
use tokio::fs;
use tracing::{error, instrument, trace, warn};
use typeshare::typeshare;
use uuid::Uuid;

use crate::config::SyncItem;
use crate::util::{derive_list_item, from_list, into_list, ListItem};

#[derive(Deserialize, Default, Serialize, Clone, PartialEq)]
#[serde(tag = "state", rename_all = "camelCase")]
pub(crate) enum ThumbnailState {
    #[default]
    None,
    #[serde(rename_all = "camelCase")]
    Downloaded { path: PathBuf },
}

impl ThumbnailState {
    pub(crate) fn is_none(&self) -> bool {
        matches!(self, ThumbnailState::None)
    }

    #[instrument(level = "trace", skip(root))]
    pub(crate) async fn verify(&mut self, root: &Path) {
        if let ThumbnailState::Downloaded { path } = self {
            let file = root.join(&path);

            match fs::metadata(&file).await {
                Ok(stats) => {
                    if !stats.is_file() {
                        error!(?path, "Expected a file");
                    }
                }
                Err(e) => {
                    if e.kind() == ErrorKind::NotFound {
                        warn!(?path, "Thumbnail no longer present");
                        *self = ThumbnailState::None;
                    } else {
                        error!(?path, error=?e, "Error accessing thumbnail");
                    }
                }
            }
        }
    }

    #[instrument(level = "trace", skip(root))]
    pub(crate) async fn delete(&mut self, root: &Path) {
        if let ThumbnailState::Downloaded { path } = self {
            let file = root.join(&path);
            trace!(?path, "Removing old thumbnail file");

            if let Err(e) = fs::remove_file(&file).await {
                if e.kind() != ErrorKind::NotFound {
                    warn!(?path, error=?e, "Failed to remove file");
                }
            }

            *self = ThumbnailState::None;
        }
    }
}

impl fmt::Debug for ThumbnailState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => write!(f, "None"),
            Self::Downloaded { path: _ } => write!(f, "Downloaded"),
        }
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[typeshare]
#[serde(rename_all = "camelCase")]
pub(crate) struct CollectionState {
    pub(crate) id: u32,
    pub(crate) library: u32,
    pub(crate) title: String,
    #[typeshare(serialized_as = "Vec<u32>")]
    #[serde(default, skip_serializing_if = "HashSet::is_empty")]
    pub(crate) items: HashSet<u32>,
    #[serde(with = "time::serde::timestamp")]
    #[typeshare(serialized_as = "number")]
    pub(crate) last_updated: OffsetDateTime,
    #[serde(default, skip_serializing_if = "ThumbnailState::is_none")]
    pub(crate) thumbnail: ThumbnailState,
}

derive_list_item!(CollectionState);

impl CollectionState {
    pub(crate) fn from<T>(collection: &Collection<T>) -> Self {
        Self {
            id: collection.rating_key(),
            library: collection.metadata().library_section_id.unwrap(),
            title: collection.title().to_owned(),
            items: Default::default(),
            last_updated: collection.metadata().updated_at.unwrap(),
            thumbnail: Default::default(),
        }
    }

    pub(crate) async fn update<T>(&mut self, collection: &Collection<T>, root: &Path) {
        self.title = collection.title().to_owned();

        if let Some(updated) = collection.metadata().updated_at {
            if updated > self.last_updated {
                self.thumbnail.delete(root).await;
            }
            self.last_updated = updated;
        }
    }

    pub(crate) async fn delete(&mut self, root: &Path) {
        self.thumbnail.verify(root).await;

        if self.thumbnail != ThumbnailState::None {
            self.thumbnail.delete(root).await;
        }
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[typeshare]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlaylistState {
    pub(crate) id: u32,
    pub(crate) title: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) videos: Vec<u32>,
}

derive_list_item!(PlaylistState);

impl PlaylistState {
    pub(crate) fn from<T>(playlist: &Playlist<T>) -> Self {
        Self {
            id: playlist.rating_key(),
            title: playlist.title().to_owned(),
            videos: Default::default(),
        }
    }

    pub(crate) fn update<T>(&mut self, playlist: &Playlist<T>) {
        self.title = playlist.title().to_owned();
    }
}

#[derive(Deserialize, Serialize, Clone, Copy, Debug)]
#[serde(rename_all = "lowercase")]
pub(crate) enum LibraryType {
    Movie,
    Show,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[typeshare]
#[serde(rename_all = "camelCase")]
pub(crate) struct LibraryState {
    pub(crate) id: u32,
    pub(crate) title: String,
    #[serde(rename = "type")]
    pub(crate) library_type: LibraryType,
}

derive_list_item!(LibraryState);

#[derive(Deserialize, Serialize, Clone, Debug)]
#[typeshare]
#[serde(rename_all = "camelCase")]
pub(crate) struct SeasonState {
    pub(crate) id: u32,
    pub(crate) show: u32,
    pub(crate) index: u32,
    pub(crate) title: String,
}

derive_list_item!(SeasonState);

impl SeasonState {
    pub(crate) fn from(season: &Season) -> Self {
        let metadata = season.metadata();

        Self {
            id: season.rating_key(),
            show: metadata.parent.parent_rating_key.unwrap(),
            index: metadata.index.unwrap(),
            title: season.title().to_owned(),
        }
    }

    pub(crate) fn update(&mut self, season: &Season) {
        let metadata = season.metadata();

        self.index = metadata.index.unwrap();
        self.show = metadata.parent.parent_rating_key.unwrap();
        self.title = season.title().to_owned();
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[typeshare]
#[serde(rename_all = "camelCase")]
pub(crate) struct ShowState {
    pub(crate) id: u32,
    pub(crate) library: u32,
    pub(crate) title: String,
    pub(crate) year: u32,
    #[serde(with = "time::serde::timestamp")]
    #[typeshare(serialized_as = "number")]
    pub(crate) last_updated: OffsetDateTime,
    #[serde(default, skip_serializing_if = "ThumbnailState::is_none")]
    pub(crate) thumbnail: ThumbnailState,
}

derive_list_item!(ShowState);

impl ShowState {
    pub(crate) fn from(show: &Show) -> Self {
        let metadata = show.metadata();

        let year = metadata.year.unwrap();
        let title = show.title().to_owned();

        Self {
            id: show.rating_key(),
            library: metadata.library_section_id.unwrap(),
            title,
            year,
            last_updated: metadata.updated_at.unwrap(),
            thumbnail: Default::default(),
        }
    }

    pub(crate) async fn update(&mut self, show: &Show, root: &Path) {
        let metadata = show.metadata();

        self.year = metadata.year.unwrap();
        self.title = show.title().to_owned();

        if let Some(updated) = show.metadata().updated_at {
            if updated > self.last_updated {
                self.thumbnail.delete(root).await;
            }
            self.last_updated = updated;
        }
    }

    pub(crate) async fn delete(&mut self, root: &Path) {
        self.thumbnail.verify(root).await;

        if self.thumbnail != ThumbnailState::None {
            self.thumbnail.delete(root).await;
        }
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[typeshare]
#[serde(rename_all = "camelCase")]
pub(crate) struct MovieState {
    pub(crate) library: u32,
    pub(crate) year: u32,
}

impl MovieState {
    pub(crate) fn from(metadata: &Metadata) -> Self {
        MovieState {
            library: metadata.library_section_id.unwrap(),
            year: metadata.year.unwrap(),
        }
    }

    pub(crate) fn update(&mut self, metadata: &Metadata) {
        self.year = metadata.year.unwrap();
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[typeshare]
#[serde(rename_all = "camelCase")]
pub(crate) struct EpisodeState {
    pub(crate) season: u32,
    pub(crate) index: u32,
}

impl EpisodeState {
    pub(crate) fn from(metadata: &Metadata) -> Self {
        EpisodeState {
            season: metadata.parent.parent_rating_key.unwrap(),
            index: metadata.index.unwrap(),
        }
    }

    pub(crate) fn update(&mut self, metadata: &Metadata) {
        self.season = metadata.parent.parent_rating_key.unwrap();
        self.index = metadata.index.unwrap();
    }
}

#[derive(Deserialize, Default, Serialize, Clone, PartialEq)]
#[serde(tag = "state", rename_all = "camelCase")]
pub(crate) enum DownloadState {
    #[default]
    None,
    #[serde(rename_all = "camelCase")]
    Downloading { path: PathBuf },
    #[serde(rename_all = "camelCase")]
    Transcoding { session_id: String, path: PathBuf },
    #[serde(rename_all = "camelCase")]
    Downloaded { path: PathBuf },
    #[serde(rename_all = "camelCase")]
    Transcoded { path: PathBuf },
}

impl DownloadState {
    fn is_none(&self) -> bool {
        matches!(self, DownloadState::None)
    }

    pub(crate) fn needs_download(&self) -> bool {
        !matches!(
            self,
            DownloadState::Downloaded { path: _ } | DownloadState::Transcoded { path: _ }
        )
    }

    #[instrument(level = "trace", skip(root, server))]
    pub(crate) async fn verify(&mut self, server: &Server, root: &Path) {
        let path = match self {
            DownloadState::None => return,
            DownloadState::Downloading { path: _ } => {
                return;
            }
            DownloadState::Transcoding { session_id, path } => {
                let file = root.join(&path);

                if let Err(plex_api::Error::ItemNotFound) =
                    server.transcode_session(session_id).await
                {
                    warn!(?path, "Transcode session is no longer present");
                    if let Err(e) = fs::remove_file(&file).await {
                        if e.kind() != ErrorKind::NotFound {
                            warn!(?path, error=?e, "Failed to remove partial download");
                        }
                    }

                    *self = DownloadState::None;
                }

                return;
            }
            DownloadState::Downloaded { path } => path,
            DownloadState::Transcoded { path } => path,
        };

        let file = root.join(&path);

        match fs::metadata(&file).await {
            Ok(stats) => {
                if stats.is_file() {
                    return;
                }
            }
            Err(e) => {
                if e.kind() != ErrorKind::NotFound {
                    error!(?path, error=?e, "Error accessing file");
                    return;
                }
            }
        }

        error!(?path, "Download is no longer present");
        *self = DownloadState::None;
    }

    #[instrument(level = "trace", skip(root, server))]
    pub(crate) async fn delete(&mut self, server: &Server, root: &Path) {
        let (path, session_id) = match self {
            DownloadState::None => return,
            DownloadState::Downloading { path } => (path, None),
            DownloadState::Transcoding { session_id, path } => (path, Some(session_id)),
            DownloadState::Downloaded { path } => (path, None),
            DownloadState::Transcoded { path } => (path, None),
        };

        let file = root.join(&path);

        trace!(?path, "Removing old video file");

        if let Err(e) = fs::remove_file(&file).await {
            if e.kind() != ErrorKind::NotFound {
                warn!(?path, error=?e, "Failed to remove file");
            }
        }

        if let Some(session_id) = session_id {
            if let Ok(session) = server.transcode_session(session_id).await {
                if let Err(e) = session.cancel().await {
                    warn!(error=?e, "Failed to cancel stale transcode session");
                }
            }
        }

        *self = DownloadState::None;
    }
}

impl fmt::Debug for DownloadState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => write!(f, "None"),
            Self::Downloading { path: _ } => write!(f, "Downloading"),
            Self::Transcoding {
                session_id,
                path: _,
            } => write!(f, "Transcoding({session_id})"),
            Self::Downloaded { path: _ } => write!(f, "Downloaded"),
            Self::Transcoded { path: _ } => write!(f, "Transcoded"),
        }
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[typeshare]
#[serde(rename_all = "camelCase")]
pub(crate) struct VideoPartState {
    #[typeshare(serialized_as = "number")]
    pub(crate) duration: u64,
    #[serde(default, skip_serializing_if = "DownloadState::is_none")]
    pub(crate) download: DownloadState,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(untagged)]
pub(crate) enum VideoDetail {
    Movie(MovieState),
    Episode(EpisodeState),
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[typeshare]
#[serde(rename_all = "camelCase")]
pub(crate) struct VideoState {
    pub(crate) id: u32,
    pub(crate) title: String,
    pub(crate) detail: VideoDetail,
    #[typeshare(serialized_as = "string")]
    pub(crate) air_date: Date,
    #[serde(default, skip_serializing_if = "ThumbnailState::is_none")]
    pub(crate) thumbnail: ThumbnailState,
    #[typeshare(serialized_as = "number")]
    pub(crate) media_id: u64,
    #[serde(with = "time::serde::timestamp")]
    #[typeshare(serialized_as = "number")]
    pub(crate) last_updated: OffsetDateTime,
    pub(crate) parts: Vec<VideoPartState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) transcode_profile: Option<String>,
}

derive_list_item!(VideoState);

impl VideoState {
    pub(crate) fn movie_state(&self) -> &MovieState {
        match self.detail {
            VideoDetail::Movie(ref m) => m,
            VideoDetail::Episode(_) => panic!("Unexpected type"),
        }
    }

    pub(crate) fn episode_state(&self) -> &EpisodeState {
        match self.detail {
            VideoDetail::Movie(_) => panic!("Unexpected type"),
            VideoDetail::Episode(ref e) => e,
        }
    }

    pub(crate) fn from<M: MediaItem>(sync: &SyncItem, item: &M) -> Self {
        let metadata = item.metadata();
        let detail = match metadata.metadata_type {
            Some(MetadataType::Movie) => VideoDetail::Movie(MovieState::from(metadata)),
            Some(MetadataType::Episode) => VideoDetail::Episode(EpisodeState::from(metadata)),
            _ => panic!("Unexpected video type: {:?}", metadata.metadata_type),
        };

        let media = &item.media()[0];
        let parts: Vec<VideoPartState> = media
            .parts()
            .iter()
            .map(|p| VideoPartState {
                duration: p.metadata().duration.unwrap(),
                download: Default::default(),
            })
            .collect();

        Self {
            id: item.rating_key(),
            title: item.title().to_owned(),
            detail,
            air_date: metadata.originally_available_at.unwrap(),
            thumbnail: Default::default(),
            media_id: media.metadata().id,
            last_updated: metadata.updated_at.unwrap(),
            parts,
            transcode_profile: sync.transcode_profile.clone(),
        }
    }

    pub(crate) async fn update<M: MetadataItem>(
        &mut self,
        sync: &SyncItem,
        item: &M,
        server: &Server,
        root: &Path,
    ) {
        let metadata = item.metadata();
        self.title = item.title().to_owned();
        self.transcode_profile = sync.transcode_profile.clone();

        match self.detail {
            VideoDetail::Movie(ref mut m) => m.update(metadata),
            VideoDetail::Episode(ref mut e) => e.update(metadata),
        }

        if let Some(updated) = metadata.updated_at {
            if updated > self.last_updated {
                self.thumbnail.delete(root).await;
                for part in self.parts.iter_mut() {
                    part.download.delete(server, root).await;
                }
            }
            self.last_updated = updated;
        }
    }

    pub(crate) async fn delete(&mut self, server: &Server, root: &Path) {
        if self.thumbnail != ThumbnailState::None {
            self.thumbnail.delete(root).await;
        }

        for part in self.parts.iter_mut() {
            if part.download != DownloadState::None {
                part.download.delete(server, root).await;
            }
        }
    }
}

#[derive(Deserialize, Default, Serialize, Clone, Debug)]
#[typeshare]
#[serde(rename_all = "camelCase")]
pub(crate) struct ServerState {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(crate) token: String,
    pub(crate) name: String,
    #[serde(
        default,
        skip_serializing_if = "HashMap::is_empty",
        serialize_with = "into_list",
        deserialize_with = "from_list"
    )]
    #[typeshare(serialized_as = "Vec<PlaylistState>")]
    pub(crate) playlists: HashMap<u32, PlaylistState>,
    #[serde(
        default,
        skip_serializing_if = "HashMap::is_empty",
        serialize_with = "into_list",
        deserialize_with = "from_list"
    )]
    #[typeshare(serialized_as = "Vec<CollectionState>")]
    pub(crate) collections: HashMap<u32, CollectionState>,
    #[serde(
        default,
        skip_serializing_if = "HashMap::is_empty",
        serialize_with = "into_list",
        deserialize_with = "from_list"
    )]
    #[typeshare(serialized_as = "Vec<LibraryState>")]
    pub(crate) libraries: HashMap<u32, LibraryState>,
    #[serde(
        default,
        skip_serializing_if = "HashMap::is_empty",
        serialize_with = "into_list",
        deserialize_with = "from_list"
    )]
    #[typeshare(serialized_as = "Vec<ShowState>")]
    pub(crate) shows: HashMap<u32, ShowState>,
    #[serde(
        default,
        skip_serializing_if = "HashMap::is_empty",
        serialize_with = "into_list",
        deserialize_with = "from_list"
    )]
    #[typeshare(serialized_as = "Vec<SeasonState>")]
    pub(crate) seasons: HashMap<u32, SeasonState>,
    #[serde(
        default,
        skip_serializing_if = "HashMap::is_empty",
        serialize_with = "into_list",
        deserialize_with = "from_list"
    )]
    #[typeshare(serialized_as = "Vec<VideoState>")]
    pub(crate) videos: HashMap<u32, VideoState>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[typeshare]
#[serde(rename_all = "camelCase")]
pub(crate) struct State {
    pub(crate) client_id: String,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub(crate) servers: HashMap<String, ServerState>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            client_id: Uuid::new_v4().braced().to_string(),
            servers: Default::default(),
        }
    }
}
