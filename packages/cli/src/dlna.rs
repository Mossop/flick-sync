use std::{
    cmp::Ordering,
    io::{self, Cursor},
    pin::Pin,
    str::FromStr,
    task::{Context, Poll},
    time::Duration,
};

use async_trait::async_trait;
use dlna_server::{
    Container, DlnaRequestHandler, DlnaServer, DlnaServiceFactory, Icon, Item, Object, Resource,
    StreamResponse, UpnpError,
};
use flick_sync::{
    Collection, FlickSync, Library, LockedFile, MovieCollection, MovieLibrary, Playlist, Season,
    Server, Show, ShowCollection, ShowLibrary, Timeout, Video, VideoPart,
};
use image::ImageReader;
use lazy_static::lazy_static;
use mime::Mime;
use pin_project::pin_project;
use regex::Regex;
use tokio::{
    io::{AsyncRead, AsyncSeek, ReadBuf},
    spawn,
};
use tracing::{Instrument, Level, Span, instrument, span};

use crate::{Resources, shared::uniform_title};

lazy_static! {
    static ref RE_VIDEO_PART: Regex = Regex::new("^video/(.+)/VP:(.+)/(\\d+)$").unwrap();
}

#[pin_project(project = EitherReaderProj)]
enum EitherReader<A, B> {
    A(#[pin] A),
    B(#[pin] B),
}

impl<A: AsyncRead, B: AsyncRead> AsyncRead for EitherReader<A, B> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.project() {
            EitherReaderProj::A(reader) => reader.poll_read(cx, buf),
            EitherReaderProj::B(reader) => reader.poll_read(cx, buf),
        }
    }
}

#[pin_project]
struct ProgressReader<R> {
    #[pin]
    inner: R,
    span: Option<Span>,
    position: u64,
    video: Video,
    offset: u64,
    secs_per_byte: f64,
    last_report: u64,
}

impl<R> ProgressReader<R> {
    fn new(inner: R, video: Video, offset: u64, secs_per_byte: f64) -> Self {
        Self {
            inner,
            span: None,
            position: 0,
            video,
            offset,
            secs_per_byte,
            last_report: 0,
        }
    }
}

impl<R: AsyncRead> AsyncRead for ProgressReader<R> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let remaining = buf.remaining();

        let this = self.project();

        let span = this
            .span
            .get_or_insert_with(|| span!(Level::TRACE, "ProgressReader"));
        let _entered = span.enter();
        let result = this.inner.poll_read(cx, buf);

        if matches!(result, Poll::Ready(Ok(()))) {
            let read = remaining - buf.remaining();
            *this.position += read as u64;

            let new_position = (*this.position as f64 * *this.secs_per_byte) as u64 + *this.offset;

            if new_position.abs_diff(*this.last_report) > 15000 {
                *this.last_report = new_position;
                let video = this.video.clone();
                let span = span.clone();
                spawn(async move {
                    let _ = video
                        .set_playback_position(new_position)
                        .instrument(span)
                        .await;
                });
            }
        }

        result
    }
}

impl<R: AsyncSeek> AsyncSeek for ProgressReader<R> {
    fn start_seek(self: Pin<&mut Self>, position: io::SeekFrom) -> io::Result<()> {
        self.project().inner.start_seek(position)
    }

