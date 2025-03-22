use std::{
    collections::HashMap,
    fmt,
    io::ErrorKind,
    path::{Path, PathBuf},
};

use anyhow::bail;
use plex_api::{
    Server as PlexServer,
    library::{Collection, FromMetadata, MediaItem, MetadataItem, Part, Playlist, Season, Show},
    media_container::server::library::{Metadata, MetadataType},
    transcode::TranscodeStatus,
};
use serde::{Deserialize, Serialize};
use serde_json::{Number, Value};
use tempfile::NamedTempFile;
use time::{Date, OffsetDateTime};
use tokio::{fs, process::Command};
use tracing::{debug, error, info, instrument, trace, warn};
use typeshare::typeshare;
use uuid::Uuid;

use crate::{
    LockedFile, Result, Server, VideoPart,
    schema::{JsonObject, JsonUtils, MigratableStore, SchemaVersion},
    sync::{OpReadGuard, OpWriteGuard},
};

const SCHEMA_VERSION: u64 = 3;

async fn remove_file(path: &Path) {
    if let Err(e) = fs::remove_file(path).await {
        if e.kind() != ErrorKind::NotFound {
            warn!(?path, error=?e, "Failed to remove file");
        }
    }
}

#[derive(Deserialize, Default, Serialize, Clone, PartialEq)]
#[serde(tag = "state", rename_all = "camelCase")]
pub(crate) enum RelatedFileState {
    #[default]
    None,
    Stored {
        #[serde(with = "time::serde::timestamp")]
        updated: OffsetDateTime,
        path: PathBuf,
    },
}

impl RelatedFileState {
    pub(crate) fn path(&self) -> Option<PathBuf> {
        match self {
            Self::None => None,
            Self::Stored { path, .. } => Some(path.clone()),
        }
    }

    pub(crate) fn file(&self, guard: OpReadGuard, root: &Path) -> Option<LockedFile> {
        let file = self.path()?;

        Some(LockedFile::new(root.join(file), guard))
    }

    pub(crate) fn is_none(&self) -> bool {
        matches!(self, RelatedFileState::None)
    }

    pub(crate) fn needs_update(&self, since: OffsetDateTime) -> bool {
        if let RelatedFileState::Stored { updated, .. } = self {
            since > *updated
        } else {
            true
        }
    }

    #[instrument(level = "trace", skip(root, guard))]
    pub(crate) async fn verify(
        &mut self,
        #[expect(unused)] guard: &OpWriteGuard,
        root: &Path,
        expected_path: &Path,
    ) {
        if let RelatedFileState::Stored { path, updated } = self {
            let file = root.join(&path);

            match fs::metadata(&file).await {
                Ok(stats) => {
                    if !stats.is_file() {
                        trace!(?path, "Removing unexpected directory");
                        let _ = fs::remove_dir_all(&file).await;
                        *self = RelatedFileState::None;

                        return;
                    }
                }
                Err(e) => {
                    if e.kind() == ErrorKind::NotFound {
                        warn!(?path, "File no longer present");
                        *self = RelatedFileState::None;

                        return;
                    } else {
                        error!(?path, error=?e, "Error accessing file");

                        return;
                    }
                }
            }

            if path != expected_path {
                let new_target = root.join(expected_path);

                if let Some(parent) = new_target.parent() {
                    if let Err(e) = fs::create_dir_all(parent).await {
                        warn!(?parent, error=?e, "Failed to create parent directories");
                        return;
                    }
                }

                if let Err(e) = fs::rename(&file, &new_target).await {
                    warn!(?path, ?expected_path, error=?e, "Failed to move file to expected location");
                } else {
                    *self = RelatedFileState::Stored {
                        updated: *updated,
                        path: expected_path.to_owned(),
                    };
                }
            }
        }
    }

