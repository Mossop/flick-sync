use std::{
    fmt,
    hash::{Hash, Hasher},
    io::{ErrorKind, IoSlice},
    ops::{Add, AddAssign},
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    pin::Pin,
    result,
    task::{Context, Poll},
    time::Duration,
};

use anyhow::{anyhow, bail};
use futures::io::AsyncWrite;
use pathdiff::diff_paths;
use pin_project::pin_project;
use plex_api::{
    Server as PlexServer,
    library::{self, Item, MediaItemWithTranscoding, MetadataItem},
    media_container::server::library::ContainerFormat,
    transcode::{DownloadQueue, QueueItemStatus, VideoTranscodeOptions},
};
use time::{Date, OffsetDateTime};
use tokio::{
    fs::{File, OpenOptions, create_dir_all, metadata},
    io::{AsyncWriteExt, BufWriter},
    sync::OwnedSemaphorePermit,
    time::sleep,
};
use tracing::{debug, info, instrument, trace, warn};
use xml::{EmitterConfig, writer::XmlEvent};

use crate::{
    DownloadProgress, FlickSync, LockedFile, Result, Server,
    config::OutputStyle,
    server::Progress,
    state::{
        CollectionState, DownloadState, LibraryState, LibraryType, PlaybackState, PlaylistState,
        RelatedFileState, SeasonState, ServerState, ShowState, VideoDetail, VideoPartState,
        VideoState,
    },
    sync::{OpReadGuard, OpWriteGuard, Timeout},
    util::{AsyncWriteAdapter, safe},
};

type EventWriter = xml::writer::EventWriter<std::fs::File>;

const METADATA_DIR: &str = ".metadata";

#[derive(Debug, Clone, Copy)]
pub(crate) enum FileType {
    Video,
    Thumbnail,
    Metadata,
    Playlist,
}

impl FileType {
    fn is_metadata(&self) -> bool {
        !matches!(self, FileType::Video)
    }
}

macro_rules! state_wrapper {
    ($typ:ident, $st_typ:ident, $prop:ident) => {
        impl $typ {
            pub fn id(&self) -> &str {
                &self.id
            }

            pub fn flick_sync(&self) -> FlickSync {
                FlickSync {
                    inner: self.server.inner.clone(),
                }
            }

            pub fn server(&self) -> Server {
                self.server.clone()
            }

            pub async fn title(&self) -> String {
                self.with_state(|s| s.title.clone()).await
            }

            #[allow(unused)]
            async fn try_lock_write(&self) -> result::Result<OpWriteGuard, Timeout> {
                self.server.try_lock_write_key(&self.id).await
            }

            #[allow(unused)]
            async fn try_lock_read(&self) -> result::Result<OpReadGuard, Timeout> {
                self.server.try_lock_read_key(&self.id).await
            }

            async fn with_server_state<F, R>(&self, cb: F) -> R
            where
                F: Send + FnOnce(&ServerState) -> R,
            {
                let state = self.server.inner.state.read().await;
                cb(&state.servers.get(&self.server.id).unwrap())
            }

            async fn with_state<F, R>(&self, cb: F) -> R
            where
                F: Send + FnOnce(&$st_typ) -> R,
            {
                self.with_server_state(|ss| cb(ss.$prop.get(&self.id).unwrap()))
                    .await
            }

            #[allow(unused)]
            async fn update_state<F>(&self, cb: F) -> Result
            where
                F: Send + FnOnce(&mut $st_typ),
            {
                let mut state = self.server.inner.state.write().await;
                let server_state = state.servers.get_mut(&self.server.id).unwrap();
                cb(server_state.$prop.get_mut(&self.id).unwrap());
                self.server.inner.persist_state(&state).await
            }
        }
    };
}

macro_rules! wrapper_builders {
    ($typ:ident, $st_typ:ident) => {
        impl $typ {
            pub(crate) fn wrap(server: &crate::server::Server, state: &$st_typ) -> Self {
                Self {
                    server: server.clone(),
                    id: state.id.clone(),
                }
            }

            #[allow(dead_code)]
            pub(crate) fn wrap_from_id(server: &crate::server::Server, id: &str) -> Self {
                Self {
                    server: server.clone(),
                    id: id.to_owned(),
                }
            }
        }
    };
}

macro_rules! thumbnail_methods {
    () => {
        pub async fn thumbnail(&self) -> result::Result<Option<LockedFile>, Timeout> {
            let guard = self.try_lock_read().await?;

            let thumbnail_state = self.with_state(|s| s.thumbnail.clone()).await;

            Ok(thumbnail_state.file(guard, &self.server.inner.path))
        }

        #[instrument(level = "trace")]
        pub(crate) async fn update_thumbnail(&self, rebuild: bool) -> Result {
            let Ok(guard) = self.try_lock_write().await else {
                return Ok(());
            };

            let (mut thumbnail, last_updated) = self
                .with_state(|s| (s.thumbnail.clone(), s.last_updated))
                .await;

            let Some(thumbnail_path) = self.file_path(FileType::Thumbnail, "jpg").await else {
                thumbnail.delete(&guard, &self.server.inner.path).await;
                return self.update_state(|s| s.thumbnail = thumbnail.clone()).await;
            };

            let must_download = if rebuild {
                thumbnail.delete(&guard, &self.server.inner.path).await;
                true
            } else {
                thumbnail
                    .verify(&guard, &self.server.inner.path, &thumbnail_path)
                    .await;
                thumbnail.needs_update(last_updated)
            };

            self.update_state(|s| s.thumbnail = thumbnail.clone())
                .await?;

            if must_download {
                let server = self.server.connect().await?;
                let item = server.item_by_id(&self.id).await?;
                debug!("Updating thumbnail for {}", item.title());

                let image = if let Some(ref thumb) = item.metadata().thumb {
                    thumb.clone()
                } else if let Some(ref composite) = item.metadata().composite {
                    composite.clone()
                } else {
                    warn!("No thumbnail found for {}", item.title());
                    return Ok(());
                };

                let target = self.server.inner.path.join(&thumbnail_path);

                if let Some(parent) = target.parent() {
                    create_dir_all(parent).await?;
                }

                let file = AsyncWriteAdapter::new(File::create(&target).await?);
                server
                    .transcode_artwork(&image, 320, 320, Default::default(), file)
                    .await?;

                let state = RelatedFileState::Stored {
                    path: thumbnail_path,
                    updated: OffsetDateTime::now_utc(),
                };

                self.update_state(|s| s.thumbnail = state).await?;
                trace!("Thumbnail for {} successfully updated", item.title());
            }

            Ok(())
        }
    };
}