    fn poll_complete(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<u64>> {
        self.project().inner.poll_complete(cx)
    }
}

async fn video_part_from_id(flick_sync: &FlickSync, id: &str) -> Option<VideoPart> {
    let captures = RE_VIDEO_PART.captures(id)?;

    let server_id = captures.get(1).unwrap().as_str();
    let video_id = captures.get(2).unwrap().as_str();
    let video_part = captures.get(3).unwrap().as_str().parse::<usize>().unwrap();

    let server = flick_sync.server(server_id).await?;

    let video = server.video(video_id).await?;

    let parts = video.parts().await;

    Some(parts.get(video_part)?.clone())
}

// Object ID forms and hierarchy:
//
// 0                         - root
//   O                       - On Deck
//   L                       - Libraries
//     <server>/L:<id>       - Library
//       <server>/V:<id>     - Movie
//       <server>/S:<id>     - Show
//         <server>/N:<id>   - Season
//           <server>/V:<id> - Episode
//   C                       - Collections
//     <server>/C:<id>       - Collection
//       <server>/V:<id>     - Movie
//       <server>/S:<id>     - Show
//   P                       - Playlists
//     <server>/P:<id>       - Playlist
//       <server>/V:<id>     - Video

fn object_title(obj: &Object) -> String {
    match obj {
        Object::Container(o) => uniform_title(&o.title),
        Object::Item(o) => uniform_title(&o.title),
    }
}

fn sort_by_title(a: &Object, b: &Object) -> Ordering {
    object_title(a).cmp(&object_title(b))
}

async fn icon_resource(id: &str, file: Result<Option<LockedFile>, Timeout>) -> Option<Icon> {
    let file = file.ok()??;
    let reader = ImageReader::new(io::BufReader::new(file.read().ok()?))
        .with_guessed_format()
        .ok()?;

    let format = reader.format()?;
    let mime_type = Mime::from_str(format.to_mime_type()).ok()?;

    let (width, height) = reader.into_dimensions().ok()?;

    Some(Icon {
        id: format!("thumbnail/{id}"),
        mime_type,
        width,
        height,
        depth: 24,
    })
}

async fn file_resource(
    video_part: &VideoPart,
    file: Result<Option<LockedFile>, Timeout>,
) -> Option<Resource> {
    let file = file.ok()??;
    let size = file.len().await.ok()?;

    let mime_type = file.mime_type().await.ok()?;
    let video = video_part.video().await;

    Some(Resource {
        id: format!(
            "video/{}/VP:{}/{}",
            video.server().id(),
            video.id(),
            video_part.index()
        ),
        mime_type,
        duration: Some(video_part.duration().await),
        size: Some(size),
        seekable: true,
    })
}

trait ToObject
where
    Self: Sized,
{
    type Children: ToObject;

    async fn to_object(self) -> Object;
    async fn to_children(self) -> Vec<Self::Children>;

    fn sort_children(children: &mut Vec<Object>) {
        children.sort_by(sort_by_title);
    }

    async fn collect_children(self) -> Vec<Object> {
        let mut result = Vec::new();

        for child in self.to_children().await {
            result.push(child.to_object().await);
        }

        Self::sort_children(&mut result);

        result
    }
}

trait FromId
where
    Self: Sized,
{
    async fn from_id(server: Server, id: &str) -> Result<Self, UpnpError>;
}

impl ToObject for Object {
    type Children = Object;

    async fn to_object(self) -> Object {
        self
    }

    async fn to_children(self) -> Vec<Self::Children> {
        Vec::new()
    }
}

async fn video_parent(video: &Video) -> String {
    match video {
        Video::Movie(v) => format!("{}/L:{}", v.library().await.id(), v.server().id()),
        Video::Episode(v) => format!("{}/N:{}", v.season().await.id(), v.server().id()),
    }
}

impl FromId for VideoPart {
    async fn from_id(server: Server, id: &str) -> Result<Self, UpnpError> {
        let Some((video_id, index)) = id.split_once('/') else {
            return Err(UpnpError::unknown_object());
        };

        let Ok(index) = usize::from_str(index) else {
            return Err(UpnpError::unknown_object());
        };

        let Some(video) = server.video(video_id).await else {
            return Err(UpnpError::unknown_object());
        };

        let parts = video.parts().await;
        parts
            .into_iter()
            .nth(index)
            .ok_or(UpnpError::unknown_object())
    }
}

impl ToObject for VideoPart {
    type Children = Object;

    async fn to_object(self) -> Object {
        let video = self.video().await;
        let parts = video.parts().await;

        let video_id = format!("{}/V:{}", video.server().id(), video.id());

        let mut resources = Vec::new();
        if let Some(resource) = file_resource(&self, self.file().await).await {
            resources.push(resource);
        }

        let (id, title, parent_id) = if parts.len() == 1 {
            (
                video_id.clone(),
                video.title().await,
                video_parent(&video).await,
            )
        } else {
            (
                format!("{}/VP:{}/{}", video.server().id(), video.id(), self.index()),
                format!("{} - Pt {}", video.title().await, self.index() + 1),
                video_id.clone(),
            )
        };

        Object::Item(Item {
            thumbnail: icon_resource(&video_id, video.thumbnail().await).await,
            id,
            parent_id,
            title,
            resources,
        })
    }