    #[instrument(level = "trace", skip(root, guard))]
    pub(crate) async fn delete(&mut self, #[expect(unused)] guard: &OpWriteGuard, root: &Path) {
        if let RelatedFileState::Stored { path, .. } = self {
            let file = root.join(&path);
            trace!(?path, "Removing old file");

            remove_file(&file).await;

            *self = RelatedFileState::None;
        }
    }
}

impl fmt::Debug for RelatedFileState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => write!(f, "None"),
            Self::Stored { .. } => write!(f, "Stored"),
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
    pub(crate) thumbnail: RelatedFileState,
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

    pub(crate) async fn update<T>(&mut self, collection: &Collection<T>) {
        self.title = collection.title().to_owned();

        if let Some(updated) = collection.metadata().updated_at {
            self.last_updated = updated;
        }
    }

    pub(crate) async fn delete(&mut self, guard: &OpWriteGuard, root: &Path) {
        self.thumbnail.delete(guard, root).await;
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[typeshare]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlaylistState {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) videos: Vec<String>,
    #[serde(with = "time::serde::timestamp")]
    #[typeshare(serialized_as = "number")]
    pub(crate) last_updated: OffsetDateTime,
    #[serde(default)]
    pub(crate) thumbnail: RelatedFileState,
}

impl PlaylistState {
    pub(crate) fn from<T>(playlist: &Playlist<T>) -> Self {
        Self {
            id: playlist.rating_key().to_owned(),
            title: playlist.title().to_owned(),
            last_updated: playlist.metadata().updated_at.unwrap(),
            videos: Default::default(),
            thumbnail: RelatedFileState::None,
        }
    }

    pub(crate) fn update<T>(&mut self, playlist: &Playlist<T>) {
        self.title = playlist.title().to_owned();

        if let Some(updated) = playlist.metadata().updated_at {
            self.last_updated = updated;
        }
    }
}

#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq)]
#[typeshare]
#[serde(rename_all = "lowercase")]
pub enum LibraryType {
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
    pub(crate) thumbnail: RelatedFileState,
    #[serde(default, skip_serializing_if = "RelatedFileState::is_none")]
    pub(crate) metadata: RelatedFileState,
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
            metadata: Default::default(),
        }
    }

    pub(crate) async fn update(&mut self, show: &Show) {
        let metadata = show.metadata();

        self.year = metadata.year.unwrap();
        self.title = show.title().to_owned();

        if let Some(updated) = show.metadata().updated_at {
            self.last_updated = updated;
        }
    }

    pub(crate) async fn delete(&mut self, guard: &OpWriteGuard, root: &Path) {
        self.thumbnail.delete(guard, root).await;
        self.metadata.delete(guard, root).await;
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
    Transcoding { session_id: String },
    #[serde(rename_all = "camelCase")]
    TranscodeDownloading { session_id: String, path: PathBuf },
    #[serde(rename_all = "camelCase")]
    Downloaded { path: PathBuf },
    #[serde(rename_all = "camelCase")]
    Transcoded { path: PathBuf },
}

impl DownloadState {
    pub(crate) fn path(&self) -> Option<PathBuf> {
        match self {
            Self::None => None,
            Self::Transcoding { .. } => None,
            Self::Downloading { path } => Some(path.clone()),
            Self::TranscodeDownloading { path, .. } => Some(path.clone()),
            Self::Downloaded { path } => Some(path.clone()),
            Self::Transcoded { path } => Some(path.clone()),
        }
    }

    pub(crate) async fn file(&self, guard: OpReadGuard, root: &Path) -> Option<LockedFile> {
        let file = self.path()?;

        Some(LockedFile::new(root.join(file), guard))
    }

    pub(crate) fn needs_download(&self) -> bool {
        !matches!(
            self,
            DownloadState::Downloaded { .. } | DownloadState::Transcoded { .. }
        )
    }

