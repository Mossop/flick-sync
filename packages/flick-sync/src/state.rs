use std::collections::HashMap;
use std::fmt;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use async_std::fs;
use plex_api::{
    Server,
    library::{Collection, MetadataItem, Part, Playlist, Season, Show},
    media_container::server::library::{Metadata, MetadataType},
};
use plex_api::{
    library::{FromMetadata, MediaItem},
    transcode::TranscodeStatus,
};
use serde::{Deserialize, Serialize};
use time::{Date, OffsetDateTime};
use tracing::{debug, error, info, instrument, trace, warn};
use typeshare::typeshare;
use uuid::Uuid;

#[derive(Deserialize, Default, Serialize, Clone, PartialEq)]
#[serde(tag = "state", rename_all = "camelCase")]
pub(crate) enum ThumbnailState {
    #[default]
    None,
    #[serde(rename_all = "camelCase")]
    Downloaded { path: PathBuf },
}

impl ThumbnailState {
    pub(crate) fn file(&self) -> Option<PathBuf> {
        match self {
            Self::None => None,
            Self::Downloaded { path } => Some(path.clone()),
        }
    }

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
            Self::Downloaded { .. } => write!(f, "Downloaded"),
        }
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[typeshare]
#[serde(rename_all = "camelCase")]
pub(crate) struct CollectionState {
    pub(crate) id: String,
    pub(crate) library: String,
    pub(crate) title: String,
    pub(crate) contents: Vec<String>,
    #[serde(with = "time::serde::timestamp")]
    #[typeshare(serialized_as = "number")]
    pub(crate) last_updated: OffsetDateTime,
    pub(crate) thumbnail: ThumbnailState,
}

impl CollectionState {
    pub(crate) fn from<T>(collection: &Collection<T>) -> Self {
        Self {
            id: collection.rating_key().to_owned(),
            library: collection
                .metadata()
                .library_section_id
                .unwrap()
                .to_string(),
            title: collection.title().to_owned(),
            contents: Default::default(),
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
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) videos: Vec<String>,
}

impl PlaylistState {
    pub(crate) fn from<T>(playlist: &Playlist<T>) -> Self {
        Self {
            id: playlist.rating_key().to_owned(),
            title: playlist.title().to_owned(),
            videos: Default::default(),
        }
    }

    pub(crate) fn update<T>(&mut self, playlist: &Playlist<T>) {
        self.title = playlist.title().to_owned();
    }
}

#[derive(Deserialize, Serialize, Clone, Copy, Debug)]
#[typeshare]
#[serde(rename_all = "lowercase")]
pub(crate) enum LibraryType {
    Movie,
    Show,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[typeshare]
#[serde(rename_all = "camelCase")]
pub(crate) struct LibraryState {
    pub(crate) id: String,
    pub(crate) title: String,
    #[serde(rename = "type")]
    pub(crate) library_type: LibraryType,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[typeshare]
#[serde(rename_all = "camelCase")]
pub(crate) struct SeasonState {
    pub(crate) id: String,
    pub(crate) show: String,
    pub(crate) index: u32,
    pub(crate) title: String,
}

impl SeasonState {
    pub(crate) fn from(season: &Season) -> Self {
        let metadata = season.metadata();

        Self {
            id: season.rating_key().to_owned(),
            show: metadata.parent.parent_rating_key.clone().unwrap(),
            index: metadata.index.unwrap(),
            title: season.title().to_owned(),
        }
    }