    async fn to_children(self) -> Vec<Self::Children> {
        Vec::new()
    }
}

impl FromId for Video {
    async fn from_id(server: Server, id: &str) -> Result<Self, UpnpError> {
        server.video(id).await.ok_or(UpnpError::unknown_object())
    }
}

impl ToObject for Video {
    type Children = VideoPart;

    async fn to_object(self) -> Object {
        let parts = self.parts().await;

        if parts.len() == 1 {
            let part = parts.into_iter().next().unwrap();

            part.to_object().await
        } else {
            let id = format!("{}/V:{}", self.server().id(), self.id());
            Object::Container(Container {
                thumbnail: icon_resource(&id, self.thumbnail().await).await,
                id,
                parent_id: video_parent(&self).await,
                title: self.title().await,
                child_count: Some(parts.len()),
            })
        }
    }

    async fn to_children(self) -> Vec<Self::Children> {
        let parts = self.parts().await;

        if parts.len() == 1 { Vec::new() } else { parts }
    }

    fn sort_children(_: &mut Vec<Object>) {}
}

impl FromId for Playlist {
    async fn from_id(server: Server, id: &str) -> Result<Self, UpnpError> {
        server.playlist(id).await.ok_or(UpnpError::unknown_object())
    }
}

impl ToObject for Playlist {
    type Children = Video;

    async fn to_object(self) -> Object {
        let id = format!("{}/P:{}", self.server().id(), self.id());
        Object::Container(Container {
            thumbnail: icon_resource(&id, self.thumbnail().await).await,
            id,
            parent_id: "P".to_string(),
            child_count: Some(self.videos().await.len()),
            title: self.title().await,
        })
    }

    async fn to_children(self) -> Vec<Self::Children> {
        let mut result = Vec::new();

        for video in self.videos().await {
            if video.is_downloaded().await {
                result.push(video)
            }
        }

        result
    }

    fn sort_children(_: &mut Vec<Object>) {}
}

impl FromId for Collection {
    async fn from_id(server: Server, id: &str) -> Result<Self, UpnpError> {
        server
            .collection(id)
            .await
            .ok_or(UpnpError::unknown_object())
    }
}

impl ToObject for Collection {
    type Children = Object;

    async fn to_object(self) -> Object {
        match self {
            Collection::Movie(c) => c.to_object().await,
            Collection::Show(c) => c.to_object().await,
        }
    }

    async fn to_children(self) -> Vec<Self::Children> {
        match self {
            Collection::Movie(c) => c.collect_children().await,
            Collection::Show(c) => c.collect_children().await,
        }
    }

    async fn collect_children(self) -> Vec<Object> {
        match self {
            Collection::Movie(c) => c.collect_children().await,
            Collection::Show(c) => c.collect_children().await,
        }
    }
}

impl ToObject for MovieCollection {
    type Children = Video;

    async fn to_object(self) -> Object {
        let id = format!("{}/C:{}", self.server().id(), self.id());

        Object::Container(Container {
            thumbnail: icon_resource(&id, self.thumbnail().await).await,
            id,
            parent_id: "C".to_string(),
            child_count: Some(self.movies().await.len()),
            title: self.title().await,
        })
    }

    async fn to_children(self) -> Vec<Self::Children> {
        let mut result = Vec::new();

        for movie in self.movies().await {
            let video = Video::Movie(movie);
            if video.is_downloaded().await {
                result.push(video)
            }
        }

        result
    }

    fn sort_children(_: &mut Vec<Object>) {}
}

impl ToObject for ShowCollection {
    type Children = Show;

    async fn to_object(self) -> Object {
        let id = format!("{}/C:{}", self.server().id(), self.id());
        Object::Container(Container {
            thumbnail: icon_resource(&id, self.thumbnail().await).await,
            id,
            parent_id: "C".to_string(),
            child_count: Some(self.shows().await.len()),
            title: self.title().await,
        })
    }

    async fn to_children(self) -> Vec<Self::Children> {
        self.shows().await
    }
}

impl FromId for Library {
    async fn from_id(server: Server, id: &str) -> Result<Self, UpnpError> {
        server.library(id).await.ok_or(UpnpError::unknown_object())
    }
}

impl ToObject for Library {
    type Children = Object;

    async fn to_object(self) -> Object {
        match self {
            Library::Movie(l) => l.to_object().await,
            Library::Show(l) => l.to_object().await,
        }
    }

