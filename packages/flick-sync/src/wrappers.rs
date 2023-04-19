use std::{
    cmp::max,
    fmt,
    io::{ErrorKind, IoSlice},
    path::{Path, PathBuf},
    pin::Pin,
    result,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};

use async_std::fs::{create_dir_all, File, OpenOptions};
use async_trait::async_trait;
use futures::AsyncWrite;
use pin_project::pin_project;
use plex_api::{
    library::{self, Item, MediaItem, MetadataItem},
    transcode::{TranscodeSession, TranscodeStatus},
};
use tokio::{
    fs::{metadata, remove_file},
    time::sleep,
};
use tracing::{debug, error, info, instrument, trace, warn};

use crate::{
    state::{
        CollectionState, DownloadState, LibraryState, PlaylistState, SeasonState, ServerState,
        ShowState, ThumbnailState, VideoDetail, VideoPartState, VideoState,
    },
    Error, Inner, Result, Server,
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

#[derive(Debug, Clone, Copy)]
enum FileType {
    Video(usize),
    Thumbnail,
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
        pub(crate) async fn thumbnail(&self) -> ThumbnailState {
            self.with_state(|s| s.thumbnail.clone()).await
        }

        #[instrument(level = "trace")]
        pub async fn update_thumbnail(&self) -> Result {
            let root = self.inner.path.read().await.to_owned();

            let mut thumbnail = self.thumbnail().await;
            thumbnail.verify(&root).await;

            self.update_state(|s| s.thumbnail = thumbnail.clone())
                .await?;

            if thumbnail.is_none() {
                let server = self.connect().await?;
                let item = server.item_by_id(&self.id).await?;
                debug!("Updating thumbnail for {}", item.title());

                let image = if let Some(ref thumb) = item.metadata().thumb {
                    thumb.clone()
                } else {
                    warn!("No thumbnail found for {}", item.title());
                    return Ok(());
                };

                let path = self.file_path(FileType::Thumbnail, "jpg").await;
                let target = root.join(&path);

                if let Some(parent) = target.parent() {
                    create_dir_all(parent).await?;
                }

                let file = File::create(root.join(&path)).await?;
                server
                    .transcode_artwork(&image, 320, 320, Default::default(), file)
                    .await?;

                let state = ThumbnailState::Downloaded { path };

                self.update_state(|s| s.thumbnail = state).await?;
                trace!("Thumbnail for {} successfully updated", item.title());
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
                id: ss.$($pprop)*.clone(),
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
                                id: id.clone(),
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
    pub(crate) id: String,
    pub(crate) inner: Arc<Inner>,
}

impl fmt::Debug for Show {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(&format!("Show({})", self.id))
    }
}

state_wrapper!(Show, ShowState, shows);

impl Show {
    thumbnail_methods!();
    parent!(library, ShowLibrary, library);
    children!(seasons, seasons, Season, show);

    async fn file_path(&self, file_type: FileType, extension: &str) -> PathBuf {
        self.with_server_state(|ss| {
            let state = ss.shows.get(&self.id).unwrap();

            let name = match file_type {
                FileType::Video(_) => panic!("Unexpected"),
                FileType::Thumbnail => format!(".thumb.{extension}"),
            };

            let library_title = &ss.libraries.get(&state.library).unwrap().title;
            PathBuf::from(safe(&self.server))
                .join(safe(library_title))
                .join(safe(format!("{} ({})", state.title, state.year)))
                .join(safe(name))
        })
        .await
    }
}

#[derive(Clone)]
pub struct Season {
    pub(crate) server: String,
    pub(crate) id: String,
    pub(crate) inner: Arc<Inner>,
}

impl fmt::Debug for Season {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(&format!("Season({})", self.id))
    }
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
                                id: id.clone(),
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

impl<'a, W, P> AsyncWrite for WriterProgress<'a, W, P>
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
    pub(crate) server: String,
    pub(crate) id: String,
    pub(crate) index: usize,
    pub(crate) inner: Arc<Inner>,
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

    async fn with_video_state<F, R>(&self, cb: F) -> R
    where
        F: Send + FnOnce(&VideoState) -> R,
    {
        self.with_server_state(|ss| cb(ss.videos.get(&self.id).unwrap()))
            .await
    }

    pub async fn transfer_state(&self) -> TransferState {
        let download_state = self.download_state().await;

        match download_state {
            DownloadState::None => TransferState::Waiting,
            DownloadState::Downloading { path: _ } => TransferState::Downloading,
            DownloadState::Transcoding {
                session_id: _,
                path: _,
            } => TransferState::Transcoding,
            _ => TransferState::Downloaded,
        }
    }

    pub async fn verify_download(&self) -> Result {
        let server = self.connect().await?;
        let mut download_state = self.download_state().await;
        let root = self.inner.path.read().await.clone();

        download_state.verify(&server, &root).await;

        self.update_state(|state| state.download = download_state)
            .await
    }

    pub async fn video(&self) -> Video {
        self.with_server_state(|server_state| {
            let video_state = server_state.videos.get(&self.id).unwrap();

            match video_state.detail {
                VideoDetail::Movie(_) => Video::Movie(Movie {
                    server: self.server.clone(),
                    id: self.id.clone(),
                    inner: self.inner.clone(),
                }),
                VideoDetail::Episode(_) => Video::Episode(Episode {
                    server: self.server.clone(),
                    id: self.id.clone(),
                    inner: self.inner.clone(),
                }),
            }
        })
        .await
    }

    async fn file_path(&self, extension: &str) -> PathBuf {
        let video = self.video().await;
        video
            .file_path(FileType::Video(self.index), extension)
            .await
    }

    pub async fn is_downloaded(&self) -> bool {
        let download_state = self.download_state().await;
        !download_state.needs_download()
    }

    #[instrument(level = "trace", skip(self), fields(session_id))]
    async fn start_transcode(&self) -> Result<TranscodeSession> {
        let server = self.connect().await?;
        let item = server.item_by_id(&self.id).await?;

        let video = match item {
            Item::Movie(m) => library::Video::Movie(m),
            Item::Episode(e) => library::Video::Episode(e),
            _ => panic!("Unexpected item type"),
        };

        let (media_id, profile) = self
            .with_video_state(|vs| (vs.media_id.clone(), vs.transcode_profile.clone()))
            .await;

        let media = video
            .media()
            .into_iter()
            .find(|m| m.metadata().id.as_ref() == Some(&media_id))
            .ok_or_else(|| Error::MissingItem)?;
        let parts = media.parts();
        let part = parts.get(self.index).ok_or_else(|| Error::MissingItem)?;

        let options = self.inner.transcode_options(profile).await;

        info!("Starting transcode");

        let session = part.create_download_session(options).await?;

        tracing::Span::current().record("session_id", session.session_id());

        // Wait until the transcode session has started.
        let mut count = 0;
        loop {
            sleep(Duration::from_millis(100)).await;

            match session.stats().await {
                Ok(_) => {
                    break;
                }
                Err(plex_api::Error::UnexpectedApiResponse {
                    status_code: 404,
                    content: _,
                }) => {
                    count += 1;
                    if count > 20 {
                        error!("Transcode session failed to start");
                        return Err(Error::TranscodeFailed);
                    }
                }
                Err(e) => return Err(e.into()),
            }
        }

        debug!("Started transcode session");

        let path = self.file_path(&session.container().to_string()).await;

        self.update_state(|state| {
            state.download = DownloadState::Transcoding {
                session_id: session.session_id().to_string(),
                path,
            }
        })
        .await?;

        Ok(session)
    }

    #[instrument(level = "trace", skip(self))]
    async fn start_download(&self) -> Result {
        let server = self.connect().await?;
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

        let target = { self.inner.path.read().await.join(&path) };
        if let Err(e) = remove_file(target).await {
            if e.kind() != ErrorKind::NotFound {
                return Err(Error::from(e));
            }
        }

        self.update_state(|state| state.download = DownloadState::Downloading { path })
            .await?;

        Ok(())
    }

    #[instrument(level = "trace", skip(self, session), fields(session_id=session.session_id()))]
    async fn wait_for_transcode(&self, session: TranscodeSession) -> Result {
        loop {
            match session.status().await {
                Ok(TranscodeStatus::Complete) => break,
                Ok(TranscodeStatus::Error) => return Err(Error::TranscodeFailed),
                Ok(TranscodeStatus::Transcoding {
                    remaining,
                    progress: _,
                }) => {
                    let delay = match remaining {
                        Some(secs) => {
                            let delay = max(5, secs / 2);
                            trace!("Item due in {secs}s, delaying for {delay}s");
                            delay
                        }
                        None => 5,
                    };

                    sleep(Duration::from_secs(delay as u64)).await;
                }
                Err(e) => {
                    error!("Error getting transcode status",);
                    return Err(e.into());
                }
            }
        }

        Ok(())
    }

    #[instrument(level = "trace")]
    pub async fn prepare_download(&self) -> Result {
        let download_state = self.download_state().await;

        if matches!(download_state, DownloadState::None) {
            if let Err(e) = self.start_transcode().await {
                if !matches!(
                    e,
                    Error::PlexError {
                        source: plex_api::Error::TranscodeRefused
                    }
                ) {
                    warn!(error=?e, "Transcode attempt failed");
                } else {
                    trace!("Transcode attempt refused");
                }

                self.start_download().await?;
            }
        }

        Ok(())
    }

    #[instrument(level = "trace")]
    pub async fn wait_for_download(&self) -> Result {
        self.prepare_download().await?;

        let download_state = self.download_state().await;
        if let DownloadState::Transcoding {
            session_id,
            path: _,
        } = download_state
        {
            let server = self.connect().await?;
            let session = server.transcode_session(&session_id).await?;
            self.wait_for_transcode(session).await?;
        }

        Ok(())
    }

    #[instrument(level = "trace", skip(self, path, progress))]
    async fn download_direct<P: Progress + Unpin>(&self, path: &Path, mut progress: P) -> Result {
        let target = { self.inner.path.read().await.join(path) };
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

        let server = self.connect().await?;
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

        let file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(&target)
            .await?;

        let writer = WriterProgress {
            offset,
            size: part.metadata().size.unwrap(),
            writer: file,
            progress: &mut progress,
        };
        debug!(offset, "Downloading");

        part.download(writer, offset..).await?;
        info!("Download complete");

        self.update_state(|state| {
            state.download = DownloadState::Downloaded {
                path: path.to_owned(),
            }
        })
        .await?;

        Ok(())
    }

    #[instrument(level = "trace", skip(self, session_id, path, progress))]
    async fn download_transcode<P: Progress + Unpin>(
        &self,
        session_id: &str,
        path: &Path,
        mut progress: P,
    ) -> Result {
        let server = self.connect().await?;
        let session = server.transcode_session(session_id).await?;
        let status = session.status().await?;
        let stats = session.stats().await?;

        if !matches!(status, TranscodeStatus::Complete) {
            return Err(Error::DownloadUnavailable);
        }

        let target = { self.inner.path.read().await.join(path) };
        let file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(&target)
            .await?;

        let writer = WriterProgress {
            offset: 0,
            size: stats.size as u64,
            writer: file,
            progress: &mut progress,
        };
        debug!("Downloading transcoded video");

        session.download(writer).await?;
        info!("Download complete");

        self.update_state(|state| {
            state.download = DownloadState::Transcoded {
                path: path.to_owned(),
            }
        })
        .await?;

        if let Err(e) = session.cancel().await {
            warn!(
                error=?e,
                "Transcode session failed to cancel"
            );
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
            DownloadState::Downloaded { path: _ } | DownloadState::Transcoded { path: _ } => Ok(()),
        }
    }

    async fn download_state(&self) -> DownloadState {
        self.with_state(|part_state| part_state.download.clone())
            .await
    }
}

#[async_trait]
impl StateWrapper<VideoPartState> for VideoPart {
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
        cb(state.servers.get(&self.server).unwrap())
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
        let mut state = self.inner.state.write().await;
        let server_state = state.servers.get_mut(&self.server).unwrap();
        cb(server_state
            .videos
            .get_mut(&self.id)
            .unwrap()
            .parts
            .get_mut(self.index)
            .unwrap());
        self.inner.persist_state(&state).await
    }
}