    pub(crate) fn update(&mut self, season: &Season) {
        let metadata = season.metadata();

        self.index = metadata.index.unwrap();
        self.show = metadata.parent.parent_rating_key.clone().unwrap();
        self.title = season.title().to_owned();
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[typeshare]
#[serde(rename_all = "camelCase")]
pub(crate) struct ShowState {
    pub(crate) id: String,
    pub(crate) library: String,
    pub(crate) title: String,
    pub(crate) year: u32,
    #[serde(with = "time::serde::timestamp")]
    #[typeshare(serialized_as = "number")]
    pub(crate) last_updated: OffsetDateTime,
    pub(crate) thumbnail: ThumbnailState,
}

impl ShowState {
    pub(crate) fn from(show: &Show) -> Self {
        let metadata = show.metadata();

        let year = metadata.year.unwrap();
        let title = show.title().to_owned();

        Self {
            id: show.rating_key().to_owned(),
            library: metadata.library_section_id.unwrap().to_string(),
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
pub(crate) struct MovieDetail {
    pub(crate) library: String,
    pub(crate) year: u32,
}

impl MovieDetail {
    pub(crate) fn from(metadata: &Metadata) -> Self {
        MovieDetail {
            library: metadata.library_section_id.unwrap().to_string(),
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
pub(crate) struct EpisodeDetail {
    pub(crate) season: String,
    pub(crate) index: u32,
}

impl EpisodeDetail {
    pub(crate) fn from(metadata: &Metadata) -> Self {
        EpisodeDetail {
            season: metadata.parent.parent_rating_key.clone().unwrap(),
            index: metadata.index.unwrap(),
        }
    }

    pub(crate) fn update(&mut self, metadata: &Metadata) {
        self.season = metadata.parent.parent_rating_key.clone().unwrap();
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
    pub(crate) fn file(&self) -> Option<PathBuf> {
        match self {
            Self::None => None,
            Self::Downloading { path } => Some(path.clone()),
            Self::Transcoding { path, .. } => Some(path.clone()),
            Self::Downloaded { path } => Some(path.clone()),
            Self::Transcoded { path } => Some(path.clone()),
        }
    }

    pub(crate) fn needs_download(&self) -> bool {
        !matches!(
            self,
            DownloadState::Downloaded { .. } | DownloadState::Transcoded { .. }
        )
    }

    #[instrument(level = "trace", skip(root, server))]
    pub(crate) async fn verify(&mut self, server: &Server, root: &Path) {
        let path = match self {
            DownloadState::None => return,
            DownloadState::Downloading { .. } => {
                return;
            }
            DownloadState::Transcoding { session_id, path } => {
                let file = root.join(&path);

                match server.transcode_session(session_id).await {
                    Ok(session) => {
                        let status = match session.status().await {
                            Ok(status) => status,
                            Err(e) => {
                                error!(?path, error=?e, "Failed to get transcode status");
                                return;
                            }
                        };

                        if !matches!(status, TranscodeStatus::Error) {
                            return;
                        }

                        error!(?path, "Transcode session has failed");
                        let _ = session.cancel().await;
                    }
                    Err(plex_api::Error::ItemNotFound) => {
                        warn!(?path, "Transcode session is no longer present");
                    }
                    Err(e) => {
                        error!(?path, error=?e, "Failed to get transcode session");
                        return;
                    }
                }

                if let Err(e) = fs::remove_file(&file).await {
                    if e.kind() != ErrorKind::NotFound {
                        warn!(?path, error=?e, "Failed to remove partial download");
                    }
                }

                *self = DownloadState::None;

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
            Self::Downloading { .. } => write!(f, "Downloading"),
            Self::Transcoding { session_id, .. } => write!(f, "Transcoding({session_id})"),
            Self::Downloaded { .. } => write!(f, "Downloaded"),
            Self::Transcoded { .. } => write!(f, "Transcoded"),
        }
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[typeshare]
#[serde(rename_all = "camelCase")]
pub(crate) struct VideoPartState {
    pub(crate) id: String,
    pub(crate) key: String,
    #[typeshare(serialized_as = "number")]
    pub(crate) size: u64,
    #[typeshare(serialized_as = "number")]
    pub(crate) duration: u64,
    pub(crate) download: DownloadState,
}

impl<M> From<&Part<'_, M>> for VideoPartState
where
    M: MediaItem,
{
    fn from(part: &Part<'_, M>) -> Self {
        let metadata = part.metadata();
        Self {
            id: metadata.id.clone().unwrap(),
            key: metadata.key.clone().unwrap(),
            size: metadata.size.unwrap(),
            duration: metadata.duration.unwrap(),
            download: Default::default(),
        }
    }
}

impl<M> PartialEq<Part<'_, M>> for VideoPartState
where
    M: MediaItem,
{
    fn eq(&self, part: &Part<'_, M>) -> bool {
        let metadata = part.metadata();
        metadata.id.as_ref().is_some_and(|id| id == &self.id)
            && metadata.key.as_ref().is_some_and(|key| key == &self.key)
            && metadata.size.is_some_and(|size| size == self.size)
            && metadata
                .duration
                .is_some_and(|duration| duration == self.duration)
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(untagged)]
pub(crate) enum VideoDetail {
    Movie(MovieDetail),
    Episode(EpisodeDetail),
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
#[serde(tag = "state", rename_all = "lowercase")]
pub(crate) enum PlaybackState {
    Unplayed,
    InProgress { position: u64 },
    Played,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[typeshare]
#[serde(rename_all = "camelCase")]
pub(crate) struct VideoState {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) detail: VideoDetail,
    #[typeshare(serialized_as = "string")]
    pub(crate) air_date: Date,
    pub(crate) thumbnail: ThumbnailState,
    pub(crate) media_id: String,
    #[serde(with = "time::serde::timestamp")]
    #[typeshare(serialized_as = "number")]
    pub(crate) last_updated: OffsetDateTime,
    pub(crate) parts: Vec<VideoPartState>,
    pub(crate) transcode_profile: Option<String>,
    pub(crate) playback_state: PlaybackState,
    #[serde(default, with = "time::serde::timestamp::option")]
    #[typeshare(serialized_as = "Option<number>")]
    pub(crate) last_viewed_at: Option<OffsetDateTime>,
}

fn playback_state_from_metadata(metadata: &Metadata) -> PlaybackState {
    if let Some(position) = metadata.view_offset {
        PlaybackState::InProgress { position }
    } else if metadata.view_count.is_some() {
        PlaybackState::Played
    } else {
        PlaybackState::Unplayed
    }
}

impl VideoState {
    pub(crate) fn movie_state(&self) -> &MovieDetail {
        match self.detail {
            VideoDetail::Movie(ref m) => m,
            VideoDetail::Episode(_) => panic!("Unexpected type"),
        }
    }

    pub(crate) fn episode_state(&self) -> &EpisodeDetail {
        match self.detail {
            VideoDetail::Movie(_) => panic!("Unexpected type"),
            VideoDetail::Episode(ref e) => e,
        }
    }

    pub(crate) fn from<M: MediaItem>(item: &M) -> Self {
        let metadata = item.metadata();
        let detail = match metadata.metadata_type {
            Some(MetadataType::Movie) => VideoDetail::Movie(MovieDetail::from(metadata)),
            Some(MetadataType::Episode) => VideoDetail::Episode(EpisodeDetail::from(metadata)),
            _ => panic!("Unexpected video type: {:?}", metadata.metadata_type),
        };

        let media = &item.media()[0];
        let parts: Vec<VideoPartState> = media.parts().iter().map(VideoPartState::from).collect();

        Self {
            id: item.rating_key().to_owned(),
            title: item.title().to_owned(),
            detail,
            air_date: metadata.originally_available_at.unwrap(),
            thumbnail: Default::default(),
            media_id: media.metadata().id.clone().unwrap(),
            last_updated: metadata.updated_at.unwrap(),
            parts,
            // Determined later
            transcode_profile: None,
            playback_state: playback_state_from_metadata(metadata),
            last_viewed_at: metadata.last_viewed_at,
        }
    }

    pub(crate) async fn update<M: MediaItem + FromMetadata>(
        &mut self,
        item: &M,
        server: &Server,
        root: &Path,
    ) {
        let metadata = item.metadata();
        self.title = item.title().to_owned();

        let server_state = playback_state_from_metadata(metadata);
        if self.last_viewed_at == metadata.last_viewed_at {
            // No server-side views since last sync.
            if server_state != self.playback_state {
                match self.playback_state {
                    PlaybackState::Unplayed => {
                        // Not going to mark as unplayed on the server for now.
                    }
                    PlaybackState::InProgress { position } => {
                        debug!(
                            video = item.rating_key(),
                            position, "Updating playback position on server"
                        );
                        match server.update_timeline(item, position).await {
                            Ok(item) => {
                                let metadata = item.metadata();
                                self.playback_state = playback_state_from_metadata(metadata);
                            }
                            Err(e) => warn!("Failed to update playback position: {e}"),
                        }
                    }
                    PlaybackState::Played => {
                        debug!(
                            video = item.rating_key(),
                            "Marking item as watched on server"
                        );
                        match server.mark_watched(item).await {
                            Ok(item) => {
                                let metadata = item.metadata();
                                self.playback_state = playback_state_from_metadata(metadata);
                            }
                            Err(e) => warn!("Failed to mark item as watched: {e}"),
                        }
                    }
                }
            }
        } else {
            // Viewed on the server, just take the server's state.
            self.last_viewed_at = metadata.last_viewed_at;
            self.playback_state = server_state;
        }

        match self.detail {
            VideoDetail::Movie(ref mut m) => m.update(metadata),
            VideoDetail::Episode(ref mut e) => e.update(metadata),
        }

        if let Some(updated) = metadata.updated_at {
            if updated > self.last_updated {
                self.thumbnail.delete(root).await;
            }
            self.last_updated = updated;
        }

        let media = &item.media()[0];
        let parts = media.parts();

        if parts.len() != self.parts.len() {
            info!("Number of video parts changed, deleting existing downloads.");
            for part in self.parts.iter_mut() {
                part.download.delete(server, root).await;
            }

            self.parts = parts.iter().map(VideoPartState::from).collect()
        } else {
            for (part_state, part) in self.parts.iter_mut().zip(parts.iter()) {
                let metadata = part.metadata();

                if part_state != part {
                    info!(
                        old_id = part_state.id,
                        new_id = metadata.id,
                        old_key = part_state.key,
                        new_key = metadata.key,
                        old_size = part_state.size,
                        new_size = metadata.size,
                        old_duration = part_state.duration,
                        new_duration = metadata.duration,
                        part = part_state.id,
                        "Part changed, deleting existing download."
                    );
                    part_state.download.delete(server, root).await;
                    *part_state = part.into();
                }
            }
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
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub(crate) playlists: HashMap<String, PlaylistState>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub(crate) collections: HashMap<String, CollectionState>,
    #[serde(default)]
    pub(crate) libraries: HashMap<String, LibraryState>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub(crate) shows: HashMap<String, ShowState>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub(crate) seasons: HashMap<String, SeasonState>,
    #[serde(default)]
    pub(crate) videos: HashMap<String, VideoState>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[typeshare]
#[serde(rename_all = "camelCase")]
pub(crate) struct State {
    pub(crate) client_id: String,
    #[serde(default)]
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