    async fn to_children(self) -> Vec<Self::Children> {
        match self {
            Library::Movie(l) => l.collect_children().await,
            Library::Show(l) => l.collect_children().await,
        }
    }

    async fn collect_children(self) -> Vec<Object> {
        match self {
            Library::Movie(l) => l.collect_children().await,
            Library::Show(l) => l.collect_children().await,
        }
    }
}

impl ToObject for MovieLibrary {
    type Children = Video;

    async fn to_object(self) -> Object {
        Object::Container(Container {
            id: format!("{}/L:{}", self.server().id(), self.id()),
            parent_id: "L".to_string(),
            child_count: Some(self.movies().await.len()),
            title: self.title().await,
            thumbnail: Some(Icon {
                id: "resource/movie-256.png".to_string(),
                mime_type: mime::IMAGE_PNG,
                width: 256,
                height: 256,
                depth: 32,
            }),
        })
    }

    async fn to_children(self) -> Vec<Self::Children> {
        let mut result = Vec::new();

        for movie in self.movies().await {
            let video = Video::Movie(movie);
            if video.is_downloaded().await {
                result.push(video)
            }
        }

        result
    }
}

impl ToObject for ShowLibrary {
    type Children = Show;

    async fn to_object(self) -> Object {
        Object::Container(Container {
            id: format!("{}/L:{}", self.server().id(), self.id()),
            parent_id: "L".to_string(),
            child_count: Some(self.shows().await.len()),
            title: self.title().await,
            thumbnail: Some(Icon {
                id: "resource/television-256.png".to_string(),
                mime_type: mime::IMAGE_PNG,
                width: 256,
                height: 256,
                depth: 32,
            }),
        })
    }

    async fn to_children(self) -> Vec<Self::Children> {
        self.shows().await
    }
}

impl FromId for Show {
    async fn from_id(server: Server, id: &str) -> Result<Self, UpnpError> {
        server.show(id).await.ok_or(UpnpError::unknown_object())
    }
}

impl ToObject for Show {
    type Children = Season;

    async fn to_object(self) -> Object {
        let library = self.library().await;
        let id = format!("{}/S:{}", self.server().id(), self.id());

        Object::Container(Container {
            thumbnail: icon_resource(&id, self.thumbnail().await).await,
            id,
            parent_id: format!("{}/L:{}", library.server().id(), library.id()),
            child_count: Some(self.seasons().await.len()),
            title: self.title().await,
        })
    }

    async fn to_children(self) -> Vec<Self::Children> {
        self.seasons().await
    }

    fn sort_children(_: &mut Vec<Object>) {}
}

impl FromId for Season {
    async fn from_id(server: Server, id: &str) -> Result<Self, UpnpError> {
        server.season(id).await.ok_or(UpnpError::unknown_object())
    }
}

impl ToObject for Season {
    type Children = Video;

    async fn to_object(self) -> Object {
        let show = self.show().await;
        let parent_id = format!("{}/S:{}", show.server().id(), show.id());

        Object::Container(Container {
            thumbnail: icon_resource(&parent_id, show.thumbnail().await).await,
            id: format!("{}/N:{}", self.server().id(), self.id()),
            parent_id,
            child_count: Some(self.episodes().await.len()),
            title: self.title().await,
        })
    }

    async fn to_children(self) -> Vec<Self::Children> {
        let mut result = Vec::new();

        for episode in self.episodes().await {
            let video = Video::Episode(episode);
            if video.is_downloaded().await {
                result.push(video)
            }
        }

        result
    }

    fn sort_children(_: &mut Vec<Object>) {}
}

struct Root {
    flick_sync: FlickSync,
}

impl ToObject for Root {
    type Children = Object;

    async fn to_object(self) -> Object {
        Object::Container(Container {
            id: "0".to_string(),
            parent_id: "-1".to_string(),
            child_count: Some(3),
            title: "Flick Sync Synced Media".to_string(),
            thumbnail: None,
        })
    }

    async fn to_children(self) -> Vec<Self::Children> {
        vec![
            OnDeck {
                flick_sync: self.flick_sync.clone(),
            }
            .to_object()
            .await,
            Libraries {
                flick_sync: self.flick_sync.clone(),
            }
            .to_object()
            .await,
            Collections {
                flick_sync: self.flick_sync.clone(),
            }
            .to_object()
            .await,
            Playlists {
                flick_sync: self.flick_sync.clone(),
            }
            .to_object()
            .await,
        ]
    }