#[derive(Clone)]
pub struct Episode {
    pub(crate) server: String,
    pub(crate) id: String,
    pub(crate) inner: Arc<Inner>,
}

impl fmt::Debug for Episode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(&format!("Episode({})", self.id))
    }
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

    pub async fn title(&self) -> String {
        self.with_state(|s| s.title.clone()).await
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
                    inner: self.inner.clone(),
                })
                .collect()
        })
        .await
    }

    async fn file_path(&self, file_type: FileType, extension: &str) -> PathBuf {
        self.with_server_state(|ss| {
            let state = ss.videos.get(&self.id).unwrap();
            let ep_state = state.episode_state();
            let season = ss.seasons.get(&ep_state.season).unwrap();
            let show = ss.shows.get(&season.show).unwrap();
            let library_title = &ss.libraries.get(&show.library).unwrap().title;

            let name = match file_type {
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
                FileType::Thumbnail => format!(
                    ".S{:02}E{:02}.thumb.{extension}",
                    season.index, ep_state.index
                ),
            };

            PathBuf::from(safe(&self.server))
                .join(safe(library_title))
                .join(safe(format!("{} ({})", show.title, show.year)))
                .join(safe(name))
        })
        .await
    }
}

#[derive(Clone)]
pub struct Movie {
    pub(crate) server: String,
    pub(crate) id: String,
    pub(crate) inner: Arc<Inner>,
}