    #[instrument(level = "trace", skip(root, guard))]
    pub(crate) async fn strip_metadata(
        &self,
        #[expect(unused)] guard: &OpWriteGuard,
        root: &Path,
    ) -> Result {
        let source_file = match self {
            Self::Downloaded { path } => root.join(path),
            Self::Transcoded { path } => root.join(path),
            _ => return Ok(()),
        };

        let temp_file = NamedTempFile::new()?;

        let result = Command::new("ffmpeg")
            .arg("-y")
            .arg("-loglevel")
            .arg("warning")
            .arg("-i")
            .arg(&source_file)
            .arg("-map_metadata")
            .arg("-1")
            .arg("-c")
            .arg("copy")
            .arg("-map")
            .arg("0")
            .arg(temp_file.path())
            .output()
            .await?;

        if result.status.success() {
            fs::copy(temp_file.path(), &source_file).await?;
        }

        Ok(())
    }

    async fn verify_transcode_status(
        &mut self,
        plex_server: &PlexServer,
        video_part: &VideoPart,
        session_id: &str,
    ) {
        match plex_server.transcode_session(session_id).await {
            Ok(session) => {
                let status = match session.status().await {
                    Ok(status) => status,
                    Err(e) => {
                        error!(error=?e, "Failed to get transcode status");
                        return;
                    }
                };

                match status {
                    TranscodeStatus::Complete => {
                        let path = video_part.file_path(&session.container().to_string()).await;
                        if matches!(self, DownloadState::Transcoding { .. }) {
                            *self = DownloadState::TranscodeDownloading {
                                session_id: session_id.to_owned(),
                                path: path.clone(),
                            }
                        }
                    }
                    TranscodeStatus::Error => {
                        error!("Transcode session has failed");
                        let _ = session.cancel().await;

                        *self = DownloadState::None;
                    }
                    TranscodeStatus::Transcoding { .. } => {
                        *self = DownloadState::Transcoding {
                            session_id: session_id.to_owned(),
                        };
                    }
                }
            }
            Err(plex_api::Error::ItemNotFound) => {
                warn!("Transcode session is no longer present");
                *self = DownloadState::None;
            }
            Err(e) => {
                error!(error=?e, "Failed to get transcode session");
            }
        }
    }

    #[instrument(level = "trace", skip(root, guard, plex_server))]
    pub(crate) async fn verify(
        &mut self,
        #[expect(unused)] guard: &OpWriteGuard,
        plex_server: &PlexServer,
        video_part: &VideoPart,
        root: &Path,
    ) {
        match self.clone() {
            DownloadState::None => return,
            DownloadState::Downloading { .. } => {
                return;
            }
            DownloadState::Transcoding { session_id }
            | DownloadState::TranscodeDownloading { session_id, .. } => {
                self.verify_transcode_status(plex_server, video_part, &session_id)
                    .await;
            }
            _ => {}
        }

        let Some(path) = self.path() else {
            return;
        };

        let file = root.join(&path);

        let extension = file
            .extension()
            .and_then(|os| os.to_str())
            .unwrap()
            .to_owned();
        let expected_path = video_part.file_path(&extension).await;

        match fs::metadata(&file).await {
            Ok(stats) => {
                if !stats.is_file() {
                    trace!(?path, "Removing unexpected directory");
                    let _ = fs::remove_dir_all(&file).await;
                    *self = DownloadState::None;

                    return;
                }
            }
            Err(e) => {
                if e.kind() != ErrorKind::NotFound {
                    error!(?path, error=?e, "Error accessing file");
                    return;
                } else {
                    error!(?path, "Download is no longer present");
                    *self = DownloadState::None;

                    return;
                }
            }
        }

        if expected_path != path {
            let new_target = root.join(&expected_path);

            if let Err(e) = fs::rename(&file, &new_target).await {
                warn!(?path, ?expected_path, error=?e, "Failed to move file to expected location");
            } else if matches!(self, DownloadState::Downloaded { .. }) {
                *self = DownloadState::Downloaded {
                    path: expected_path.to_owned(),
                };
            } else if matches!(self, DownloadState::Transcoded { .. }) {
                *self = DownloadState::Transcoded {
                    path: expected_path.to_owned(),
                };
            } else {
                unreachable!("Should have returned early if the video is not local");
            }
        }
    }

