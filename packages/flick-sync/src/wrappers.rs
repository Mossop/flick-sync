use std::{
    fmt,
    io::{ErrorKind, IoSlice},
    ops::{Add, AddAssign},
    path::{Path, PathBuf},
    pin::Pin,
    result,
    task::{Context, Poll},
    time::Duration,
};

use futures::io::AsyncWrite;
use pathdiff::diff_paths;
use pin_project::pin_project;
use plex_api::{
    library::{self, Item, MediaItem, MetadataItem},
    media_container::server::library::ContainerFormat,
    transcode::TranscodeStatus,
};
use tokio::{
    fs::{File, OpenOptions, create_dir_all, metadata, remove_file},
    io::{AsyncWriteExt, BufWriter},
    time::sleep,
};
use tracing::{debug, error, info, instrument, trace, warn};
use xml::{EmitterConfig, writer::XmlEvent};

use crate::{
    Error, FlickSync, Result, Server,
    state::{
        CollectionState, DownloadState, LibraryState, LibraryType, PlaylistState, RelatedFileState,
        SeasonState, ServerState, ShowState, VideoDetail, VideoPartState, VideoState,
    },
    util::{AsyncWriteAdapter, safe},
};

type EventWriter = xml::writer::EventWriter<std::fs::File>;

#[derive(Debug, Clone, Copy)]
pub(crate) enum FileType {
    Video(usize),
    Thumbnail,
    Metadata,
    Playlist,
}

