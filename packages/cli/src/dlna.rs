use std::{
    cmp::Ordering,
    io::{self, SeekFrom},
    net::Ipv4Addr,
    os::unix::fs::MetadataExt,
    path::PathBuf,
    pin::Pin,
    str::FromStr,
    sync::Arc,
    task::{Context, Poll},
};

use async_trait::async_trait;
use bytes::Bytes;
use clap::Args;
use dlna_server::{
    Container, DlnaRequestHandler, DlnaServer, Icon, Item, Object, Range, Resource, StreamResponse,
    UpnpError,
};
use file_format::FileFormat;
use flick_sync::{
    Collection, FlickSync, Library, MovieCollection, MovieLibrary, Playlist, Season, Server, Show,
    ShowCollection, ShowLibrary, Video, VideoPart,
};
use futures::Stream;
use image::image_dimensions;
use mime::Mime;
use pathdiff::diff_paths;
use pin_project::pin_project;
use rust_embed::{Embed, EmbeddedFile};
use tokio::{
    fs,
    io::{AsyncSeekExt, BufReader},
    sync::Notify,
};
use tokio_util::io::ReaderStream;
use tracing::debug;

use crate::{Console, Runnable, error::Error};

#[derive(Embed)]
#[folder = "../../resources"]
struct Resources;

#[pin_project]
struct StreamLimiter<S> {
    remaining: Option<u64>,
    #[pin]
    inner: S,
}

impl<S> StreamLimiter<S> {
    fn new(stream: S, limit: Option<u64>) -> Self {
        Self {
            remaining: limit,
            inner: stream,
        }
    }
}

impl<S> Stream for StreamLimiter<S>
where
    S: Stream<Item = Result<Bytes, io::Error>>,
{
    type Item = Result<Bytes, io::Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if let Some(remaining) = self.remaining {
            if remaining == 0 {
                return Poll::Ready(None);
            }
        }

        let this = self.project();

        match this.inner.poll_next(cx) {
            Poll::Ready(Some(Ok(mut bytes))) => {
                if let Some(limit) = this.remaining.take() {
                    if limit < bytes.len() as u64 {
                        bytes.truncate(limit as usize);
                        *this.remaining = Some(0);
                    } else {
                        *this.remaining = Some(limit - bytes.len() as u64);
                    }
                }

                Poll::Ready(Some(Ok(bytes)))
            }
            o => o,
        }
    }
}

#[pin_project]
struct EmbeddedFileStream {
    position: usize,
    file: EmbeddedFile,
}

impl EmbeddedFileStream {
    fn new(file: EmbeddedFile) -> Self {
        Self { file, position: 0 }
    }
}

impl Stream for EmbeddedFileStream {
    type Item = Result<Bytes, io::Error>;

    fn poll_next(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.project();

        if *this.position >= this.file.data.len() {
            Poll::Ready(None)
        } else {
            let bytes = Bytes::copy_from_slice(&this.file.data);
            *this.position = this.file.data.len();
            Poll::Ready(Some(Ok(bytes)))
        }
    }
}