macro_rules! metadata_methods {
    () => {
        #[instrument(level = "trace")]
        pub(crate) async fn update_metadata(&self, rebuild: bool) -> Result {
            let Ok(guard) = self.try_lock_write().await else {
                return Ok(());
            };

            let (mut metadata, last_updated) = self
                .with_state(|s| (s.metadata.clone(), s.last_updated))
                .await;

            let Some(metadata_path) = self.file_path(FileType::Metadata, "nfo").await else {
                metadata.delete(&guard, &self.server.inner.path).await;
                return self.update_state(|s| s.metadata = metadata.clone()).await;
            };

            let must_create = if rebuild {
                metadata.delete(&guard, &self.server.inner.path).await;
                true
            } else {
                metadata
                    .verify(&guard, &self.server.inner.path, &metadata_path)
                    .await;
                metadata.needs_update(last_updated)
            };

            self.update_state(|s| s.metadata = metadata.clone()).await?;

            if must_create {
                let target = self.server.inner.path.join(&metadata_path);

                if let Some(parent) = target.parent() {
                    create_dir_all(parent).await?;
                }

                let output = std::fs::File::create(&target)?;

                let mut writer = EmitterConfig::new()
                    .perform_indent(true)
                    .create_writer(output);

                self.write_metadata(&mut writer).await?;

                let state = RelatedFileState::Stored {
                    path: metadata_path,
                    updated: OffsetDateTime::now_utc(),
                };

                self.update_state(|s| s.metadata = state).await?;
                trace!("Metadata for {} successfully updated", self.id);
            }

            Ok(())
        }
    };
}

macro_rules! parent {
    ($meth:ident, $typ:ident, $($pprop:tt)*) => {
        pub async fn $meth(&self) -> $typ {
            self.with_state(|ss| $typ::wrap_from_id(&self.server, &ss.$($pprop)*))
            .await
        }
    };
}

