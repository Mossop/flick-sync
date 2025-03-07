use std::{net::Ipv4Addr, sync::Arc};

use async_trait::async_trait;
use clap::Args;
use dlna_server::{Container, DlnaRequestHandler, DlnaServer, Item, Object, Resource, UpnpError};
use flick_sync::{
    Collection, FlickSync, Library, MovieCollection, MovieLibrary, Playlist, Season, Server, Show,
    ShowCollection, ShowLibrary, Video, VideoPart,
};
use std::str::FromStr;
use tokio::sync::Notify;

use crate::{Console, Runnable, error::Error};

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

trait ToObject
where
    Self: Sized,
{
    type Children: ToObject;

    async fn to_object(self) -> Object;
    async fn to_children(self) -> Vec<Self::Children>;

    async fn collect_children(self) -> Vec<Object> {
        let mut result = Vec::new();

        for child in self.to_children().await {
            result.push(child.to_object().await);
        }

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

        let (title, parent_id) = if parts.len() == 1 {
            (video.title().await, video_parent(&video).await)
        } else {
            (
                format!("{} - Pt {}", video.title().await, self.index() + 1),
                format!("{}/V:{}", video.server().id(), video.id()),
            )
        };

        Object::Item(Item {
            id: format!("{}/VP:{}/{}", video.server().id(), video.id(), self.index()),
            parent_id,
            title,
            resources: vec![Resource {
                id: video.id().to_owned(),
                mime_type: "video/mp4".parse().unwrap(),
                duration: Some(self.duration().await),
                size: None,
                seekable: true,
            }],
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
            })
        }
    }

    async fn to_children(self) -> Vec<Self::Children> {
        let parts = self.parts().await;

        if parts.len() == 1 { Vec::new() } else { parts }
    }
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
}

impl ToObject for MovieCollection {
    type Children = Video;

    async fn to_object(self) -> Object {
        Object::Container(Container {
            id: format!("{}/C:{}", self.server().id(), self.id()),
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
}

impl ToObject for ShowCollection {
    type Children = Show;

    async fn to_object(self) -> Object {
        Object::Container(Container {
            id: format!("{}/C:{}", self.server().id(), self.id()),
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
}

impl ToObject for MovieLibrary {
    type Children = Video;

    async fn to_object(self) -> Object {
        Object::Container(Container {
            id: format!("{}/L:{}", self.server().id(), self.id()),
            parent_id: "L".to_string(),
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
}

impl ToObject for ShowLibrary {
    type Children = Show;

    async fn to_object(self) -> Object {
        Object::Container(Container {
            id: format!("{}/L:{}", self.server().id(), self.id()),
            parent_id: "L".to_string(),
            child_count: Some(self.shows().await.len()),
            title: self.title().await,
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
        })
    }

    async fn to_children(self) -> Vec<Self::Children> {
        self.seasons().await
    }
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
