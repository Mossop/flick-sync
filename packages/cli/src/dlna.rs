use std::sync::Arc;

use clap::Args;
use dlna_server::DlnaServer;
use flick_sync::FlickSync;
use tokio::sync::Notify;

use crate::{Console, Result, Runnable};

#[derive(Args)]
pub struct Dlna {}

impl Runnable for Dlna {
    async fn run(self, flick_sync: FlickSync, console: Console) -> Result {
        let server = DlnaServer::new().await?;
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