macro_rules! children {
    ($meth:ident, $prop:ident, $typ:ident, $($pprop:tt)*) => {
        pub async fn $meth(&self) -> Vec<$typ> {
            self.with_server_state(|ss| {
                ss.$prop
                    .values()
                    .filter_map(|s| {
                        if s.$($pprop)* == self.id {
                            Some($typ::wrap(&self.server, s))
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

async fn write_playlist(root: &Path, playlist_path: &Path, videos: Vec<Video>) -> Result {
    let target = root.join(playlist_path);
    let parent = target.parent().unwrap();

    create_dir_all(&parent).await?;
    let output = File::create(&target).await?;
    let mut writer = BufWriter::new(output);

    for video in videos {
        let download = video.download_state().await;

        if let Some(video_path) = download.path()
            && !download.needs_download()
            && let Some(relative) = diff_paths(root.join(video_path), parent)
        {
            writer
                .write_all(relative.as_os_str().as_encoded_bytes())
                .await?;
            writer.write_all(b"\n").await?;
        }
    }

    writer.flush().await?;
    writer.shutdown().await?;

    Ok(())
}

#[derive(Clone)]
pub struct Show {
    server: Server,
    id: String,
}

impl fmt::Debug for Show {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(&format!("Show({})", self.id))
    }
}

state_wrapper!(Show, ShowState, shows);
wrapper_builders!(Show, ShowState);

impl Show {
    thumbnail_methods!();
    metadata_methods!();
    parent!(library, ShowLibrary, library);

    pub async fn seasons(&self) -> Vec<Season> {
        self.with_server_state(|ss| {
            let mut season_states: Vec<&SeasonState> = ss
                .seasons
                .values()
                .filter(|ss| ss.show == self.id)
                .collect();

            season_states.sort_by(|sa, sb| sa.index.cmp(&sb.index));

            season_states
                .into_iter()
                .map(|ss| Season::wrap(&self.server, ss))
                .collect()
        })
        .await
    }

    async fn write_metadata(&self, writer: &mut EventWriter) -> Result {
        self.with_state(|state| {
            writer.write(XmlEvent::start_element("tvshow"))?;

            writer.write(XmlEvent::start_element("title"))?;
            writer.write(XmlEvent::characters(&state.title))?;
            writer.write(XmlEvent::end_element())?;

            writer.write(XmlEvent::end_element())?;

            Ok(())
        })
        .await
    }

    pub(crate) async fn file_path(&self, file_type: FileType, extension: &str) -> Option<PathBuf> {
        let output_style = self.server.inner.output_style().await;

        self.with_server_state(|ss| {
            let state = ss.shows.get(&self.id).unwrap();

            let name = match file_type {
                FileType::Video | FileType::Playlist => {
                    return None;
                }
                FileType::Thumbnail => {
                    if output_style == OutputStyle::Standardized {
                        format!("dvdcover.{extension}")
                    } else {
                        format!("{}.{extension}", &self.id)
                    }
                }
                FileType::Metadata => {
                    if output_style != OutputStyle::Standardized {
                        return None;
                    }

                    format!("tvshow.{extension}")
                }
            };

            Some(if output_style == OutputStyle::Standardized {
                let library_title = &ss.libraries.get(&state.library).unwrap().title;

                PathBuf::from(safe(&self.server.id))
                    .join(safe(library_title))
                    .join(safe(format!("{} ({})", state.title, state.year)))
                    .join(safe(name))
            } else {
                PathBuf::from(safe(&self.server.id))
                    .join(METADATA_DIR)
                    .join(safe(name))
            })
        })
        .await
    }
}

#[derive(Clone)]
pub struct Season {
    server: Server,
    id: String,
}

impl fmt::Debug for Season {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(&format!("Season({})", self.id))
    }
}

state_wrapper!(Season, SeasonState, seasons);
wrapper_builders!(Season, SeasonState);

impl Season {
    parent!(show, Show, show);

    pub async fn index(&self) -> usize {
        self.with_state(|ss| ss.index).await
    }

    pub async fn episodes(&self) -> Vec<Episode> {
        self.with_server_state(|ss| {
            let mut episode_states: Vec<(usize, &VideoState)> = ss
                .videos
                .values()
                .filter_map(|vs| {
                    if let VideoDetail::Episode(ref detail) = vs.detail {
                        if detail.season == self.id {
                            Some((detail.index, vs))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
                .collect();

            episode_states.sort_by(|(ia, _), (ib, _)| ia.cmp(ib));

            episode_states
                .into_iter()
                .map(|(_, vs)| Episode::wrap(&self.server, vs))
                .collect()
        })
        .await
    }
}

#[pin_project]
struct WriterProgress<'a, W, P> {
    offset: u64,
    #[pin]
    writer: W,
    progress: &'a mut P,
    access_permit: &'a mut Option<OwnedSemaphorePermit>,
}

impl<W, P> AsyncWrite for WriterProgress<'_, W, P>
where
    W: AsyncWrite + Unpin,
    P: Progress + Unpin,
{
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<result::Result<usize, futures::io::Error>> {
        let this = self.project();
        let result = this.writer.poll_write(cx, buf);

        if let Poll::Ready(Ok(count)) = result {
            *this.offset += count as u64;
            if *this.offset > 8096 {
                this.access_permit.take();
            }
            this.progress.progress(*this.offset);
        }

        result
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[IoSlice<'_>],
    ) -> Poll<result::Result<usize, futures::io::Error>> {
        let this = self.project();
        let result = this.writer.poll_write_vectored(cx, bufs);

        if let Poll::Ready(Ok(count)) = result {
            this.access_permit.take();
            *this.offset += count as u64;
            this.progress.progress(*this.offset);
        }

        result
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<result::Result<(), futures::io::Error>> {
        let writer = Pin::new(&mut self.get_mut().writer);
        writer.poll_flush(cx)
    }

    fn poll_close(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<result::Result<(), futures::io::Error>> {
        let writer = Pin::new(&mut self.get_mut().writer);
        writer.poll_close(cx)
    }
}

#[derive(Clone, PartialEq)]
pub enum TransferState {
    Waiting,
    Transcoding,
    Downloading,
    Downloaded,
}

#[derive(Clone)]
pub struct VideoPart {
    server: Server,
    id: String,
    index: usize,
}

impl fmt::Debug for VideoPart {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(&format!("Show({}, {})", self.id, self.index))
    }
}

impl VideoPart {
    async fn with_server_state<F, R>(&self, cb: F) -> R
    where
        F: Send + FnOnce(&ServerState) -> R,
    {
        let state = self.server.inner.state.read().await;
        cb(state.servers.get(&self.server.id).unwrap())
    }

    async fn with_state<F, R>(&self, cb: F) -> R
    where
        F: Send + FnOnce(&VideoPartState) -> R,
    {
        self.with_video_state(|vs| cb(vs.parts.get(self.index).unwrap()))
            .await
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn index(&self) -> usize {
        self.index
    }

    async fn with_video_state<F, R>(&self, cb: F) -> R
    where
        F: Send + FnOnce(&VideoState) -> R,
    {
        self.with_server_state(|ss| cb(ss.videos.get(&self.id).unwrap()))
            .await
    }

    pub async fn duration(&self) -> Duration {
        self.with_state(|vs| Duration::from_millis(vs.duration))
            .await
    }

    pub async fn video(&self) -> Video {
        self.with_video_state(|video_state| Video::wrap(&self.server, video_state))
            .await
    }

    pub(crate) async fn remote_size(&self) -> u64 {
        self.with_state(|vps| vps.size).await
    }
}

#[derive(Clone, Copy, Default)]
pub struct VideoStats {
    pub local_videos: u32,
    pub remote_videos: u32,
    pub local_bytes: u64,
    pub remote_bytes: u64,
    pub local_duration: Duration,
    pub remote_duration: Duration,
}

impl Add for VideoStats {
    type Output = VideoStats;

    fn add(self, rhs: VideoStats) -> VideoStats {
        Self {
            local_videos: self.local_videos + rhs.local_videos,
            remote_videos: self.remote_videos + rhs.remote_videos,
            local_bytes: self.local_bytes + rhs.local_bytes,
            remote_bytes: self.remote_bytes + rhs.remote_bytes,
            local_duration: self.local_duration + rhs.local_duration,
            remote_duration: self.remote_duration + rhs.remote_duration,
        }
    }
}

impl AddAssign for VideoStats {
    fn add_assign(&mut self, rhs: VideoStats) {
        self.local_videos += rhs.local_videos;
        self.remote_videos += rhs.remote_videos;
        self.local_bytes += rhs.local_bytes;
        self.remote_bytes += rhs.remote_bytes;
        self.local_duration += rhs.local_duration;
        self.remote_duration += rhs.remote_duration;
    }
}

impl VideoStats {
    async fn from(video: &Video) -> Self {
        let mut stats = VideoStats::default();

        let state = video.download_state().await;
        if !state.needs_download() {
            stats.local_videos += 1;
        }

        for part in video.parts().await {
            stats.remote_videos += 1;

            let part_duration = part.duration().await;
            stats.remote_duration += part_duration;

            if !state.needs_download() {
                stats.local_duration += part_duration;
            }

            let mut remote_bytes = part.remote_size().await;

            if let Some(path) = state.path() {
                let path = part.server.inner.path.join(path);
                if let Ok(file_stats) = metadata(path).await {
                    stats.local_bytes += file_stats.len();
                    remote_bytes = file_stats.len();
                }
            }

            stats.remote_bytes += remote_bytes;
        }

        stats
    }
}

#[derive(Clone)]
pub struct Episode {
    server: Server,
    id: String,
}

impl fmt::Debug for Episode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(&format!("Episode({})", self.id))
    }
}

impl PartialEq for Episode {
    fn eq(&self, other: &Self) -> bool {
        self.server.id == other.server.id && self.id == other.id
    }
}

impl Eq for Episode {}

impl Hash for Episode {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.server.id.hash(state);
        self.id.hash(state);
    }
}

state_wrapper!(Episode, VideoState, videos);
wrapper_builders!(Episode, VideoState);

impl Episode {
    thumbnail_methods!();
    metadata_methods!();
    parent!(season, Season, episode_state().season);

    pub fn video(&self) -> Video {
        Video::Episode(self.clone())
    }

    pub async fn index(&self) -> usize {
        self.with_state(|vs| vs.episode_state().index).await
    }

    pub async fn air_date(&self) -> Option<Date> {
        self.with_state(|vs| vs.air_date).await
    }

    pub async fn playback_state(&self) -> PlaybackState {
        self.with_state(|vs| vs.playback_state.clone()).await
    }

    pub async fn last_played(&self) -> Option<OffsetDateTime> {
        self.with_state(|vs| vs.last_viewed_at).await
    }

    pub async fn next_episode(&self) -> Option<Episode> {
        let season = self.season().await;
        if let Some(ep) = season
            .episodes()
            .await
            .into_iter()
            .nth(self.index().await + 1)
        {
            return Some(ep);
        }

        let show = season.show().await;
        if let Some(season) = show
            .seasons()
            .await
            .into_iter()
            .nth(season.index().await + 1)
        {
            season.episodes().await.into_iter().next()
        } else {
            None
        }
    }

    pub async fn set_playback_state(&self, state: PlaybackState) -> Result {
        self.update_state(|vs| {
            vs.playback_state = state;
        })
        .await
    }

    pub async fn duration(&self) -> Duration {
        let mut total = Duration::from_millis(0);

        for part in self.parts().await {
            total += part.duration().await;
        }

        total
    }

    async fn write_metadata(&self, writer: &mut EventWriter) -> Result {
        let season = self.season().await.with_state(|ss| ss.index).await;
        let show = self.show().await.with_state(|ss| ss.title.clone()).await;

        self.with_state(|state| {
            writer.write(XmlEvent::start_element("episodedetails"))?;

            writer.write(XmlEvent::start_element("title"))?;
            writer.write(XmlEvent::characters(&state.title))?;
            writer.write(XmlEvent::end_element())?;

            writer.write(XmlEvent::start_element("showtitle"))?;
            writer.write(XmlEvent::characters(&show))?;
            writer.write(XmlEvent::end_element())?;

            writer.write(XmlEvent::start_element("season"))?;
            writer.write(XmlEvent::characters(&season.to_string()))?;
            writer.write(XmlEvent::end_element())?;

            writer.write(XmlEvent::start_element("episode"))?;
            writer.write(XmlEvent::characters(
                &state.episode_state().index.to_string(),
            ))?;
            writer.write(XmlEvent::end_element())?;

            writer.write(XmlEvent::end_element())?;

            Ok(())
        })
        .await
    }

    pub async fn stats(&self) -> VideoStats {
        VideoStats::from(&self.video()).await
    }

    pub async fn is_downloaded(&self) -> bool {
        self.video().is_downloaded().await
    }

    pub async fn show(&self) -> Show {
        self.season().await.show().await
    }

    pub async fn library(&self) -> ShowLibrary {
        self.show().await.library().await
    }

    pub async fn parts(&self) -> Vec<VideoPart> {
        self.with_state(|vs| {
            vs.parts
                .iter()
                .enumerate()
                .map(|(index, _)| VideoPart {
                    server: self.server.clone(),
                    id: self.id.clone(),
                    index,
                })
                .collect()
        })
        .await
    }

    pub(crate) async fn file_path(&self, file_type: FileType, extension: &str) -> Option<PathBuf> {
        let output_style = self.server.inner.output_style().await;

        self.with_server_state(|ss| {
            let state = ss.videos.get(&self.id).unwrap();
            let ep_state = state.episode_state();
            let season = ss.seasons.get(&ep_state.season).unwrap();
            let show = ss.shows.get(&season.show).unwrap();

            let name = match file_type {
                FileType::Playlist => return None,
                FileType::Video => {
                    format!(
                        "S{:02}E{:02} - {}.{extension}",
                        season.index, ep_state.index, state.title
                    )
                }
                FileType::Thumbnail => {
                    if output_style == OutputStyle::Standardized {
                        format!(
                            "S{:02}E{:02} - {}.{extension}",
                            season.index, ep_state.index, state.title
                        )
                    } else {
                        format!("{}.{extension}", self.id)
                    }
                }
                FileType::Metadata => {
                    if output_style != OutputStyle::Standardized {
                        return None;
                    }

                    format!(
                        "S{:02}E{:02} - {}.{extension}",
                        season.index, ep_state.index, state.title
                    )
                }
            };

            Some(
                if output_style == OutputStyle::Standardized || !file_type.is_metadata() {
                    let library_title = &ss.libraries.get(&show.library).unwrap().title;

                    PathBuf::from(safe(&self.server.id))
                        .join(safe(library_title))
                        .join(safe(format!("{} ({})", show.title, show.year)))
                        .join(safe(name))
                } else {
                    PathBuf::from(safe(&self.server.id))
                        .join(METADATA_DIR)
                        .join(safe(name))
                },
            )
        })
        .await
    }
}

#[derive(Clone)]
pub struct Movie {
    server: Server,
    id: String,
}

impl PartialEq for Movie {
    fn eq(&self, other: &Self) -> bool {
        self.server.id == other.server.id && self.id == other.id
    }
}

impl Eq for Movie {}

impl Hash for Movie {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.server.id.hash(state);
        self.id.hash(state);
    }
}

impl fmt::Debug for Movie {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(&format!("Movie({})", self.id))
    }
}

state_wrapper!(Movie, VideoState, videos);
wrapper_builders!(Movie, VideoState);

impl Movie {
    thumbnail_methods!();
    metadata_methods!();
    parent!(library, MovieLibrary, movie_state().library);

    pub fn video(&self) -> Video {
        Video::Movie(self.clone())
    }

    pub async fn air_date(&self) -> Option<Date> {
        self.with_state(|vs| vs.air_date).await
    }

    pub async fn playback_state(&self) -> PlaybackState {
        self.with_state(|vs| vs.playback_state.clone()).await
    }

    pub async fn last_played(&self) -> Option<OffsetDateTime> {
        self.with_state(|vs| vs.last_viewed_at).await
    }

    pub async fn next_movie(&self) -> Option<Movie> {
        let self_air_date = self.with_state(|vs| vs.air_date).await;
        let mut lowest = None;

        for collection in self.library().await.collections().await {
            if collection.contains(self).await {
                for movie in collection.movies().await {
                    if movie.id() == self.id() {
                        continue;
                    }

                    let movie_air_date = movie.with_state(|vs| vs.air_date).await;
                    if movie_air_date < self_air_date {
                        continue;
                    }

                    if let Some((current_lowest, current_movie)) = lowest.take() {
                        if current_lowest < movie_air_date {
                            lowest = Some((current_lowest, current_movie));
                        } else {
                            lowest = Some((movie_air_date, movie));
                        }
                    } else {
                        lowest = Some((movie_air_date, movie));
                    }
                }

                if let Some((_, movie)) = lowest {
                    return Some(movie);
                }
            }
        }

        None
    }

    pub async fn set_playback_state(&self, state: PlaybackState) -> Result {
        self.update_state(|vs| {
            vs.playback_state = state;
        })
        .await
    }

    pub async fn duration(&self) -> Duration {
        let mut total = Duration::from_millis(0);

        for part in self.parts().await {
            total += part.duration().await;
        }

        total
    }

    async fn write_metadata(&self, writer: &mut EventWriter) -> Result {
        self.with_state(|state| {
            writer.write(XmlEvent::start_element("movie"))?;

            writer.write(XmlEvent::start_element("title"))?;
            writer.write(XmlEvent::characters(&state.title))?;
            writer.write(XmlEvent::end_element())?;

            writer.write(XmlEvent::end_element())?;

            Ok(())
        })
        .await
    }

    pub async fn is_downloaded(&self) -> bool {
        self.video().is_downloaded().await
    }

    pub async fn stats(&self) -> VideoStats {
        VideoStats::from(&self.video()).await
    }

    pub async fn parts(&self) -> Vec<VideoPart> {
        self.with_state(|vs| {
            vs.parts
                .iter()
                .enumerate()
                .map(|(index, _)| VideoPart {
                    server: self.server.clone(),
                    id: self.id.clone(),
                    index,
                })
                .collect()
        })
        .await
    }

    pub(crate) async fn file_path(&self, file_type: FileType, extension: &str) -> Option<PathBuf> {
        let output_style = self.server.inner.output_style().await;

        self.with_server_state(|ss| {
            let state = ss.videos.get(&self.id).unwrap();
            let m_state = state.movie_state();

            let name = match file_type {
                FileType::Playlist => return None,
                FileType::Video => {
                    format!("{} ({}).{extension}", state.title, m_state.year)
                }
                FileType::Thumbnail => {
                    if output_style != OutputStyle::Standardized {
                        format!("{}.{extension}", self.id)
                    } else if state.parts.len() == 1 {
                        format!("{} ({}).{extension}", state.title, m_state.year)
                    } else {
                        format!("movie.{extension}")
                    }
                }
                FileType::Metadata => {
                    if output_style != OutputStyle::Standardized {
                        return None;
                    }

                    if state.parts.len() == 1 {
                        format!("{} ({}).{extension}", state.title, m_state.year)
                    } else {
                        format!("movie.{extension}")
                    }
                }
            };

            Some(
                if output_style == OutputStyle::Standardized || !file_type.is_metadata() {
                    let library_title = &ss.libraries.get(&m_state.library).unwrap().title;

                    let mut base = PathBuf::from(safe(&self.server.id)).join(safe(library_title));

                    if output_style == OutputStyle::Standardized {
                        base = base.join(safe(format!("{} ({})", state.title, m_state.year)));
                    }

                    base.join(safe(name))
                } else {
                    PathBuf::from(safe(&self.server.id))
                        .join(METADATA_DIR)
                        .join(safe(name))
                },
            )
        })
        .await
    }
}

#[derive(Clone)]
pub enum Video {
    Movie(Movie),
    Episode(Episode),
}

impl PartialEq for Video {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Movie(v1), Self::Movie(v2)) => v1 == v2,
            (Self::Episode(v1), Self::Episode(v2)) => v1 == v2,
            _ => false,
        }
    }
}

impl Eq for Video {}

impl Hash for Video {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Self::Movie(v) => v.hash(state),
            Self::Episode(v) => v.hash(state),
        }
    }
}

impl Video {
    pub async fn playback_state(&self) -> PlaybackState {
        match self {
            Self::Movie(v) => v.playback_state().await,
            Self::Episode(v) => v.playback_state().await,
        }
    }

    pub async fn air_date(&self) -> Option<Date> {
        match self {
            Self::Movie(v) => v.air_date().await,
            Self::Episode(v) => v.air_date().await,
        }
    }

    pub async fn last_played(&self) -> Option<OffsetDateTime> {
        match self {
            Self::Movie(v) => v.last_played().await,
            Self::Episode(v) => v.last_played().await,
        }
    }

    pub async fn next_video(&self) -> Option<Video> {
        match self {
            Self::Movie(v) => v.next_movie().await.map(Video::Movie),
            Self::Episode(v) => v.next_episode().await.map(Video::Episode),
        }
    }

    pub async fn set_playback_state(&self, state: PlaybackState) -> Result {
        match self {
            Self::Movie(v) => v.set_playback_state(state).await,
            Self::Episode(v) => v.set_playback_state(state).await,
        }
    }

    pub async fn set_playback_position(&self, position: u64) -> Result {
        trace!(video = self.id(), position, "Updating playback position");

        let new_state = if position <= (5 * 60000) {
            PlaybackState::Unplayed
        } else {
            let duration = self.duration().await.as_millis() as f64;
            let percent_played = (100 * position) as f64 / duration;

            if (100.0 - percent_played) <= 12.5 {
                PlaybackState::Played
            } else {
                PlaybackState::InProgress { position }
            }
        };

        self.set_playback_state(new_state).await
    }

    pub async fn duration(&self) -> Duration {
        match self {
            Self::Movie(v) => v.duration().await,
            Self::Episode(v) => v.duration().await,
        }
    }

    pub async fn thumbnail(&self) -> result::Result<Option<LockedFile>, Timeout> {
        match self {
            Self::Movie(v) => v.thumbnail().await,
            Self::Episode(v) => v.thumbnail().await,
        }
    }

    pub(crate) fn wrap(server: &Server, state: &VideoState) -> Self {
        match state.detail {
            VideoDetail::Movie(_) => Self::Movie(Movie::wrap(server, state)),
            VideoDetail::Episode(_) => Self::Episode(Episode::wrap(server, state)),
        }
    }

    pub fn flick_sync(&self) -> FlickSync {
        match self {
            Self::Movie(v) => v.flick_sync(),
            Self::Episode(v) => v.flick_sync(),
        }
    }

    pub async fn library(&self) -> Library {
        match self {
            Self::Movie(v) => Library::Movie(v.library().await),
            Self::Episode(v) => Library::Show(v.library().await),
        }
    }

    pub async fn stats(&self) -> VideoStats {
        match self {
            Self::Movie(v) => v.stats().await,
            Self::Episode(v) => v.stats().await,
        }
    }

    pub fn server(&self) -> Server {
        match self {
            Self::Movie(v) => v.server(),
            Self::Episode(v) => v.server(),
        }
    }

    pub fn id(&self) -> &str {
        match self {
            Self::Movie(v) => &v.id,
            Self::Episode(v) => &v.id,
        }
    }

    pub async fn title(&self) -> String {
        match self {
            Self::Movie(v) => v.title().await,
            Self::Episode(v) => v.title().await,
        }
    }

    pub async fn parts(&self) -> Vec<VideoPart> {
        match self {
            Self::Movie(v) => v.parts().await,
            Self::Episode(v) => v.parts().await,
        }
    }

    pub(crate) async fn file_path(&self, file_type: FileType, extension: &str) -> Option<PathBuf> {
        match self {
            Self::Movie(v) => v.file_path(file_type, extension).await,
            Self::Episode(v) => v.file_path(file_type, extension).await,
        }
    }

    pub async fn update_thumbnail(&self, rebuild: bool) -> Result {
        match self {
            Self::Movie(v) => v.update_thumbnail(rebuild).await,
            Self::Episode(v) => v.update_thumbnail(rebuild).await,
        }
    }

    pub async fn update_metadata(&self, rebuild: bool) -> Result {
        match self {
            Self::Movie(v) => v.update_metadata(rebuild).await,
            Self::Episode(v) => v.update_metadata(rebuild).await,
        }
    }

    pub(crate) async fn transfer_state(&self) -> TransferState {
        let download_state = self.download_state().await;

        match download_state {
            DownloadState::None => TransferState::Waiting,
            DownloadState::Downloading { .. } => TransferState::Downloading,
            DownloadState::Transcoding { .. } => TransferState::Transcoding,
            _ => TransferState::Downloaded,
        }
    }

    async fn try_lock_write(&self) -> result::Result<OpWriteGuard, Timeout> {
        self.server().try_lock_write_key(self.id()).await
    }

    async fn try_lock_read(&self) -> result::Result<OpReadGuard, Timeout> {
        self.server().try_lock_read_key(self.id()).await
    }

    async fn with_server_state<F, R>(&self, cb: F) -> R
    where
        F: Send + FnOnce(&ServerState) -> R,
    {
        let server = self.server();
        let state = server.inner.state.read().await;
        cb(state.servers.get(server.id()).unwrap())
    }

    async fn with_state<F, R>(&self, cb: F) -> R
    where
        F: Send + FnOnce(&VideoState) -> R,
    {
        self.with_server_state(|ss| cb(ss.videos.get(self.id()).unwrap()))
            .await
    }

    #[allow(unused)]
    async fn update_state<F>(&self, cb: F) -> Result
    where
        F: Send + FnOnce(&mut VideoState),
    {
        let server = self.server();
        let mut state = server.inner.state.write().await;
        let server_state = state.servers.get_mut(server.id()).unwrap();
        cb(server_state.videos.get_mut(self.id()).unwrap());
        server.inner.persist_state(&state).await
    }

    pub async fn file(&self) -> result::Result<Option<LockedFile>, Timeout> {
        let guard = self.try_lock_read().await?;

        Ok(self
            .download_state()
            .await
            .file(guard, &self.server().inner.path)
            .await)
    }

    #[instrument(level = "trace", skip(self, plex_server), fields(video=self.id()))]
    pub(crate) async fn verify_download(
        &self,
        plex_server: &PlexServer,
        allow_video_deletion: bool,
    ) -> Result {
        let Ok(guard) = self.try_lock_write().await else {
            return Ok(());
        };

        let mut download_state = self.download_state().await;

        download_state
            .verify(
                &guard,
                plex_server,
                self,
                &self.server().inner.path,
                allow_video_deletion,
            )
            .await;

        self.update_state(|state| state.download = download_state)
            .await
    }

    pub async fn recover_download(&self) -> Result {
        let title = self.with_state(|vs| vs.title.clone()).await;
        let mut expected_size = 0;

        for part in self.parts().await {
            expected_size += part.remote_size().await;
        }

        for container in [
            ContainerFormat::Avi,
            ContainerFormat::Mpeg,
            ContainerFormat::MpegTs,
            ContainerFormat::M4v,
            ContainerFormat::Mp4,
            ContainerFormat::Mkv,
        ] {
            let path = self
                .file_path(FileType::Video, &container.to_string())
                .await
                .unwrap();
            let target = self.server().inner.path.join(&path);

            if let Ok(stats) = metadata(target).await
                && stats.is_file()
            {
                info!(path=?path.display(), "Recovered download for {title}");

                let download_state = if stats.size() == expected_size {
                    DownloadState::Downloaded { path }
                } else {
                    DownloadState::Transcoded { path }
                };

                return self
                    .update_state(|state| state.download = download_state)
                    .await;
            }
        }

        Ok(())
    }

    pub async fn is_downloaded(&self) -> bool {
        let download_state = self.download_state().await;
        !download_state.needs_download()
    }

    async fn transcode_profile(&self) -> VideoTranscodeOptions {
        let profile = self.with_state(|vs| vs.transcode_profile.clone()).await;

        let server_profile = self.server().transcode_profile().await;

        self.server()
            .inner
            .transcode_options(&profile.unwrap_or(server_profile))
            .await
    }

    #[instrument(level = "trace", skip(self, queue, guard, path, progress), fields(video=self.id()))]
    async fn download_media<P: Progress>(
        &self,
        queue: &DownloadQueue,
        queue_id: u32,
        guard: &OpWriteGuard,
        path: &Path,
        progress: &mut P,
    ) -> Result {
        let target = self.server().inner.path.join(path);
        let offset = match metadata(&target).await {
            Ok(stats) => stats.len(),
            Err(e) => {
                if e.kind() == ErrorKind::NotFound {
                    0
                } else {
                    return Err(e.into());
                }
            }
        };

        let item = queue.item(queue_id).await?;

        if let Some(parent) = target.parent() {
            create_dir_all(parent).await?;
        }

        let file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(&target)
            .await?;

        if let Ok(Some(len)) = item.len().await {
            progress.length(len);
        }

        let writer = WriterProgress {
            offset,
            writer: AsyncWriteAdapter::new(BufWriter::new(file)),
            progress,
            access_permit: &mut None,
        };
        info!(path=?path, offset, "Downloading source file");

        item.download(writer, offset..).await?;

        info!(path=?path, "Download complete");

        let new_state = if item.is_transcode() {
            DownloadState::Transcoded {
                path: path.to_owned(),
            }
        } else {
            DownloadState::Downloaded {
                path: path.to_owned(),
            }
        };

        if let Err(e) = item.delete().await {
            warn!(error=?e, "Failed to delete transcode session");
        }

        self.update_state(|state| {
            state.download = new_state.clone();
        })
        .await?;

        if let Err(e) = new_state
            .strip_metadata(guard, &self.server().inner.path)
            .await
        {
            warn!(path=?path, error=%e, "Failed to strip metadata");
        }

        Ok(())
    }

    async fn queue_download(&self, plex_server: &PlexServer, queue: &DownloadQueue) -> Result {
        let item = plex_server.item_by_id(self.id()).await?;
        let video = match item {
            Item::Movie(m) => library::Video::Movie(m),
            Item::Episode(e) => library::Video::Episode(e),
            _ => panic!("Unexpected item type"),
        };

        let options = self.transcode_profile().await;

        let queue_item = video.queue_download(queue, options).await?;

        self.update_state(|state| {
            state.download = DownloadState::Transcoding {
                queue_id: queue_item.id(),
            };
        })
        .await?;

        Ok(())
    }

    #[instrument(level = "trace", skip(self, queue, queue_id, progress), fields(video=self.id()))]
    async fn wait_for_available<D: DownloadProgress>(
        &self,
        queue: &DownloadQueue,
        queue_id: u32,
        progress: D,
    ) -> Result {
        let mut queue_item = queue.item(queue_id).await?;

        loop {
            match queue_item.status() {
                QueueItemStatus::Available => break,
                QueueItemStatus::Waiting | QueueItemStatus::Deciding => {
                    sleep(Duration::from_millis(500)).await;
                    queue_item.update().await?;
                }
                QueueItemStatus::Processing => {
                    let mut progress = progress.transcode_started(self).await;

                    loop {
                        match queue_item.status() {
                            QueueItemStatus::Waiting | QueueItemStatus::Deciding => {
                                // This shouldn't happen.
                                progress.failed(anyhow!(
                                    "Transcode session unexpectedly went back to waiting/deciding"
                                ));
                                break;
                            }
                            QueueItemStatus::Available => {
                                progress.finished();
                                break;
                            }
                            QueueItemStatus::Processing => {
                                let delay = if let Some(stats) = queue_item.stats() {
                                    progress.progress(stats.progress as u64);
                                    if let Some(remaining) = stats.remaining {
                                        remaining.clamp(1, 5) as u64
                                    } else {
                                        5
                                    }
                                } else {
                                    5
                                };

                                sleep(Duration::from_secs(delay)).await;
                                queue_item.update().await?;
                            }
                            QueueItemStatus::Expired | QueueItemStatus::Error => {
                                warn!("Transcode failed or expired");
                                progress.failed(anyhow!("Transcode failed or expired"));

                                self.update_state(|state| {
                                    state.download = DownloadState::None;
                                })
                                .await?;

                                return Ok(());
                            }
                        }
                    }
                }
                QueueItemStatus::Expired | QueueItemStatus::Error => {
                    self.update_state(|state| {
                        state.download = DownloadState::None;
                    })
                    .await?;

                    bail!("Transcode failed or expired");
                }
            }
        }

        let container = match queue_item.container().await? {
            Some(c) => c,
            None => {
                let options = self.transcode_profile().await;
                let container = options
                    .containers
                    .first()
                    .cloned()
                    .unwrap_or(ContainerFormat::Mp4);

                warn!(
                    ?container,
                    "No container specified by Plex. Guessing based on transcode profile"
                );
                container
            }
        };

        let path = self
            .file_path(FileType::Video, &container.to_string())
            .await
            .unwrap();

        self.update_state(|state| {
            state.download = DownloadState::Downloading { queue_id, path };
        })
        .await
    }

    #[instrument(level = "trace", skip_all, fields(video=self.id()))]
    pub(crate) async fn download<D: DownloadProgress>(
        self,
        plex_server: PlexServer,
        progress: D,
    ) -> bool {
        let Ok(guard) = self.try_lock_write().await else {
            progress
                .download_failed(&self, anyhow!("Failed to lock item for writing"))
                .await;
            return false;
        };

        let mut download_state = self.download_state().await;
        download_state
            .verify(
                &guard,
                &plex_server,
                &self,
                &self.server().inner.path,
                false,
            )
            .await;

        if let Err(e) = self
            .update_state(|state| state.download = download_state.clone())
            .await
        {
            progress.download_failed(&self, e).await;
            return false;
        }

        let queue = match plex_server.download_queue().await {
            Ok(q) => q,
            Err(e) => {
                warn!(error=?e);
                progress.download_failed(&self, e.into()).await;
                return false;
            }
        };

        if download_state == DownloadState::None
            && let Err(e) = self.queue_download(&plex_server, &queue).await
        {
            warn!(error=?e);
            progress.download_failed(&self, e).await;
            return false;
        }

        loop {
            download_state = self.download_state().await;

            match download_state {
                DownloadState::None => {
                    return false;
                }
                DownloadState::Transcoding { queue_id } => {
                    if let Err(e) = self
                        .wait_for_available(&queue, queue_id, progress.clone())
                        .await
                    {
                        warn!(error=?e);
                        progress.download_failed(&self, e).await;
                        return false;
                    }
                }
                DownloadState::Downloading { queue_id, path } => {
                    let _permit = self
                        .server()
                        .inner
                        .download_permits
                        .clone()
                        .acquire_owned()
                        .await
                        .unwrap();

                    let mut progress = progress.download_started(&self).await;

                    if let Err(e) = self
                        .download_media(&queue, queue_id, &guard, &path, &mut progress)
                        .await
                    {
                        warn!(error=?e);
                        progress.failed(e);
                        return false;
                    }

                    progress.finished();
                }
                DownloadState::Downloaded { .. } | DownloadState::Transcoded { .. } => return true,
            }
        }
    }

    async fn download_state(&self) -> DownloadState {
        self.with_state(|part_state| part_state.download.clone())
            .await
    }

    pub(crate) async fn strip_metadata(&self) {
        let Ok(guard) = self.try_lock_write().await else {
            return;
        };

        let state = self.download_state().await;

        if let Err(e) = state
            .strip_metadata(&guard, &self.server().inner.path)
            .await
        {
            warn!(error=%e, "Unable to strip metadata from video file");
        }
    }
}

#[derive(Clone)]
pub struct Playlist {
    server: Server,
    id: String,
}

impl fmt::Debug for Playlist {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(&format!("Playlist({})", self.id))
    }
}

state_wrapper!(Playlist, PlaylistState, playlists);
wrapper_builders!(Playlist, PlaylistState);

impl Playlist {
    thumbnail_methods!();

    pub async fn videos(&self) -> Vec<Video> {
        self.with_server_state(|ss| {
            let ps = ss.playlists.get(&self.id).unwrap();
            ps.videos
                .iter()
                .map(|id| Video::wrap(&self.server, ss.videos.get(id).unwrap()))
                .collect()
        })
        .await
    }

    pub(crate) async fn file_path(&self, file_type: FileType, extension: &str) -> Option<PathBuf> {
        let output_style = self.server.inner.output_style().await;

        self.with_state(|state| {
            let name = match file_type {
                FileType::Thumbnail => {
                    if output_style == OutputStyle::Standardized {
                        format!("{}.{extension}", state.title)
                    } else {
                        format!("{}.{extension}", self.id)
                    }
                }
                FileType::Playlist => {
                    if output_style != OutputStyle::Standardized {
                        return None;
                    }

                    format!("{}.{extension}", state.title)
                }
                _ => return None,
            };

            let mut root = PathBuf::from(safe(&self.server.id));

            root = if output_style == OutputStyle::Standardized {
                root.join("Playlists")
            } else {
                root.join(METADATA_DIR)
            };

            Some(root.join(safe(&name)))
        })
        .await
    }

    pub(crate) async fn write_playlist(&self) -> Result {
        let Some(playlist_path) = self.file_path(FileType::Playlist, "m3u").await else {
            return Ok(());
        };

        write_playlist(&self.server.inner.path, &playlist_path, self.videos().await).await
    }
}

#[derive(Clone)]
pub struct MovieCollection {
    server: Server,
    id: String,
}

impl fmt::Debug for MovieCollection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(&format!("MovieCollection({})", self.id))
    }
}