    fn sort_children(_: &mut Vec<Object>) {}
}

struct OnDeck {
    flick_sync: FlickSync,
}

impl ToObject for OnDeck {
    type Children = Video;

    async fn to_object(self) -> Object {
        Object::Container(Container {
            id: "O".to_string(),
            parent_id: "0".to_string(),
            child_count: Some(self.flick_sync.on_deck().await.len()),
            title: "On Deck".to_string(),
            thumbnail: Some(Icon {
                id: "resource/logo-256.png".to_string(),
                mime_type: mime::IMAGE_PNG,
                width: 256,
                height: 256,
                depth: 32,
            }),
        })
    }

    async fn to_children(self) -> Vec<Self::Children> {
        self.flick_sync.on_deck().await
    }
}

struct Libraries {
    flick_sync: FlickSync,
}

impl ToObject for Libraries {
    type Children = Library;

    async fn to_object(self) -> Object {
        let mut library_count = 0;
        for server in self.flick_sync.servers().await {
            library_count += server.libraries().await.len();
        }

        Object::Container(Container {
            id: "L".to_string(),
            parent_id: "0".to_string(),
            child_count: Some(library_count),
            title: "Libraries".to_string(),
            thumbnail: Some(Icon {
                id: "resource/library-256.png".to_string(),
                mime_type: mime::IMAGE_PNG,
                width: 256,
                height: 256,
                depth: 32,
            }),
        })
    }

    async fn to_children(self) -> Vec<Self::Children> {
        let mut libraries = Vec::new();

        for server in self.flick_sync.servers().await {
            libraries.extend(server.libraries().await);
        }

        libraries
    }
}

struct Playlists {
    flick_sync: FlickSync,
}

impl ToObject for Playlists {
    type Children = Playlist;

    async fn to_object(self) -> Object {
        let mut playlist_count = 0;
        for server in self.flick_sync.servers().await {
            playlist_count += server.playlists().await.len();
        }

        Object::Container(Container {
            id: "P".to_string(),
            parent_id: "0".to_string(),
            child_count: Some(playlist_count),
            title: "Playlists".to_string(),
            thumbnail: Some(Icon {
                id: "resource/media-256.png".to_string(),
                mime_type: mime::IMAGE_PNG,
                width: 256,
                height: 256,
                depth: 32,
            }),
        })
    }

    async fn to_children(self) -> Vec<Self::Children> {
        let mut playlists = Vec::new();

        for server in self.flick_sync.servers().await {
            playlists.extend(server.playlists().await);
        }

        playlists
    }
}

struct Collections {
    flick_sync: FlickSync,
}

impl ToObject for Collections {
    type Children = Collection;

    async fn to_object(self) -> Object {
        let mut collection_count = 0;
        for server in self.flick_sync.servers().await {
            collection_count += server.collections().await.len();
        }

        Object::Container(Container {
            id: "C".to_string(),
            parent_id: "0".to_string(),
            child_count: Some(collection_count),
            title: "Collections".to_string(),
            thumbnail: Some(Icon {
                id: "resource/library-256.png".to_string(),
                mime_type: mime::IMAGE_PNG,
                width: 256,
                height: 256,
                depth: 32,
            }),
        })
    }

    async fn to_children(self) -> Vec<Self::Children> {
        let mut collections = Vec::new();

        for server in self.flick_sync.servers().await {
            collections.extend(server.collections().await);
        }

        collections
    }
}

pub(crate) struct DlnaHandler {
    flick_sync: FlickSync,
}

impl DlnaHandler {
    async fn extract_id<'a>(&self, object_id: &'a str) -> Option<(Server, &'a str, &'a str)> {
        let (server_id, item) = object_id.split_once('/')?;
        let (item_type, item_id) = item.split_once(':')?;

        let server = self.flick_sync.server(server_id).await?;

        Some((server, item_type, item_id))
    }
}