#[pin_project(project = EitherProj)]
enum EitherStream<A, B> {
    A(#[pin] A),
    B(#[pin] B),
}

impl<A, B, C> Stream for EitherStream<A, B>
where
    A: Stream<Item = C>,
    B: Stream<Item = C>,
{
    type Item = C;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.project() {
            EitherProj::A(s) => s.poll_next(cx),
            EitherProj::B(s) => s.poll_next(cx),
        }
    }
}

// Object ID forms and hierarchy:
//
// 0                         - root
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

fn uniform_title(obj: &Object) -> String {
    let title = match obj {
        Object::Container(o) => o.title.to_lowercase(),
        Object::Item(o) => o.title.to_lowercase(),
    };

    let mut trimmed = title.trim().trim_start_matches("a ").trim();
    trimmed = trimmed.trim().trim_start_matches("the ").trim();

    trimmed.to_string()
}

fn sort_by_title(a: &Object, b: &Object) -> Ordering {
    uniform_title(a).cmp(&uniform_title(b))
}

async fn icon_resource(root: PathBuf, path: Option<PathBuf>) -> Option<Icon> {
    let path = path?;

    let format = FileFormat::from_file(&path).ok()?;
    let mime_type = Mime::from_str(format.media_type()).ok()?;

    let (width, height) = image_dimensions(&path).ok()?;

    Some(Icon {
        id: format!("thumbnail/{}", diff_paths(&path, root).unwrap().display()),
        mime_type,
        width,
        height,
        depth: 24,
    })
}

async fn file_resource(video_part: &VideoPart, path: Option<PathBuf>) -> Option<Resource> {
    let path = path?;

    let format = FileFormat::from_file(&path).ok()?;
    let mime_type = Mime::from_str(format.media_type()).ok()?;
    let metadata = fs::metadata(&path).await.ok()?;
    let video = video_part.video().await;
    let root = video.flick_sync().root().await;

    Some(Resource {
        id: format!("video/{}", diff_paths(&path, root).unwrap().display()),
        mime_type,
        duration: Some(video_part.duration().await),
        size: Some(metadata.size()),
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

        let mut resources = Vec::new();
        if let Some(resource) = file_resource(&self, self.file().await).await {
            resources.push(resource);
        }

        let (id, title, parent_id) = if parts.len() == 1 {
            (
                format!("{}/V:{}", video.server().id(), video.id()),
                video.title().await,
                video_parent(&video).await,
            )
        } else {
            (
                format!("{}/VP:{}/{}", video.server().id(), video.id(), self.index()),
                format!("{} - Pt {}", video.title().await, self.index() + 1),
                format!("{}/V:{}", video.server().id(), video.id()),
            )
        };

        Object::Item(Item {
            thumbnail: icon_resource(
                video.flick_sync().root().await,
                video.thumbnail_file().await,
            )
            .await,
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
            Object::Container(Container {
                id: format!("{}/V:{}", self.server().id(), self.id()),
                parent_id: video_parent(&self).await,
                title: self.title().await,
                child_count: Some(parts.len()),
                thumbnail: icon_resource(
                    self.flick_sync().root().await,
                    self.thumbnail_file().await,
                )
                .await,
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
        Object::Container(Container {
            id: format!("{}/P:{}", self.server().id(), self.id()),
            parent_id: "P".to_string(),
            child_count: Some(self.videos().await.len()),
            title: self.title().await,
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
        Object::Container(Container {
            id: format!("{}/C:{}", self.server().id(), self.id()),
            parent_id: "C".to_string(),
            child_count: Some(self.movies().await.len()),
            title: self.title().await,
            thumbnail: icon_resource(self.flick_sync().root().await, self.thumbnail_file().await)
                .await,
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
        Object::Container(Container {
            id: format!("{}/C:{}", self.server().id(), self.id()),
            parent_id: "C".to_string(),
            child_count: Some(self.shows().await.len()),
            title: self.title().await,
            thumbnail: icon_resource(self.flick_sync().root().await, self.thumbnail_file().await)
                .await,
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

        Object::Container(Container {
            id: format!("{}/S:{}", self.server().id(), self.id()),
            parent_id: format!("{}/L:{}", library.server().id(), library.id()),
            child_count: Some(self.seasons().await.len()),
            title: self.title().await,
            thumbnail: icon_resource(self.flick_sync().root().await, self.thumbnail_file().await)
                .await,
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

        Object::Container(Container {
            id: format!("{}/N:{}", self.server().id(), self.id()),
            parent_id: format!("{}/S:{}", show.server().id(), show.id()),
            child_count: Some(self.episodes().await.len()),
            title: self.title().await,
            thumbnail: icon_resource(self.flick_sync().root().await, show.thumbnail_file().await)
                .await,
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

struct DlnaHandler {
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
    ) -> Result<
        StreamResponse<EitherStream<EmbeddedFileStream, ReaderStream<BufReader<fs::File>>>>,
        UpnpError,
    > {
        if let Some(path) = icon_id.strip_prefix("thumbnail/") {
            let target = self.flick_sync.root().await.join(path);

            let Ok(format) = FileFormat::from_file(&target) else {
                return Err(UpnpError::unknown_object());
            };

            let Ok(mime_type) = Mime::from_str(format.media_type()) else {
                return Err(UpnpError::unknown_object());
            };

            let Ok(metadata) = fs::metadata(&target).await else {
                return Err(UpnpError::unknown_object());
            };

            let Ok(file) = fs::File::open(target).await else {
                return Err(UpnpError::unknown_object());
            };

            Ok(StreamResponse {
                mime_type,
                range: None,
                resource_size: Some(metadata.size()),
                stream: EitherStream::B(ReaderStream::new(BufReader::new(file))),
            })
        } else if let Some(resource) = icon_id.strip_prefix("resource/") {
            let Some(icon_file) = Resources::get(resource) else {
                return Err(UpnpError::unknown_object());
            };

            Ok(StreamResponse {
                mime_type: mime::IMAGE_PNG,
                range: None,
                resource_size: Some(icon_file.data.len() as u64),
                stream: EitherStream::A(EmbeddedFileStream::new(icon_file)),
            })
        } else {
            return Err(UpnpError::unknown_object());
        }
    }

    async fn get_resource(&self, resource_id: &str) -> Result<Resource, UpnpError> {
        let Some(path) = resource_id.strip_prefix("video/") else {
            return Err(UpnpError::unknown_object());
        };

        let target = self.flick_sync.root().await.join(path);

        let Ok(format) = FileFormat::from_file(&target) else {
            return Err(UpnpError::unknown_object());
        };

        let Ok(mime_type) = Mime::from_str(format.media_type()) else {
            return Err(UpnpError::unknown_object());
        };

        let Ok(metadata) = fs::metadata(&target).await else {
            return Err(UpnpError::unknown_object());
        };

        Ok(Resource {
            id: resource_id.to_owned(),
            mime_type,
            size: Some(metadata.size()),
            seekable: true,
            duration: None,
        })
    }

    async fn stream_resource(
        &self,
        resource_id: &str,
        seek: u64,
        length: Option<u64>,
    ) -> Result<StreamResponse<StreamLimiter<ReaderStream<BufReader<fs::File>>>>, UpnpError> {
        let Some(path) = resource_id.strip_prefix("video/") else {
            debug!("Bad prefix");
            return Err(UpnpError::unknown_object());
        };

        let target = self.flick_sync.root().await.join(path);

        let Ok(format) = FileFormat::from_file(&target) else {
            debug!("Bad format");
            return Err(UpnpError::unknown_object());
        };

        let Ok(mime_type) = Mime::from_str(format.media_type()) else {
            debug!("Bad mime");
            return Err(UpnpError::unknown_object());
        };

        let Ok(metadata) = fs::metadata(&target).await else {
            debug!("Bad meta");
            return Err(UpnpError::unknown_object());
        };

        let Ok(mut file) = fs::File::open(target).await else {
            debug!("Bad open");
            return Err(UpnpError::unknown_object());
        };

        let range = match (seek, length) {
            (0, None) => None,
            (start, None) => {
                if file.seek(SeekFrom::Start(start)).await.is_ok() {
                    Some(Range {
                        start,
                        length: metadata.size() - start,
                    })
                } else {
                    None
                }
            }
            (0, Some(length)) => Some(Range { start: 0, length }),
            (start, Some(length)) => {
                if file.seek(SeekFrom::Start(start)).await.is_ok() {
                    Some(Range { start, length })
                } else {
                    None
                }
            }
        };

        let limit = range.as_ref().map(|r| r.length);

        Ok(StreamResponse {
            mime_type,
            range,
            resource_size: Some(metadata.size()),
            stream: StreamLimiter::new(ReaderStream::new(BufReader::new(file)), limit),
        })
    }
}

#[derive(Args)]
pub struct Dlna {}

impl Runnable for Dlna {
    async fn run(self, flick_sync: FlickSync, _console: Console) -> Result<(), Error> {
        let uuid = flick_sync.client_id().await;
        let handler = DlnaHandler { flick_sync };

        let server = DlnaServer::builder(handler)
            .uuid(uuid)
            .server_version(&format!(
                "FlickSync/{}.{}",
                env!("CARGO_PKG_VERSION_MAJOR"),
                env!("CARGO_PKG_VERSION_MINOR")
            ))
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
            .bind(Ipv4Addr::UNSPECIFIED, 1980)
            .build()
            .await?;

        let notify = Arc::new(Notify::new());

        let handler_notify = notify.clone();
        ctrlc::set_handler(move || {
            handler_notify.notify_one();
        })
        .unwrap();

        notify.notified().await;

        server.shutdown().await;

        Ok(())
    }
}