state_wrapper!(MovieCollection, CollectionState, collections);
wrapper_builders!(MovieCollection, CollectionState);

impl MovieCollection {
    thumbnail_methods!();
    parent!(library, MovieLibrary, library);

    pub async fn contains(&self, movie: &Movie) -> bool {
        let id = movie.id();

        self.with_state(|cs| cs.contents.iter().any(|i| i == id))
            .await
    }

    pub async fn movies(&self) -> Vec<Movie> {
        self.with_state(|cs| {
            cs.contents
                .iter()
                .map(|id| Movie::wrap_from_id(&self.server, id))
                .collect()
        })
        .await
    }

    pub async fn videos(&self) -> Vec<Video> {
        self.movies().await.into_iter().map(Video::Movie).collect()
    }

    pub(crate) async fn file_path(&self, file_type: FileType, extension: &str) -> Option<PathBuf> {
        let output_style = self.server.inner.output_style().await;

        self.with_server_state(|ss| {
            let state = ss.collections.get(&self.id).unwrap();

            let name = match file_type {
                FileType::Thumbnail => {
                    if output_style == OutputStyle::Standardized {
                        format!("{}.{extension}", state.title)
                    } else {
                        format!("{}.{extension}", self.id)
                    }
                }
                FileType::Playlist => {
                    if output_style != OutputStyle::Standardized {
                        return None;
                    }
                    format!("{}.{extension}", state.title)
                }
                _ => return None,
            };

            let mut root = PathBuf::from(safe(&self.server.id));

            root = if output_style == OutputStyle::Standardized {
                let library_title = &ss.libraries.get(&state.library).unwrap().title;

                root.join(safe(library_title)).join("Collections")
            } else {
                root.join(METADATA_DIR)
            };

            Some(root.join(safe(&name)))
        })
        .await
    }

