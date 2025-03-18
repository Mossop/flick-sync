use std::net::Ipv4Addr;

use actix_web::{App, HttpServer, middleware::from_fn, web::ThinData};
use clap::Args;
use flick_sync::FlickSync;
use futures::{StreamExt, select};
use tokio::signal::unix::{SignalKind, signal};
use tokio_stream::wrappers::SignalStream;

use crate::{Console, Runnable, dlna::build_dlna, error::Error};

mod middleware;
mod services;

#[derive(Args)]
pub struct Serve {
    /// The port to use for the web server.
    #[clap(short, long, env = "FLICK_SYNC_PORT")]
    port: Option<u16>,
}

impl Runnable for Serve {
    async fn run(self, flick_sync: FlickSync, console: Console) -> Result<(), Error> {
        let port = self.port.unwrap_or(80);

        let (dlna_server, service_factory) = build_dlna(flick_sync.clone(), console, port).await?;

        let http_server = HttpServer::new(move || {
            App::new()
                .app_data(ThinData(flick_sync.clone()))
                .service(service_factory.clone())
                .wrap(from_fn(middleware::middleware))
                .service(services::resources)
                .service(services::thumbnail)
                .service(services::playlist_list)
                .service(services::library_list)
                .service(services::collection_list)
                .service(services::index)
        })
        .bind((Ipv4Addr::UNSPECIFIED, port))?
        .run();

        let http_handle = http_server.handle();

        tokio::spawn(http_server);

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
