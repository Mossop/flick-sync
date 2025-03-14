use std::net::Ipv4Addr;

use actix_web::{App, HttpServer};
use clap::Args;
use flick_sync::FlickSync;
use futures::{StreamExt, select};
use tokio::{
    signal::unix::{SignalKind, signal},
    spawn,
};
use tokio_stream::wrappers::SignalStream;

use crate::{Console, Runnable, dlna::build_dlna, error::Error};

#[derive(Args)]
pub struct Serve {
    /// The port to use for the web server.
    #[clap(short, long)]
    port: Option<u16>,
}

impl Runnable for Serve {
    async fn run(self, flick_sync: FlickSync, console: Console) -> Result<(), Error> {
        let port = self.port.unwrap_or(80);

        let (dlna_server, service_factory) = build_dlna(flick_sync.clone(), console, port).await?;

        let http_server = HttpServer::new(move || App::new().service(service_factory.clone()))
            .bind((Ipv4Addr::UNSPECIFIED, port))?
            .run();

        let http_handle = http_server.handle();

        spawn(http_server);

        let mut sighup = SignalStream::new(signal(SignalKind::hangup()).unwrap()).fuse();
        let mut sigint = SignalStream::new(signal(SignalKind::interrupt()).unwrap()).fuse();
        let mut sigterm = SignalStream::new(signal(SignalKind::interrupt()).unwrap()).fuse();

        loop {
            select! {
                _ = sighup.next() => dlna_server.restart(),
                _ = sigint.next() => break,
                _ = sigterm.next() => break,
            }
        }

        http_handle.stop(true).await;
        dlna_server.shutdown().await;

        Ok(())
    }
}
