use std::{net::Ipv4Addr, sync::Arc};

use async_trait::async_trait;
use clap::Args;
use dlna_server::{Container, DlnaRequestHandler, DlnaServer, Object};
use flick_sync::FlickSync;
use tokio::sync::Notify;

use crate::{Console, Result, Runnable};

struct DlnaHandler {
    console: Console,
    flick_sync: FlickSync,
}

#[async_trait]
impl DlnaRequestHandler for DlnaHandler {
    async fn list_children(&self, parent_id: &str) -> Vec<Object> {
        if parent_id == "0" {
            vec![
                Object::Container(Container {
                    id: "L".to_string(),
                    parent_id: "0".to_string(),
                    child_count: None,
                    title: "Libraries".to_string(),
                }),
                Object::Container(Container {
                    id: "P".to_string(),
                    parent_id: "0".to_string(),
                    child_count: None,
                    title: "Playlists".to_string(),
                }),
            ]
        } else {
            Vec::new()
        }
    }
}

#[derive(Args)]
pub struct Dlna {}

impl Runnable for Dlna {
    async fn run(self, flick_sync: FlickSync, console: Console) -> Result {
        let uuid = flick_sync.client_id().await;
        let handler = DlnaHandler {
            console,
            flick_sync,
        };

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
