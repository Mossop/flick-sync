use std::{net::Ipv4Addr, sync::Arc};

use async_trait::async_trait;
use clap::Args;
use dlna_server::{Container, DlnaRequestHandler, DlnaServer, Item, Object, Resource, UpnpError};
use flick_sync::{FlickSync, Library, Playlist, Video};
use tokio::sync::Notify;

use crate::{Console, Runnable, error::Error};

trait ToObject {
    async fn to_object(&self) -> Object;
    async fn to_children(&self) -> Vec<Object>;
}

impl ToObject for Video {
    async fn to_object(&self) -> Object {
        Object::Item(Item {
            id: format!("{}/V:{}", self.server().id(), self.id()),
            parent_id: "P".to_string(),
            title: self.title().await,
            resources: vec![Resource {
                id: self.id().to_owned(),
                mime: "video/mp4".parse().unwrap(),
                duration: None,
                size: None,
            }],
        })
    }

    async fn to_children(&self) -> Vec<Object> {
        Vec::new()
    }
}

impl ToObject for Playlist {
    async fn to_object(&self) -> Object {
        Object::Container(Container {
            id: format!("{}/P:{}", self.server().id(), self.id()),
            parent_id: "P".to_string(),
            child_count: Some(self.videos().await.len()),
            title: self.title().await,
        })
    }

    async fn to_children(&self) -> Vec<Object> {
        let mut result = Vec::new();

        for video in self.videos().await {
            result.push(video.to_object().await);
        }

        result
    }
}

impl ToObject for Library {
    async fn to_object(&self) -> Object {
        Object::Container(Container {
            id: format!("{}/L:{}", self.server().id(), self.id()),
            parent_id: "L".to_string(),
            child_count: Some(2),
            title: self.title().await,
        })
    }

    async fn to_children(&self) -> Vec<Object> {
        let collections = self.collections().await.len();

        let (count, title) = match self {
            Library::Movie(l) => (l.movies().await.len(), "Movies".to_string()),
            Library::Show(l) => (l.shows().await.len(), "Shows".to_string()),
        };

        vec![
            Object::Container(Container {
                id: format!("{}/LL:{}", self.server().id(), self.id()),
                parent_id: format!("{}/L:{}", self.server().id(), self.id()),
                child_count: Some(count),
                title,
            }),
            Object::Container(Container {
                id: format!("{}/LC:{}", self.server().id(), self.id()),
                parent_id: format!("{}/L:{}", self.server().id(), self.id()),
                child_count: Some(collections),
                title: "Collections".to_string(),
            }),
        ]
    }
}

struct DlnaHandler {
    flick_sync: FlickSync,
}

impl DlnaHandler {
    async fn build_results(
        &self,
        object_id: &str,
        is_metadata: bool,
    ) -> Result<Vec<Object>, UpnpError> {
        let Some((server_id, item_id)) = object_id.split_once('/') else {
            return Err(UpnpError::unknown_object());
        };

        let Some(server) = self.flick_sync.server(server_id).await else {
            return Err(UpnpError::unknown_object());
        };

        let Some((item_type, item_id)) = item_id.split_once(':') else {
            return Err(UpnpError::unknown_object());
        };

        match item_type {
            "P" => {
                if let Some(playlist) = server
                    .playlists()
                    .await
                    .into_iter()
                    .find(|pl| pl.id() == item_id)
                {
                    if is_metadata {
                        Ok(vec![playlist.to_object().await])
                    } else {
                        Ok(playlist.to_children().await)
                    }
                } else {
                    Err(UpnpError::unknown_object())
                }
            }
            _ => Err(UpnpError::unknown_object()),
        }
    }
}

#[async_trait]
impl DlnaRequestHandler for DlnaHandler {
    async fn get_object(&self, object_id: &str) -> Result<Object, UpnpError> {
        if object_id == "0" {
            Ok(Object::Container(Container {
                id: "0".to_string(),
                parent_id: "-1".to_string(),
                child_count: Some(2),
                title: "Flick Sync Synced Media".to_string(),
            }))
        } else if object_id == "L" {
            let mut library_count = 0;
            for server in self.flick_sync.servers().await {
                library_count += server.libraries().await.len();
            }

            Ok(Object::Container(Container {
                id: "L".to_string(),
                parent_id: "0".to_string(),
                child_count: Some(library_count),
                title: "Libraries".to_string(),
            }))
        } else if object_id == "P" {
            let mut playlist_count = 0;
            for server in self.flick_sync.servers().await {
                playlist_count += server.playlists().await.len();
            }

            Ok(Object::Container(Container {
                id: "P".to_string(),
                parent_id: "0".to_string(),
                child_count: Some(playlist_count),
                title: "Playlists".to_string(),
            }))
        } else {
            Ok(self
                .build_results(object_id, true)
                .await?
                .into_iter()
                .next()
                .unwrap())
        }
    }

    async fn list_children(&self, parent_id: &str) -> Result<Vec<Object>, UpnpError> {
        if parent_id == "0" {
            Ok(vec![
                self.get_object("L").await.unwrap(),
                self.get_object("P").await.unwrap(),
            ])
        } else if parent_id == "L" {
            let mut results: Vec<Object> = Vec::new();

            for server in self.flick_sync.servers().await {
                for library in server.libraries().await {
                    results.push(library.to_object().await);
                }
            }

            Ok(results)
        } else if parent_id == "P" {
            let mut results: Vec<Object> = Vec::new();

            for server in self.flick_sync.servers().await {
                for playlist in server.playlists().await {
                    results.push(playlist.to_object().await);
                }
            }

            Ok(results)
        } else {
            self.build_results(parent_id, false).await
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