#[async_trait]
impl DlnaRequestHandler for DlnaHandler {
    async fn get_object(&self, object_id: &str) -> Result<Object, UpnpError> {
        if object_id == "0" {
            Ok(Root {
                flick_sync: self.flick_sync.clone(),
            }
            .to_object()
            .await)
        } else if object_id == "O" {
            Ok(OnDeck {
                flick_sync: self.flick_sync.clone(),
            }
            .to_object()
            .await)
        } else if object_id == "L" {
            Ok(Libraries {
                flick_sync: self.flick_sync.clone(),
            }
            .to_object()
            .await)
        } else if object_id == "P" {
            Ok(Playlists {
                flick_sync: self.flick_sync.clone(),
            }
            .to_object()
            .await)
        } else if object_id == "C" {
            Ok(Collections {
                flick_sync: self.flick_sync.clone(),
            }
            .to_object()
            .await)
        } else {
            let Some((server, item_type, item_id)) = self.extract_id(object_id).await else {
                return Err(UpnpError::unknown_object());
            };

            match item_type {
                "L" => Ok(Library::from_id(server, item_id).await?.to_object().await),
                "P" => Ok(Playlist::from_id(server, item_id).await?.to_object().await),
                "C" => Ok(Collection::from_id(server, item_id)
                    .await?
                    .to_object()
                    .await),
                "S" => Ok(Show::from_id(server, item_id).await?.to_object().await),
                "N" => Ok(Season::from_id(server, item_id).await?.to_object().await),
                "V" => Ok(Video::from_id(server, item_id).await?.to_object().await),
                "VP" => Ok(VideoPart::from_id(server, item_id).await?.to_object().await),
                _ => Err(UpnpError::unknown_object()),
            }
        }
    }

    async fn list_children(&self, object_id: &str) -> Result<Vec<Object>, UpnpError> {
        if object_id == "0" {
            Ok(Root {
                flick_sync: self.flick_sync.clone(),
            }
            .collect_children()
            .await)
        } else if object_id == "O" {
            Ok(OnDeck {
                flick_sync: self.flick_sync.clone(),
            }
            .collect_children()
            .await)
        } else if object_id == "L" {
            Ok(Libraries {
                flick_sync: self.flick_sync.clone(),
            }
            .collect_children()
            .await)
        } else if object_id == "P" {
            Ok(Playlists {
                flick_sync: self.flick_sync.clone(),
            }
            .collect_children()
            .await)
        } else if object_id == "C" {
            Ok(Collections {
                flick_sync: self.flick_sync.clone(),
            }
            .collect_children()
            .await)
        } else {
            let Some((server, item_type, item_id)) = self.extract_id(object_id).await else {
                return Err(UpnpError::unknown_object());
            };

            match item_type {
                "L" => Ok(Library::from_id(server, item_id)
                    .await?
                    .collect_children()
                    .await),
                "P" => Ok(Playlist::from_id(server, item_id)
                    .await?
                    .collect_children()
                    .await),
                "C" => Ok(Collection::from_id(server, item_id)
                    .await?
                    .collect_children()
                    .await),
                "S" => Ok(Show::from_id(server, item_id)
                    .await?
                    .collect_children()
                    .await),
                "N" => Ok(Season::from_id(server, item_id)
                    .await?
                    .collect_children()
                    .await),
                "V" => Ok(Video::from_id(server, item_id)
                    .await?
                    .collect_children()
                    .await),
                "VP" => Ok(VideoPart::from_id(server, item_id)
                    .await?
                    .collect_children()
                    .await),
                _ => Err(UpnpError::unknown_object()),
            }
        }
    }

