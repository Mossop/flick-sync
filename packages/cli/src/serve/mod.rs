use std::{net::Ipv4Addr, time::Duration};

use actix_web::{App, HttpServer, middleware::from_fn, web::ThinData};
use clap::Args;
use flick_sync::FlickSync;
use futures::{StreamExt, select};
use tokio::{
    signal::unix::{SignalKind, signal},
    sync::broadcast,
    time,
};
use tokio_stream::wrappers::SignalStream;

use crate::{Console, Runnable, dlna::build_dlna, error::Error, serve::events::Event};

mod events;
mod middleware;
mod services;

#[derive(Args)]
pub struct Serve {
    /// The port to use for the web server.
    #[clap(short, long, env = "FLICK_SYNC_PORT")]
    port: Option<u16>,
}

async fn background_task(event_sender: broadcast::Sender<Event>) {
    loop {
        let _ = event_sender.send(Event::SyncStart);
        time::sleep(Duration::from_secs(5)).await;
        let _ = event_sender.send(Event::SyncEnd);

        time::sleep(Duration::from_secs(60 * 5)).await;
    }
}

impl Runnable for Serve {
    async fn run(self, flick_sync: FlickSync, console: Console) -> Result<(), Error> {
        let port = self.port.unwrap_or(80);

        let (dlna_server, service_factory) = build_dlna(flick_sync.clone(), console, port).await?;

        let (event_sender, _) = broadcast::channel::<Event>(20);

        let background_task = tokio::spawn(background_task(event_sender.clone()));

        let http_server = HttpServer::new(move || {
            App::new()
                .app_data(ThinData(flick_sync.clone()))
                .app_data(ThinData(event_sender.clone()))
                .service(service_factory.clone())
                .wrap(from_fn(middleware::middleware))
                .service(services::events)
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

        background_task.abort();
        http_handle.stop(true).await;
        dlna_server.shutdown().await;

        Ok(())
    }
}
