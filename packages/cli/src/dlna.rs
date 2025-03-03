use std::{net::Ipv4Addr, sync::Arc};

use clap::Args;
use dlna_server::{DlnaRequestHandler, DlnaServer};
use flick_sync::FlickSync;
use tokio::sync::Notify;

use crate::{Console, Result, Runnable};

struct DlnaHandler {
    console: Console,
    flick_sync: FlickSync,
}

impl DlnaRequestHandler for DlnaHandler {}

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