    pub(crate) async fn write_playlist(&self) -> Result {
        let Some(playlist_path) = self.file_path(FileType::Playlist, "m3u").await else {
            return Ok(());
        };

        write_playlist(&self.server.inner.path, &playlist_path, self.videos().await).await
    }
}

#[derive(Clone)]
pub struct ShowCollection {
    server: Server,
    id: String,
}

impl fmt::Debug for ShowCollection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(&format!("ShowCollection({})", self.id))
    }
}

state_wrapper!(ShowCollection, CollectionState, collections);
wrapper_builders!(ShowCollection, CollectionState);

impl ShowCollection {
    thumbnail_methods!();
    parent!(library, ShowLibrary, library);

    pub async fn shows(&self) -> Vec<Show> {
        self.with_state(|cs| {
            cs.contents
                .iter()
                .map(|id| Show::wrap_from_id(&self.server, id))
                .collect()
        })
        .await
    }

    pub(crate) async fn file_path(&self, file_type: FileType, extension: &str) -> Option<PathBuf> {
        let output_style = self.server.inner.output_style().await;

        self.with_server_state(|ss| {
            let state = ss.collections.get(&self.id).unwrap();

            let name = match file_type {
                FileType::Thumbnail => {
                    if output_style == OutputStyle::Standardized {
                        format!("{}.{extension}", state.title)
                    } else {
                        format!("{}.{extension}", self.id)
                    }
                }
                FileType::Playlist => {
                    if output_style != OutputStyle::Standardized {
                        return None;
                    }
                    format!("{}.{extension}", state.title)
                }
                _ => return None,
            };

            let mut root = PathBuf::from(safe(&self.server.id));

            root = if output_style == OutputStyle::Standardized {
                let library_title = &ss.libraries.get(&state.library).unwrap().title;

                root.join(safe(library_title)).join("Collections")
            } else {
                root.join(METADATA_DIR)
            };

            Some(root.join(safe(&name)))
        })
        .await
    }