    async fn stream_icon(
        &self,
        icon_id: &str,
    ) -> Result<StreamResponse<impl AsyncRead + 'static>, UpnpError> {
        if let Some(object_id) = icon_id.strip_prefix("thumbnail/") {
            let Some((server, item_type, item_id)) = self.extract_id(object_id).await else {
                return Err(UpnpError::unknown_object());
            };

            let Ok(Some(thumbnail)) = (match item_type {
                "P" => Playlist::from_id(server, item_id).await?.thumbnail().await,
                "C" => {
                    Collection::from_id(server, item_id)
                        .await?
                        .thumbnail()
                        .await
                }
                "S" => Show::from_id(server, item_id).await?.thumbnail().await,
                "V" => Video::from_id(server, item_id).await?.thumbnail().await,
                _ => return Err(UpnpError::unknown_object()),
            }) else {
                return Err(UpnpError::unknown_object());
            };

            let Ok(mime_type) = thumbnail.mime_type().await else {
                return Err(UpnpError::unknown_object());
            };

            let Ok(size) = thumbnail.len().await else {
                return Err(UpnpError::unknown_object());
            };

            let Ok(async_reader) = thumbnail.async_read().await else {
                return Err(UpnpError::unknown_object());
            };

            Ok(StreamResponse {
                mime_type,
                resource_size: Some(size),
                reader: EitherReader::A(async_reader),
            })
        } else if let Some(resource) = icon_id.strip_prefix("resource/") {
            let Some(icon_file) = Resources::get(&format!("upnp/{resource}")) else {
                return Err(UpnpError::unknown_object());
            };

            Ok(StreamResponse {
                mime_type: mime::IMAGE_PNG,
                resource_size: Some(icon_file.data.len() as u64),
                reader: EitherReader::B(Cursor::new(icon_file.data)),
            })
        } else {
            return Err(UpnpError::unknown_object());
        }
    }

    async fn get_resource(&self, resource_id: &str) -> Result<Resource, UpnpError> {
        let Some(part) = video_part_from_id(&self.flick_sync, resource_id).await else {
            return Err(UpnpError::unknown_object());
        };

        let Ok(Some(file)) = part.file().await else {
            return Err(UpnpError::unknown_object());
        };

        let Ok(size) = file.len().await else {
            return Err(UpnpError::unknown_object());
        };

        let Ok(mime_type) = file.mime_type().await else {
            return Err(UpnpError::unknown_object());
        };

        Ok(Resource {
            id: resource_id.to_owned(),
            mime_type,
            size: Some(size),
            seekable: true,
            duration: None,
        })
    }

    #[instrument(skip(self))]
    async fn stream_resource(
        &self,
        resource_id: &str,
    ) -> Result<impl AsyncRead + AsyncSeek + Unpin + 'static, UpnpError> {
        let Some(part) = video_part_from_id(&self.flick_sync, resource_id).await else {
            return Err(UpnpError::unknown_object());
        };

        let Ok(Some(file)) = part.file().await else {
            return Err(UpnpError::unknown_object());
        };

        let Ok(size) = file.len().await else {
            return Err(UpnpError::unknown_object());
        };

        let Ok(reader) = file.async_read().await else {
            return Err(UpnpError::unknown_object());
        };

        let video = part.video().await;
        let mut initial_duration = Duration::from_millis(0);
        if part.index() > 0 {
            let parts = video.parts().await;
            for previous in &parts[0..part.index()] {
                initial_duration += previous.duration().await;
            }
        }

        let part_duration = part.duration().await;
        let secs_per_byte = part_duration.as_millis() as f64 / size as f64;

        let progress_reader = ProgressReader::new(
            reader,
            video,
            initial_duration.as_millis() as u64,
            secs_per_byte,
        );

        Ok(progress_reader)
    }
}

pub(crate) async fn build_dlna(
    flick_sync: FlickSync,
    port: u16,
) -> anyhow::Result<(DlnaServer, DlnaServiceFactory<DlnaHandler>)> {
    let uuid = flick_sync.client_id().await;
    let handler = DlnaHandler { flick_sync };

    DlnaServer::builder(handler)
        .uuid(uuid)
        .http_port(port)
        .server_version(&format!(
            "FlickSync/{}.{}",
            env!("CARGO_PKG_VERSION_MAJOR"),
            env!("CARGO_PKG_VERSION_MINOR")
        ))
        .server_name("Synced Flicks")
        .icon(Icon {
            id: "resource/logo-32.png".to_string(),
            mime_type: mime::IMAGE_PNG,
            width: 32,
            height: 32,
            depth: 32,
        })
        .icon(Icon {
            id: "resource/logo-64.png".to_string(),
            mime_type: mime::IMAGE_PNG,
            width: 64,
            height: 64,
            depth: 32,
        })
        .icon(Icon {
            id: "resource/logo-128.png".to_string(),
            mime_type: mime::IMAGE_PNG,
            width: 128,
            height: 128,
            depth: 32,
        })
        .icon(Icon {
            id: "resource/logo-256.png".to_string(),
            mime_type: mime::IMAGE_PNG,
            width: 256,
            height: 256,
            depth: 32,
        })
        .build_service()
        .await
}