    #[instrument(level = "trace", skip(root, guard, plex_server))]
    pub(crate) async fn delete(
        &mut self,
        #[expect(unused)] guard: &OpWriteGuard,
        plex_server: &PlexServer,
        root: &Path,
    ) {
        let path = match self {
            DownloadState::None => return,
            DownloadState::Downloading { path } => path,
            DownloadState::Transcoding { session_id } => {
                if let Ok(session) = plex_server.transcode_session(session_id).await {
                    if let Err(e) = session.cancel().await {
                        warn!(error=?e, "Failed to cancel stale transcode session");
                    }
                }

                return;
            }
            DownloadState::TranscodeDownloading { session_id, path } => {
                if let Ok(session) = plex_server.transcode_session(session_id).await {
                    if let Err(e) = session.cancel().await {
                        warn!(error=?e, "Failed to cancel stale transcode session");
                    }
                }

                path
            }
            DownloadState::Downloaded { path } => path,
            DownloadState::Transcoded { path } => path,
        };

        let file = root.join(&path);

        trace!(?path, "Removing old video file");

        remove_file(&file).await;

        *self = DownloadState::None;
    }
}

impl fmt::Debug for DownloadState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => write!(f, "None"),
            Self::Downloading { .. } => write!(f, "Downloading"),
            Self::Transcoding { session_id } => write!(f, "Transcoding({session_id})"),
            Self::TranscodeDownloading { session_id, .. } => {
                write!(f, "TranscodeDownloading({session_id})")
            }
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
pub enum PlaybackState {
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
    pub(crate) thumbnail: RelatedFileState,
    #[serde(default, skip_serializing_if = "RelatedFileState::is_none")]
    pub(crate) metadata: RelatedFileState,
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
            metadata: Default::default(),
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
        server: &Server,
        item: &M,
        plex_server: &PlexServer,
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
                        match plex_server.update_timeline(item, position).await {
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
                        match plex_server.mark_watched(item).await {
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
            self.last_updated = updated;
        }

        let media = &item.media()[0];
        let parts = media.parts();

        if let Ok(guard) = server.try_lock_write_key(&self.id).await {
            if parts.len() != self.parts.len() {
                info!("Number of video parts changed, deleting existing downloads.");
                for part in self.parts.iter_mut() {
                    part.download.delete(&guard, plex_server, root).await;
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
                        part_state.download.delete(&guard, plex_server, root).await;
                        *part_state = part.into();
                    }
                }
            }
        }
    }

    pub(crate) async fn delete(
        &mut self,
        guard: &OpWriteGuard,
        plex_server: &PlexServer,
        root: &Path,
    ) {
        self.thumbnail.delete(guard, root).await;

        self.metadata.delete(guard, root).await;

        for part in self.parts.iter_mut() {
            part.download.delete(guard, plex_server, root).await;
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
    schema: SchemaVersion<SCHEMA_VERSION>,
    pub(crate) client_id: Uuid,
    #[serde(default)]
    pub(crate) servers: HashMap<String, ServerState>,
}

impl State {
    fn migrate_v0(data: &mut JsonObject) -> Result {
        for thumbnail in data
            .prop("servers")
            .values()
            .prop("videos")
            .values()
            .prop("thumbnail")
            .as_object()
        {
            if thumbnail.get("state") == Some(&Value::String("downloaded".to_string())) {
                thumbnail.insert("state".to_owned(), Value::String("stored".to_owned()));
            }
        }

        for thumbnail in data
            .prop("servers")
            .values()
            .prop("videos")
            .values()
            .prop("metadata")
            .as_object()
        {
            if thumbnail.get("state") == Some(&Value::String("downloaded".to_string())) {
                thumbnail.insert("state".to_owned(), Value::String("stored".to_owned()));
            }
        }

        for thumbnail in data
            .prop("servers")
            .values()
            .prop("collections")
            .values()
            .prop("thumbnail")
            .as_object()
        {
            if thumbnail.get("state") == Some(&Value::String("downloaded".to_string())) {
                thumbnail.insert("state".to_owned(), Value::String("stored".to_owned()));
            }
        }

        for thumbnail in data
            .prop("servers")
            .values()
            .prop("shows")
            .values()
            .prop("thumbnail")
            .as_object()
        {
            if thumbnail.get("state") == Some(&Value::String("downloaded".to_string())) {
                thumbnail.insert("state".to_owned(), Value::String("stored".to_owned()));
            }
        }

        for thumbnail in data
            .prop("servers")
            .values()
            .prop("shows")
            .values()
            .prop("metadata")
            .as_object()
        {
            if thumbnail.get("state") == Some(&Value::String("downloaded".to_string())) {
                thumbnail.insert("state".to_owned(), Value::String("stored".to_owned()));
            }
        }

        Ok(())
    }

    fn migrate_v1(data: &mut JsonObject) -> Result {
        for video in data
            .prop("servers")
            .values()
            .prop("videos")
            .values()
            .as_object()
        {
            if let Some(updated) = video.get("lastUpdated").cloned() {
                if let Some(Value::Object(obj)) = video.get_mut("thumbnail") {
                    if obj.contains_key("path") {
                        obj.insert("updated".to_string(), updated.clone());
                    }
                }

                if let Some(Value::Object(obj)) = video.get_mut("metadata") {
                    if obj.contains_key("path") {
                        obj.insert("updated".to_string(), updated);
                    }
                }
            }
        }

        for show in data
            .prop("servers")
            .values()
            .prop("shows")
            .values()
            .as_object()
        {
            if let Some(updated) = show.get("lastUpdated").cloned() {
                if let Some(Value::Object(obj)) = show.get_mut("thumbnail") {
                    if obj.contains_key("path") {
                        obj.insert("updated".to_string(), updated.clone());
                    }
                }

                if let Some(Value::Object(obj)) = show.get_mut("metadata") {
                    if obj.contains_key("path") {
                        obj.insert("updated".to_string(), updated);
                    }
                }
            }
        }

        for collection in data
            .prop("servers")
            .values()
            .prop("collections")
            .values()
            .as_object()
        {
            if let Some(updated) = collection.get("lastUpdated").cloned() {
                if let Some(Value::Object(obj)) = collection.get_mut("thumbnail") {
                    if obj.contains_key("path") {
                        obj.insert("updated".to_string(), updated);
                    }
                }
            }
        }

        Ok(())
    }

    fn migrate_v2(data: &mut JsonObject) -> Result {
        for playlist in data
            .prop("servers")
            .values()
            .prop("playlists")
            .values()
            .as_object()
        {
            playlist.insert(
                "lastUpdated".to_string(),
                Value::Number(Number::from_u128(0).unwrap()),
            );
        }

        Ok(())
    }
}

impl Default for State {
    fn default() -> Self {
        Self {
            schema: Default::default(),
            client_id: Uuid::new_v4(),
            servers: Default::default(),
        }
    }
}

impl MigratableStore for State {
    fn migrate(data: &mut JsonObject) -> Result<bool> {
        let version = match data.get("schema") {
            None => 0,
            Some(Value::Number(number)) => match number.as_u64() {
                Some(SCHEMA_VERSION) => return Ok(false),
                Some(version) => {
                    if version > SCHEMA_VERSION {
                        bail!("Unexpected schema version");
                    }

                    version
                }
                _ => bail!("Unexpected schema version"),
            },
            _ => bail!("Missing schema version"),
        };

        if version < 1 {
            Self::migrate_v0(data)?;
        }

        if version < 2 {
            Self::migrate_v1(data)?;
        }

        if version < 3 {
            Self::migrate_v2(data)?;
        }

        data.insert("schema".to_string(), SCHEMA_VERSION.into());

        Ok(true)
    }
}