    pub(crate) async fn write_playlist(&self) -> Result {
        let Some(playlist_path) = self.file_path(FileType::Playlist, "m3u").await else {
            return Ok(());
        };

        let mut videos: Vec<Video> = Vec::new();

        for show in self.shows().await {
            for season in show.seasons().await {
                for episode in season.episodes().await {
                    videos.push(Video::Episode(episode));
                }
            }
        }

        write_playlist(&self.server.inner.path, &playlist_path, videos).await
    }
}

#[derive(Clone)]
pub enum Collection {
    Movie(MovieCollection),
    Show(ShowCollection),
}

impl Collection {
    pub fn id(&self) -> &str {
        match self {
            Self::Movie(c) => c.id(),
            Self::Show(c) => c.id(),
        }
    }

    pub fn server(&self) -> Server {
        match self {
            Self::Movie(c) => c.server(),
            Self::Show(c) => c.server(),
        }
    }

    pub async fn library(&self) -> Library {
        match self {
            Self::Movie(c) => Library::Movie(c.library().await),
            Self::Show(c) => Library::Show(c.library().await),
        }
    }

    pub async fn title(&self) -> String {
        match self {
            Self::Movie(c) => c.title().await,
            Self::Show(c) => c.title().await,
        }
    }

    pub async fn thumbnail(&self) -> result::Result<Option<LockedFile>, Timeout> {
        match self {
            Self::Movie(c) => c.thumbnail().await,
            Self::Show(c) => c.thumbnail().await,
        }
    }