impl fmt::Debug for Movie {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(&format!("Movie({})", self.id))
    }
}

state_wrapper!(Movie, VideoState, videos);

impl Movie {
    thumbnail_methods!();
    parent!(library, MovieLibrary, movie_state().library);

    pub async fn title(&self) -> String {
        self.with_state(|s| s.title.clone()).await
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
                    inner: self.inner.clone(),
                })
                .collect()
        })
        .await
    }

    async fn file_path(&self, file_type: FileType, extension: &str) -> PathBuf {
        self.with_server_state(|ss| {
            let state = ss.videos.get(&self.id).unwrap();
            let m_state = state.movie_state();
            let library_title = &ss.libraries.get(&m_state.library).unwrap().title;

            let name = match file_type {
                FileType::Video(index) => {
                    let part_name = if state.parts.len() == 1 {
                        "".to_string()
                    } else {
                        format!(" - pt{}", index + 1)
                    };

                    format!("{} ({}){part_name}.{extension}", state.title, m_state.year)
                }
                FileType::Thumbnail => format!(".thumb.{extension}",),
            };

            PathBuf::from(safe(&self.server))
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
    pub async fn library(&self) -> Library {
        match self {
            Self::Movie(v) => Library::Movie(v.library().await),
            Self::Episode(v) => Library::Show(v.library().await),
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

    async fn file_path(&self, file_type: FileType, extension: &str) -> PathBuf {
        match self {
            Self::Movie(v) => v.file_path(file_type, extension).await,
            Self::Episode(v) => v.file_path(file_type, extension).await,
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
    pub(crate) id: String,
    pub(crate) inner: Arc<Inner>,
}

impl fmt::Debug for Playlist {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(&format!("Playlist({})", self.id))
    }
}

state_wrapper!(Playlist, PlaylistState, playlists);

impl Playlist {
    pub async fn videos(&self) -> Vec<Video> {
        self.with_server_state(|ss| {
            let ps = ss.playlists.get(&self.id).unwrap();
            ps.videos
                .iter()
                .map(|id| match ss.videos.get(id).unwrap().detail {
                    VideoDetail::Movie(_) => Video::Movie(Movie {
                        server: self.server.clone(),
                        id: id.clone(),
                        inner: self.inner.clone(),
                    }),
                    VideoDetail::Episode(_) => Video::Episode(Episode {
                        server: self.server.clone(),
                        id: id.clone(),
                        inner: self.inner.clone(),
                    }),
                })
                .collect()
        })
        .await
    }
}

#[derive(Clone)]
pub struct MovieCollection {
    pub(crate) server: String,
    pub(crate) id: String,
    pub(crate) inner: Arc<Inner>,
}

impl fmt::Debug for MovieCollection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(&format!("MovieCollection({})", self.id))
    }
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
                    id: id.clone(),
                    inner: self.inner.clone(),
                })
                .collect()
        })
        .await
    }

    async fn file_path(&self, _file_type: FileType, extension: &str) -> PathBuf {
        self.with_server_state(|ss| {
            let state = ss.collections.get(&self.id).unwrap();
            let library_title = &ss.libraries.get(&state.library).unwrap().title;

            PathBuf::from(safe(&self.server))
                .join(safe(library_title))
                .join(safe(format!(".{}.{extension}", state.id)))
        })
        .await
    }
}

#[derive(Clone)]
pub struct ShowCollection {
    pub(crate) server: String,
    pub(crate) id: String,
    pub(crate) inner: Arc<Inner>,
}

impl fmt::Debug for ShowCollection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(&format!("ShowCollection({})", self.id))
    }
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
                    id: id.clone(),
                    inner: self.inner.clone(),
                })
                .collect()
        })
        .await
    }

    async fn file_path(&self, _file_type: FileType, extension: &str) -> PathBuf {
        self.with_server_state(|ss| {
            let state = ss.collections.get(&self.id).unwrap();
            let library_title = &ss.libraries.get(&state.library).unwrap().title;

            PathBuf::from(safe(&self.server))
                .join(safe(library_title))
                .join(safe(format!(".{}.{extension}", state.id)))
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

impl fmt::Debug for MovieLibrary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(&format!("MovieLibrary({})", self.id))
    }
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
                                id: id.clone(),
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

impl fmt::Debug for ShowLibrary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(&format!("ShowLibrary({})", self.id))
    }
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