trait StateWrapper<S> {
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
        }

        impl StateWrapper<$st_typ> for $typ {
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
        pub async fn thumbnail_file(&self) -> Option<PathBuf> {
            if let Some(relative_file) = self.thumbnail().await.file() {
                Some(self.server.inner.path.read().await.join(relative_file))
            } else {
                None
            }
        }

        pub(crate) async fn thumbnail(&self) -> RelatedFileState {
            self.with_state(|s| s.thumbnail.clone()).await
        }

        #[instrument(level = "trace")]
        pub async fn update_thumbnail(&self, rebuild: bool) -> Result {
            let root = self.server.inner.path.read().await.to_owned();

            let thumbnail_path = self.file_path(FileType::Thumbnail, "jpg").await;

            let mut thumbnail = self.thumbnail().await;
            if rebuild {
                thumbnail.delete(&root).await;
            } else {
                thumbnail.verify(&root, &thumbnail_path).await;
            }

            self.update_state(|s| s.thumbnail = thumbnail.clone())
                .await?;

            if thumbnail.is_none() {
                let server = self.server.connect().await?;
                let item = server.item_by_id(&self.id).await?;
                debug!("Updating thumbnail for {}", item.title());

                let image = if let Some(ref thumb) = item.metadata().thumb {
                    thumb.clone()
                } else {
                    warn!("No thumbnail found for {}", item.title());
                    return Ok(());
                };

                let target = root.join(&thumbnail_path);

                if let Some(parent) = target.parent() {
                    create_dir_all(parent).await?;
                }

                let file = AsyncWriteAdapter::new(File::create(&target).await?);
                server
                    .transcode_artwork(&image, 320, 320, Default::default(), file)
                    .await?;

                let state = RelatedFileState::Stored {
                    path: thumbnail_path,
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
        pub(crate) async fn metadata(&self) -> RelatedFileState {
            self.with_state(|s| s.metadata.clone()).await
        }

        #[instrument(level = "trace")]
        pub async fn update_metadata(&self, rebuild: bool) -> Result {
            let root = self.server.inner.path.read().await;

            let metadata_path = self.file_path(FileType::Metadata, "nfo").await;

            let mut metadata = self.metadata().await;
            if rebuild {
                metadata.delete(&root).await;
            } else {
                metadata.verify(&root, &metadata_path).await;
            }

            self.update_state(|s| s.metadata = metadata.clone()).await?;

            if metadata.is_none() {
                let target = root.join(&metadata_path);

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

    let output = File::create(&target).await?;
    let mut writer = BufWriter::new(output);

    for video in videos {
        for part in video.parts().await {
            let download = part.download_state().await;

            if let Some(video_path) = download.file() {
                if !download.needs_download() {
                    if let Some(relative) = diff_paths(root.join(video_path), parent) {
                        writer
                            .write_all(relative.as_os_str().as_encoded_bytes())
                            .await?;
                        writer.write_all(b"\n").await?;
                    }
                }
            }
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

    pub async fn write_metadata(&self, writer: &mut EventWriter) -> Result {
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

    pub(crate) async fn file_path(&self, file_type: FileType, extension: &str) -> PathBuf {
        self.with_server_state(|ss| {
            let state = ss.shows.get(&self.id).unwrap();

            let name = match file_type {
                FileType::Video(_) | FileType::Playlist => {
                    unreachable!("Invalid file type for Show")
                }
                FileType::Thumbnail => format!("dvdcover.{extension}"),
                FileType::Metadata => format!("tvshow.{extension}"),
            };

            let library_title = &ss.libraries.get(&state.library).unwrap().title;
            PathBuf::from(safe(&self.server.id))
                .join(safe(library_title))
                .join(safe(format!("{} ({})", state.title, state.year)))
                .join(safe(name))
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

    pub async fn episodes(&self) -> Vec<Episode> {
        self.with_server_state(|ss| {
            let mut episode_states: Vec<(u32, &VideoState)> = ss
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

pub trait Progress {
    fn progress(&mut self, position: u64, size: u64);
}

#[pin_project]
struct WriterProgress<'a, W, P> {
    offset: u64,
    size: u64,
    #[pin]
    writer: W,
    progress: &'a mut P,
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
            this.progress.progress(*this.offset, *this.size);
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
            *this.offset += count as u64;
            this.progress.progress(*this.offset, *this.size);
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

    pub async fn transfer_state(&self) -> TransferState {
        let download_state = self.download_state().await;

        match download_state {
            DownloadState::None => TransferState::Waiting,
            DownloadState::Downloading { .. } => TransferState::Downloading,
            DownloadState::Transcoding { .. } => TransferState::Transcoding,
            _ => TransferState::Downloaded,
        }
    }

    pub async fn file(&self) -> Option<PathBuf> {
        let file = self.download_state().await.file()?;

        Some(self.server.inner.path.read().await.join(file))
    }

    #[instrument(level = "trace", skip(self), fields(video=self.id, part=self.index))]
    pub async fn verify_download(&self) -> Result {
        let server = self.server.connect().await?;
        let mut download_state = self.download_state().await;
        let root = self.server.inner.path.read().await.clone();

        let expected_path = if let Some(extension) = download_state.file().and_then(|fp| {
            fp.extension()
                .and_then(|os| os.to_str())
                .map(|s| s.to_owned())
        }) {
            Some(self.file_path(&extension).await)
        } else {
            None
        };

        download_state
            .verify(&server, &root, expected_path.as_deref())
            .await;

        self.update_state(|state| state.download = download_state)
            .await
    }

    pub async fn video(&self) -> Video {
        self.with_video_state(|video_state| Video::wrap(&self.server, video_state))
            .await
    }

    pub(crate) async fn file_path(&self, extension: &str) -> PathBuf {
        let video = self.video().await;
        video
            .file_path(FileType::Video(self.index), extension)
            .await
    }

    pub async fn rebuild_download(&self) -> Result {
        let root = self.server.inner.path.read().await;
        let title = self.with_video_state(|vs| vs.title.clone()).await;

        for container in [
            ContainerFormat::Avi,
            ContainerFormat::Mpeg,
            ContainerFormat::MpegTs,
            ContainerFormat::M4v,
            ContainerFormat::Mp4,
            ContainerFormat::Mkv,
        ] {
            let path = self.file_path(&container.to_string()).await;
            let target = root.join(&path);

            if let Ok(stats) = metadata(target).await {
                if stats.is_file() {
                    info!(path=?path.display(), "Recovered download for {title}");

                    return self
                        .update_state(|state| state.download = DownloadState::Downloaded { path })
                        .await;
                }
            }
        }

        Ok(())
    }

    pub async fn is_downloaded(&self) -> bool {
        let download_state = self.download_state().await;
        !download_state.needs_download()
    }

    #[instrument(level = "trace", skip(self), fields(session_id, video=self.id, part=self.index))]
    async fn start_transcode(&self) -> Result {
        let (media_id, profile) = self
            .with_video_state(|vs| (vs.media_id.clone(), vs.transcode_profile.clone()))
            .await;

        let server_profile = self.server.transcode_profile().await;

        let options = if let Some(options) = self
            .server
            .inner
            .transcode_options(profile.or(server_profile))
            .await
        {
            options
        } else {
            return Err(Error::TranscodeSkipped);
        };

        let _permit = self.server.transcode_permit().await;

        let server = self.server.connect().await?;
        let item = server.item_by_id(&self.id).await?;

        let video = match item {
            Item::Movie(m) => library::Video::Movie(m),
            Item::Episode(e) => library::Video::Episode(e),
            _ => panic!("Unexpected item type"),
        };

        let media = video
            .media()
            .into_iter()
            .find(|m| m.metadata().id.as_ref() == Some(&media_id))
            .ok_or_else(|| Error::MissingItem)?;
        let parts = media.parts();
        let part = parts.get(self.index).ok_or_else(|| Error::MissingItem)?;

        trace!("Attempting transcode");

        let session = part.create_download_session(options).await?;

        tracing::Span::current().record("session_id", session.session_id());

        // Wait until the transcode session has started.
        let mut count = 0;
        loop {
            match session.stats().await {
                Ok(_) => {
                    if count > 0 {
                        trace!("Saw transcode session after {count} delays");
                    }
                    break;
                }
                Err(plex_api::Error::ItemNotFound) => {
                    count += 1;
                    if count > 20 {
                        error!("Transcode session failed to start");
                        return Err(Error::TranscodeFailed);
                    }
                }
                Err(e) => return Err(e.into()),
            }

            sleep(Duration::from_millis(100)).await;
        }

        debug!("Started transcode session");

        let path = self.file_path(&session.container().to_string()).await;

        if let Err(e) = self
            .update_state(|state| {
                state.download = DownloadState::Transcoding {
                    session_id: session.session_id().to_string(),
                    path,
                }
            })
            .await
        {
            warn!("Failed to store download state, abandoning transcode.");
            if let Err(e) = session.cancel().await {
                error!(error=?e, "Failed to cancel transcode.");
            }

            return Err(e);
        }

        Ok(())
    }

    #[instrument(level = "trace", skip(self), fields(video=self.id, part=self.index))]
    async fn enter_downloading_state(&self) -> Result {
        let server = self.server.connect().await?;
        let item = server.item_by_id(&self.id).await?;

        let media_id = self
            .with_server_state(|ss| {
                let video_state = ss.videos.get(&self.id).unwrap();
                video_state.media_id.clone()
            })
            .await;

        let media = item
            .media()
            .into_iter()
            .find(|m| m.metadata().id.as_ref() == Some(&media_id))
            .ok_or_else(|| Error::MissingItem)?;
        let parts = media.parts();
        let part = parts.get(self.index).ok_or_else(|| Error::MissingItem)?;

        let path = self
            .file_path(&part.metadata().container.unwrap().to_string())
            .await;

        let target = { self.server.inner.path.read().await.join(&path) };
        if let Err(e) = remove_file(target).await {
            if e.kind() != ErrorKind::NotFound {
                return Err(Error::from(e));
            }
        }

        self.update_state(|state| state.download = DownloadState::Downloading { path })
            .await?;

        Ok(())
    }

    #[instrument(level = "trace", skip(self, progress), fields(video=self.id, part=self.index))]
    async fn wait_for_transcode_to_complete<P: Progress + Unpin>(
        &self,
        session_id: &str,
        progress: &mut P,
    ) -> Result {
        let server = self.server.connect().await?;
        let session = match server.transcode_session(session_id).await {
            Ok(session) => session,
            Err(plex_api::Error::ItemNotFound) => {
                warn!("Server dropped transcode session");
                self.update_state(|state| state.download = DownloadState::None)
                    .await?;

                return Err(Error::TranscodeLost);
            }
            Err(e) => {
                error!(error=?e, "Error getting transcode status");
                return Err(e.into());
            }
        };

        loop {
            match session.status().await {
                Ok(TranscodeStatus::Complete) => {
                    progress.progress(100, 100);
                    break;
                }
                Ok(TranscodeStatus::Error) => {
                    let _ = session.cancel().await;
                    return Err(Error::TranscodeFailed);
                }
                Ok(TranscodeStatus::Transcoding {
                    remaining,
                    progress: p,
                }) => {
                    progress.progress(p as u64, 100);
                    let delay = if let Some(remaining) = remaining {
                        remaining.clamp(2, 5)
                    } else {
                        5
                    };

                    sleep(Duration::from_secs(delay.into())).await;
                }
                Err(plex_api::Error::ItemNotFound) => {
                    warn!("Server dropped transcode session");
                    self.update_state(|state| state.download = DownloadState::None)
                        .await?;

                    return Err(Error::TranscodeLost);
                }
                Err(e) => {
                    error!(error=?e, "Error getting transcode status");
                    return Err(e.into());
                }
            }
        }

        Ok(())
    }

    #[instrument(level = "trace", fields(video=self.id, part=self.index))]
    pub async fn negotiate_transfer_type(&self) -> Result {
        let mut download_state = self.download_state().await;

        if matches!(download_state, DownloadState::Transcoding { .. }) {
            let root = self.server.inner.path.read().await;
            download_state
                .verify(&self.server.connect().await?, &root, None)
                .await;

            self.update_state(|state| state.download = download_state.clone())
                .await?;
        }

        if matches!(download_state, DownloadState::None) {
            match self.start_transcode().await {
                Err(Error::TranscodeSkipped) => (),
                Err(Error::PlexError {
                    source: plex_api::Error::TranscodeRefused,
                }) => debug!("Transcode attempt refused"),
                Err(e) => return Err(e),
                Ok(_) => {
                    return Ok(());
                }
            }

            self.enter_downloading_state().await?;
        }

        Ok(())
    }

    #[instrument(level = "trace", skip(progress), fields(video=self.id, part=self.index))]
    pub async fn wait_for_download_to_be_available<P: Progress + Unpin>(
        &self,
        mut progress: P,
    ) -> Result {
        loop {
            self.negotiate_transfer_type().await?;

            let download_state = self.download_state().await;
            if let DownloadState::Transcoding { session_id, .. } = download_state {
                match self
                    .wait_for_transcode_to_complete(&session_id, &mut progress)
                    .await
                {
                    Err(Error::TranscodeLost) => continue,
                    r => return r,
                }
            } else {
                return Ok(());
            }
        }
    }

    #[instrument(level = "trace", skip(self, path, progress), fields(video=self.id, part=self.index))]
    async fn download_direct<P: Progress + Unpin>(&self, path: &Path, mut progress: P) -> Result {
        let root = self.server.inner.path.read().await;
        let target = root.join(path);
        let offset = match metadata(&target).await {
            Ok(stats) => stats.len(),
            Err(e) => {
                if e.kind() == ErrorKind::NotFound {
                    0
                } else {
                    return Err(Error::from(e));
                }
            }
        };

        let server = self.server.connect().await?;
        let item = server.item_by_id(&self.id).await?;

        let media_id = self
            .with_server_state(|ss| {
                let video_state = ss.videos.get(&self.id).unwrap();
                video_state.media_id.clone()
            })
            .await;

        let media = item
            .media()
            .into_iter()
            .find(|m| m.metadata().id.as_ref() == Some(&media_id))
            .ok_or_else(|| Error::MissingItem)?;
        let parts = media.parts();
        let part = parts.get(self.index).ok_or_else(|| Error::MissingItem)?;

        if let Some(parent) = target.parent() {
            create_dir_all(parent).await?;
        }

        let file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(&target)
            .await?;

        let writer = WriterProgress {
            offset,
            size: part.metadata().size.unwrap(),
            writer: AsyncWriteAdapter::new(file),
            progress: &mut progress,
        };
        info!(path=?path, offset, "Downloading source file");

        part.download(writer, offset..).await?;
        info!(path=?path, "Download complete");

        let new_state = DownloadState::Downloaded {
            path: path.to_owned(),
        };

        self.update_state(|state| {
            state.download = new_state.clone();
        })
        .await?;

        if let Err(e) = new_state.strip_metadata(&root).await {
            warn!(path=?path, error=%e, "Failed to strip metadata");
        }

        Ok(())
    }

    #[instrument(level = "trace", skip(self, path, progress), fields(video=self.id, part=self.index))]
    async fn download_transcode<P: Progress + Unpin>(
        &self,
        session_id: &str,
        path: &Path,
        mut progress: P,
    ) -> Result {
        let server = self.server.connect().await?;
        let session = server.transcode_session(session_id).await?;
        let status = session.status().await?;
        let stats = session.stats().await?;

        if !matches!(status, TranscodeStatus::Complete) {
            return Err(Error::DownloadUnavailable);
        }

        let root = self.server.inner.path.read().await;
        let target = root.join(path);

        if let Some(parent) = target.parent() {
            create_dir_all(parent).await?;
        }

        let file = OpenOptions::new()
            .write(true)
            .truncate(true)
            .create(true)
            .open(&target)
            .await?;

        let writer = WriterProgress {
            offset: 0,
            size: stats.size as u64,
            writer: AsyncWriteAdapter::new(file),
            progress: &mut progress,
        };
        info!(path=?path, "Downloading transcoded video");

        session.download(writer).await?;
        info!(path=?path, "Download complete");

        let new_state = DownloadState::Downloaded {
            path: path.to_owned(),
        };

        self.update_state(|state| {
            state.download = new_state.clone();
        })
        .await?;

        if let Err(e) = session.cancel().await {
            warn!(
                error=?e,
                "Transcode session failed to cancel"
            );
        }

        if let Err(e) = new_state.strip_metadata(&root).await {
            warn!(path=?path, error=%e, "Failed to strip metadata");
        }

        Ok(())
    }

    pub async fn download<P: Progress + Unpin>(&self, progress: P) -> Result {
        let download_state = self.download_state().await;
        match download_state {
            DownloadState::None => Err(Error::DownloadUnavailable),
            DownloadState::Downloading { path } => self.download_direct(&path, progress).await,
            DownloadState::Transcoding { session_id, path } => {
                self.download_transcode(&session_id, &path, progress).await
            }
            DownloadState::Downloaded { .. } | DownloadState::Transcoded { .. } => Ok(()),
        }
    }

    async fn download_state(&self) -> DownloadState {
        self.with_state(|part_state| part_state.download.clone())
            .await
    }

    pub(crate) async fn strip_metadata(&self) {
        let root = self.server.inner.path.read().await;

        let state = self.download_state().await;

        if let Err(e) = state.strip_metadata(&root).await {
            warn!(error=%e, "Unable to strip metadata from video file");
        }
    }
}

impl StateWrapper<VideoPartState> for VideoPart {
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

    async fn update_state<F>(&self, cb: F) -> Result
    where
        F: Send + FnOnce(&mut VideoPartState),
    {
        let mut state = self.server.inner.state.write().await;
        let server_state = state.servers.get_mut(&self.server.id).unwrap();
        cb(server_state
            .videos
            .get_mut(&self.id)
            .unwrap()
            .parts
            .get_mut(self.index)
            .unwrap());
        self.server.inner.persist_state(&state).await
    }
}

#[derive(Clone, Copy, Default)]
pub struct VideoStats {
    pub downloaded_parts: u32,
    pub total_parts: u32,
    pub local_bytes: u64,
    pub remote_bytes: u64,
    pub remaining_bytes: u64,
    pub local_duration: Duration,
    pub remote_duration: Duration,
}

impl Add for VideoStats {
    type Output = VideoStats;

    fn add(self, rhs: VideoStats) -> VideoStats {
        Self {
            downloaded_parts: self.downloaded_parts + rhs.downloaded_parts,
            total_parts: self.total_parts + rhs.total_parts,
            local_bytes: self.local_bytes + rhs.local_bytes,
            remote_bytes: self.remote_bytes + rhs.remote_bytes,
            remaining_bytes: self.remaining_bytes + rhs.remaining_bytes,
            local_duration: self.local_duration + rhs.local_duration,
            remote_duration: self.remote_duration + rhs.remote_duration,
        }
    }
}

impl AddAssign for VideoStats {
    fn add_assign(&mut self, rhs: VideoStats) {
        self.downloaded_parts += rhs.downloaded_parts;
        self.total_parts += rhs.total_parts;
        self.local_bytes += rhs.local_bytes;
        self.remote_bytes += rhs.remote_bytes;
        self.remaining_bytes += rhs.remaining_bytes;
        self.local_duration += rhs.local_duration;
        self.remote_duration += rhs.remote_duration;
    }
}

impl VideoStats {
    async fn try_from<M: MediaItem>(item: M, parts: Vec<VideoPart>) -> Result<Self> {
        let media = item.media();
        let media = &media[0];

        let mut stats = VideoStats::default();

        for (local_part, remote_part) in parts.into_iter().zip(media.parts()) {
            stats.total_parts += 1;

            let part_duration = local_part.duration().await;
            stats.remote_duration += part_duration;
            let state = local_part.download_state().await;

            if !state.needs_download() {
                stats.local_duration += part_duration;
            }

            if let Some(path) = state.file() {
                let path = local_part.server.inner.path.read().await.join(path);
                if let Ok(file_stats) = metadata(path).await {
                    stats.local_bytes += file_stats.len();
                }
            }

            if !state.needs_download() {
                stats.downloaded_parts += 1;
                stats.remote_bytes += remote_part.metadata().size.unwrap();
            } else {
                stats.remaining_bytes += remote_part.metadata().size.unwrap();
            }
        }

        Ok(stats)
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

state_wrapper!(Episode, VideoState, videos);
wrapper_builders!(Episode, VideoState);

impl Episode {
    thumbnail_methods!();
    metadata_methods!();
    parent!(season, Season, episode_state().season);

    pub async fn write_metadata(&self, writer: &mut EventWriter) -> Result {
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

    pub async fn stats(&self) -> Result<VideoStats> {
        let server = self.server.connect().await?;
        let item = server.item_by_id(&self.id).await?;
        VideoStats::try_from(item, self.parts().await).await
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

    pub(crate) async fn file_path(&self, file_type: FileType, extension: &str) -> PathBuf {
        self.with_server_state(|ss| {
            let state = ss.videos.get(&self.id).unwrap();
            let ep_state = state.episode_state();
            let season = ss.seasons.get(&ep_state.season).unwrap();
            let show = ss.shows.get(&season.show).unwrap();
            let library_title = &ss.libraries.get(&show.library).unwrap().title;

            let name = match file_type {
                FileType::Playlist => unreachable!("Invalid file type for Episode"),
                FileType::Video(index) => {
                    let part_name = if state.parts.len() == 1 {
                        "".to_string()
                    } else {
                        format!(" - pt{}", index + 1)
                    };

                    format!(
                        "S{:02}E{:02} - {}{part_name}.{extension}",
                        season.index, ep_state.index, state.title
                    )
                }
                FileType::Thumbnail => {
                    format!(
                        "S{:02}E{:02} - {}.{extension}",
                        season.index, ep_state.index, state.title
                    )
                }
                FileType::Metadata => {
                    format!(
                        "S{:02}E{:02} - {}.{extension}",
                        season.index, ep_state.index, state.title
                    )
                }
            };

            PathBuf::from(safe(&self.server.id))
                .join(safe(library_title))
                .join(safe(format!("{} ({})", show.title, show.year)))
                .join(safe(name))
        })
        .await
    }
}

#[derive(Clone)]
pub struct Movie {
    server: Server,
    id: String,
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

    pub async fn write_metadata(&self, writer: &mut EventWriter) -> Result {
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

    pub async fn stats(&self) -> Result<VideoStats> {
        let server = self.server.connect().await?;
        let item = server.item_by_id(&self.id).await?;
        VideoStats::try_from(item, self.parts().await).await
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

    pub(crate) async fn file_path(&self, file_type: FileType, extension: &str) -> PathBuf {
        self.with_server_state(|ss| {
            let state = ss.videos.get(&self.id).unwrap();
            let m_state = state.movie_state();
            let library_title = &ss.libraries.get(&m_state.library).unwrap().title;

            let name = match file_type {
                FileType::Playlist => unreachable!("Invalid file type for Movie"),
                FileType::Video(index) => {
                    let part_name = if state.parts.len() == 1 {
                        "".to_string()
                    } else {
                        format!(" - pt{}", index + 1)
                    };

                    format!("{} ({}){part_name}.{extension}", state.title, m_state.year)
                }
                FileType::Thumbnail => {
                    if state.parts.len() == 1 {
                        format!("{} ({}).{extension}", state.title, m_state.year)
                    } else {
                        format!("movie.{extension}")
                    }
                }
                FileType::Metadata => {
                    if state.parts.len() == 1 {
                        format!("{} ({}).{extension}", state.title, m_state.year)
                    } else {
                        format!("movie.{extension}")
                    }
                }
            };

            PathBuf::from(safe(&self.server.id))
                .join(safe(library_title))
                .join(safe(format!("{} ({})", state.title, m_state.year)))
                .join(safe(name))
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
    pub(crate) fn wrap(server: &Server, state: &VideoState) -> Self {
        match state.detail {
            VideoDetail::Movie(_) => Self::Movie(Movie::wrap(server, state)),
            VideoDetail::Episode(_) => Self::Episode(Episode::wrap(server, state)),
        }
    }

    pub async fn is_downloaded(&self) -> bool {
        for part in self.parts().await {
            if !part.is_downloaded().await {
                return false;
            }
        }

        true
    }

    pub fn flick_sync(&self) -> FlickSync {
        match self {
            Self::Movie(v) => v.flick_sync(),
            Self::Episode(v) => v.flick_sync(),
        }
    }

    pub async fn thumbnail_file(&self) -> Option<PathBuf> {
        match self {
            Self::Movie(v) => v.thumbnail_file().await,
            Self::Episode(v) => v.thumbnail_file().await,
        }
    }

    pub async fn library(&self) -> Library {
        match self {
            Self::Movie(v) => Library::Movie(v.library().await),
            Self::Episode(v) => Library::Show(v.library().await),
        }
    }

    pub async fn stats(&self) -> Result<VideoStats> {
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

    pub(crate) async fn file_path(&self, file_type: FileType, extension: &str) -> PathBuf {
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

    pub(crate) async fn file_path(&self, file_type: FileType, extension: &str) -> PathBuf {
        self.with_state(|state| {
            let name = match file_type {
                FileType::Thumbnail | FileType::Playlist => format!("{}.{extension}", state.title),
                _ => unreachable!("Invalid file type for Playlist"),
            };

            PathBuf::from(safe(&self.server.id))
                .join("Playlists")
                .join(safe(&name))
        })
        .await
    }

    pub(crate) async fn write_playlist(&self) -> Result {
        let root = self.server.inner.path.read().await;
        let playlist_path = self.file_path(FileType::Playlist, "m3u").await;

        write_playlist(&root, &playlist_path, self.videos().await).await
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

    pub(crate) async fn file_path(&self, file_type: FileType, extension: &str) -> PathBuf {
        self.with_server_state(|ss| {
            let state = ss.collections.get(&self.id).unwrap();
            let library_title = &ss.libraries.get(&state.library).unwrap().title;

            let name = match file_type {
                FileType::Thumbnail | FileType::Playlist => format!("{}.{extension}", state.title),
                _ => unreachable!("Invalid file type for MovieCollection"),
            };

            PathBuf::from(safe(&self.server.id))
                .join(safe(library_title))
                .join("Collections")
                .join(safe(&name))
        })
        .await
    }

    pub(crate) async fn write_playlist(&self) -> Result {
        let root = self.server.inner.path.read().await;
        let playlist_path = self.file_path(FileType::Playlist, "m3u").await;

        write_playlist(&root, &playlist_path, self.videos().await).await
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

    pub(crate) async fn file_path(&self, file_type: FileType, extension: &str) -> PathBuf {
        self.with_server_state(|ss| {
            let state = ss.collections.get(&self.id).unwrap();
            let library_title = &ss.libraries.get(&state.library).unwrap().title;

            let name = match file_type {
                FileType::Thumbnail | FileType::Playlist => format!("{}.{extension}", state.title),
                _ => unreachable!("Invalid file type for ShowCollection"),
            };

            PathBuf::from(safe(&self.server.id))
                .join(safe(library_title))
                .join("Collections")
                .join(safe(&name))
        })
        .await
    }

    pub(crate) async fn write_playlist(&self) -> Result {
        let mut videos: Vec<Video> = Vec::new();

        for show in self.shows().await {
            for season in show.seasons().await {
                for episode in season.episodes().await {
                    videos.push(Video::Episode(episode));
                }
            }
        }

        let root = self.server.inner.path.read().await;
        let playlist_path = self.file_path(FileType::Playlist, "m3u").await;

        write_playlist(&root, &playlist_path, videos).await
    }
}

#[derive(Clone)]
pub enum Collection {
    Movie(MovieCollection),
    Show(ShowCollection),
}

impl Collection {
    pub async fn update_thumbnail(&self, rebuild: bool) -> Result {
        match self {
            Self::Movie(c) => c.update_thumbnail(rebuild).await,
            Self::Show(c) => c.update_thumbnail(rebuild).await,
        }
    }

    pub(crate) async fn file_path(&self, file_type: FileType, extension: &str) -> PathBuf {
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