    pub async fn update_thumbnail(&self, rebuild: bool) -> Result {
        match self {
            Self::Movie(c) => c.update_thumbnail(rebuild).await,
            Self::Show(c) => c.update_thumbnail(rebuild).await,
        }
    }

    pub(crate) async fn file_path(&self, file_type: FileType, extension: &str) -> Option<PathBuf> {
        match self {
            Self::Movie(c) => c.file_path(file_type, extension).await,
            Self::Show(c) => c.file_path(file_type, extension).await,
        }
    }

    pub(crate) async fn write_playlist(&self) -> Result {
        match self {
            Self::Movie(c) => c.write_playlist().await,
            Self::Show(c) => c.write_playlist().await,
        }
    }
}

#[derive(Clone)]
pub struct MovieLibrary {
    server: Server,
    id: String,
}

impl fmt::Debug for MovieLibrary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(&format!("MovieLibrary({})", self.id))
    }
}

state_wrapper!(MovieLibrary, LibraryState, libraries);
wrapper_builders!(MovieLibrary, LibraryState);

impl MovieLibrary {
    children!(collections, collections, MovieCollection, library);

    pub async fn movies(&self) -> Vec<Movie> {
        self.with_server_state(|ss| {
            ss.videos
                .values()
                .filter_map(|vs| {
                    if let VideoDetail::Movie(ref detail) = vs.detail {
                        if detail.library == self.id {
                            Some(Movie::wrap(&self.server, vs))
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
    server: Server,
    id: String,
}

impl fmt::Debug for ShowLibrary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(&format!("ShowLibrary({})", self.id))
    }
}

state_wrapper!(ShowLibrary, LibraryState, libraries);
wrapper_builders!(ShowLibrary, LibraryState);

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
    pub fn id(&self) -> &str {
        match self {
            Self::Movie(l) => &l.id,
            Self::Show(l) => &l.id,
        }
    }

    pub fn server(&self) -> Server {
        match self {
            Self::Movie(l) => l.server(),
            Self::Show(l) => l.server(),
        }
    }

    pub async fn title(&self) -> String {
        match self {
            Self::Movie(l) => l.title().await,
            Self::Show(l) => l.title().await,
        }
    }

    pub(crate) fn wrap(server: &Server, state: &LibraryState) -> Self {
        match state.library_type {
            LibraryType::Movie => Self::Movie(MovieLibrary::wrap(server, state)),
            LibraryType::Show => Self::Show(ShowLibrary::wrap(server, state)),
        }
    }

    pub fn library_type(&self) -> LibraryType {
        match self {
            Self::Movie(_) => LibraryType::Movie,
            Self::Show(_) => LibraryType::Show,
        }
    }

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
