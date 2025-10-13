use std::{
    any::Any,
    collections::{HashMap, VecDeque},
    net::{Ipv4Addr, SocketAddr},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use actix_tls::accept::rustls_0_23::TlsStream;
use actix_web::{
    App, HttpServer, dev::Extensions, middleware::from_fn, rt::net::TcpStream, web::ThinData,
};
use clap::{Args, builder::FalseyValueParser};
use flick_sync::{DownloadProgress, FlickSync, Progress, Server, Video};
use futures::{FutureExt, StreamExt, select};
use rustls::{
    ServerConfig,
    pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject},
};
use tokio::{
    signal::unix::{SignalKind, signal},
    sync::{Notify, broadcast},
    time,
};
use tokio_stream::wrappers::SignalStream;
use tracing::warn;

use crate::{
    Console, Result, Runnable,
    dlna::build_dlna,
    serve::events::{Event, SyncLogItem, SyncLogMessage, SyncProgressBar},
};

mod events;
mod middleware;
mod services;

const LOG_LIMIT: usize = 20;

#[derive(Args)]
pub struct Serve {
    /// The port to use for the web server.
    #[clap(short, long, env = "FLICK_SYNC_PORT")]
    port: Option<u16>,
    #[clap(short, long, env = "FLICK_SYNC_CERTIFICATE")]
    certificate: Option<String>,
    #[clap(short = 'k', long, env = "FLICK_SYNC_PRIVATE_KEY")]
    private_key: Option<String>,
    #[clap(short, long, env = "FLICK_SYNC_DISABLE_SYNCING", value_parser = FalseyValueParser::new())]
    disable_syncing: bool,
}

#[derive(Default)]
struct SyncStatus {
    is_syncing: bool,
    log: VecDeque<SyncLogItem>,
    progress: HashMap<String, SyncProgressBar>,
}

struct SyncProgress {
    is_download: bool,
    video: Video,
    task: SyncTask,
    position: u64,
    length: Option<u64>,
    ref_count: Arc<AtomicUsize>,
}

impl SyncProgress {
    fn new(task: SyncTask, video: Video, is_download: bool) -> Self {
        let this = Self {
            task,
            video,
            is_download,
            position: 0,
            length: if is_download { None } else { Some(100) },
            ref_count: Arc::new(AtomicUsize::new(1)),
        };

        this.task.add_progress(&this);

        this
    }
}

impl Clone for SyncProgress {
    fn clone(&self) -> Self {
        self.ref_count.fetch_add(1, Ordering::SeqCst);

        Self {
            is_download: self.is_download,
            video: self.video.clone(),
            task: self.task.clone(),
            position: self.position,
            length: self.length,
            ref_count: self.ref_count.clone(),
        }
    }
}

impl Drop for SyncProgress {
    fn drop(&mut self) {
        if self.ref_count.fetch_sub(1, Ordering::SeqCst) == 1 {
            self.task.remove_progress(self);
        }
    }
}

impl Progress for SyncProgress {
    fn progress(&mut self, position: u64) {
        if let Some(length) = self.length {
            let current = (100 * self.position) / length;
            let new = (100 * position) / length;

            if new > current {
                self.position = position;
                self.task.update_progress(self);
            }
        } else if position.abs_diff(self.position) > 1024 * 1024 {
            self.position = position;
            self.task.update_progress(self);
        }
    }

    fn length(&mut self, length: u64) {
        self.task.update_length(self, length);
    }

    fn finished(self) {
        self.task.remove_progress(&self);

        if self.is_download {
            self.task
                .log(SyncLogMessage::DownloadComplete(self.video.clone()));
        } else {
            self.task
                .log(SyncLogMessage::TranscodeComplete(self.video.clone()));
        }
    }

    fn failed(self, error: anyhow::Error) {
        self.task.remove_progress(&self);

        if self.is_download {
            self.task.log(SyncLogMessage::DownloadFailed((
                self.video.clone(),
                error.to_string(),
            )));
        } else {
            self.task.log(SyncLogMessage::TranscodeFailed((
                self.video.clone(),
                error.to_string(),
            )));
        }
    }
}

#[derive(Clone)]
struct SyncTask {
    event_sender: broadcast::Sender<Event>,
    status: Arc<Mutex<SyncStatus>>,
}

impl SyncTask {
    fn new(status: Arc<Mutex<SyncStatus>>, event_sender: broadcast::Sender<Event>) -> Self {
        Self {
            event_sender,
            status,
        }
    }

    fn progress_key(progress: &SyncProgress) -> String {
        format!(
            "{}:{}",
            progress.video.id(),
            if progress.is_download { "D" } else { "T" }
        )
    }

    fn update_bars<'a, I: Iterator<Item = &'a SyncProgressBar>>(&self, bars: I) {
        self.send_event(Event::Progress(
            bars.filter_map(|b| {
                if b.length.is_some() {
                    Some(b.clone())
                } else {
                    None
                }
            })
            .collect(),
        ));
    }

    fn add_progress(&self, progress: &SyncProgress) {
        let mut status = self.status.lock().unwrap();

        status.progress.insert(
            Self::progress_key(progress),
            SyncProgressBar {
                is_download: progress.is_download,
                video: progress.video.clone(),
                position: progress.position,
                length: progress.length,
            },
        );

        self.update_bars(status.progress.values());
    }

    fn update_length(&self, progress: &SyncProgress, length: u64) {
        let mut status = self.status.lock().unwrap();

        if let Some(bar) = status.progress.get_mut(&Self::progress_key(progress)) {
            bar.length = Some(length);
            self.update_bars(status.progress.values());
        }
    }

    fn update_progress(&self, progress: &SyncProgress) {
        let mut status = self.status.lock().unwrap();

        if let Some(bar) = status.progress.get_mut(&Self::progress_key(progress)) {
            bar.position = progress.position;
            self.update_bars(status.progress.values());
        }
    }

    fn remove_progress(&self, progress: &SyncProgress) {
        let mut status = self.status.lock().unwrap();

        if status
            .progress
            .remove(&Self::progress_key(progress))
            .is_some()
        {
            self.update_bars(status.progress.values());
        }
    }

    fn log(&self, message: SyncLogMessage) {
        let log_item: SyncLogItem = message.into();

        let mut status = self.status.lock().unwrap();
        status.log.push_back(log_item.clone());
        while status.log.len() > LOG_LIMIT {
            status.log.pop_front();
        }

        self.send_event(Event::Log(log_item));
    }

    fn send_event(&self, event: Event) {
        let _ = self.event_sender.send(event);
    }

    async fn sync_started(&self, server: Server) {
        self.log(SyncLogMessage::SyncStarted(server.name().await));
    }

    async fn sync_failed(&self, server: Server, error: anyhow::Error) {
        self.log(SyncLogMessage::SyncFailed((
            server.name().await,
            error.to_string(),
        )));
    }

    async fn sync_finished(&self, server: Server, complete: bool) {
        self.log(SyncLogMessage::SyncFinished((
            server.name().await,
            complete,
        )));
    }
}

impl DownloadProgress for SyncTask {
    async fn transcode_started(&self, video: &Video) -> impl Progress + Clone + 'static {
        self.log(SyncLogMessage::TranscodeStarted(video.clone()));

        SyncProgress::new(self.clone(), video.clone(), false)
    }

    async fn download_started(&self, video: &Video) -> impl Progress + Clone + 'static {
        self.log(SyncLogMessage::DownloadStarted(video.clone()));

        SyncProgress::new(self.clone(), video.clone(), true)
    }

    async fn download_failed(&self, video: &Video, error: anyhow::Error) {
        self.log(SyncLogMessage::DownloadFailed((
            video.clone(),
            error.to_string(),
        )));
    }
}

async fn background_task(
    flick_sync: FlickSync,
    status: Arc<Mutex<SyncStatus>>,
    event_sender: broadcast::Sender<Event>,
    wakeup: Arc<Notify>,
    full_sync: bool,
) {
    select! {
        _ = time::sleep(Duration::from_secs(30)).fuse() => {},
        _ = wakeup.notified().fuse() => {},
    }

    loop {
        status.lock().unwrap().is_syncing = true;
        let _ = event_sender.send(Event::SyncStart);
        let task = SyncTask::new(status.clone(), event_sender.clone());

        flick_sync.prune_root().await;

        for server in flick_sync.servers().await {
            task.sync_started(server.clone()).await;

            if let Err(e) = server.update_state(full_sync).await {
                warn!(server=server.id(), error=?e, "Failed to update server");
                task.sync_failed(server, e).await;
                continue;
            }

            task.send_event(Event::SyncChange);

            if let Err(e) = server.prune().await {
                warn!(server=server.id(), error=?e, "Failed to prune server directory");
            }

            if full_sync {
                match server.download(task.clone()).await {
                    Ok(complete) => {
                        server.write_playlists().await;
                        task.sync_finished(server, complete).await;
                    }
                    Err(e) => task.sync_failed(server, e).await,
                }
            } else {
                task.sync_finished(server, false).await;
            }
        }

        status.lock().unwrap().is_syncing = false;
        task.send_event(Event::SyncChange);
        let _ = event_sender.send(Event::SyncEnd);

        select! {
            _ = time::sleep(Duration::from_secs(30 * 60)).fuse() => {},
            _ = wakeup.notified().fuse() => {},
        }
    }
}

struct ConnectionInfo {
    local_addr: SocketAddr,
}

fn on_connect(connection: &dyn Any, data: &mut Extensions) {
    if let Some(stream) = connection.downcast_ref::<TcpStream>() {
        if let Ok(addr) = stream.local_addr() {
            data.insert(ConnectionInfo { local_addr: addr });
        }
    } else if let Some(tls) = connection.downcast_ref::<TlsStream<TcpStream>>() {
        let (stream, _) = tls.get_ref();

        if let Ok(addr) = stream.local_addr() {
            data.insert(ConnectionInfo { local_addr: addr });
        }
    }
}

#[derive(Clone)]
struct ServiceData {
    flick_sync: FlickSync,
    http_port: u16,
    status: Arc<Mutex<SyncStatus>>,
    event_sender: broadcast::Sender<Event>,
    sync_trigger: Arc<Notify>,
}

impl Runnable for Serve {
    async fn run(self, flick_sync: FlickSync, _console: Console) -> Result {
        let mut sighup = SignalStream::new(signal(SignalKind::hangup()).unwrap()).fuse();
        let mut sigint = SignalStream::new(signal(SignalKind::interrupt()).unwrap()).fuse();
        let mut sigterm = SignalStream::new(signal(SignalKind::terminate()).unwrap()).fuse();

        let port = self.port.unwrap_or(80);

        let (dlna_server, service_factory) = build_dlna(flick_sync.clone(), port).await?;

        let (event_sender, _) = broadcast::channel::<Event>(20);

        let status: Arc<Mutex<SyncStatus>> = Default::default();
        let sync_trigger = Arc::new(Notify::new());

        let background_task = if !self.disable_syncing {
            Some(tokio::spawn(background_task(
                flick_sync.clone(),
                status.clone(),
                event_sender.clone(),
                sync_trigger.clone(),
                !self.disable_syncing,
            )))
        } else {
            None
        };

        let service_data = ServiceData {
            flick_sync,
            http_port: port,
            status,
            event_sender,
            sync_trigger,
        };

        let mut http_server = HttpServer::new(move || {
            App::new()
                .app_data(ThinData(service_data.clone()))
                .service(service_factory.clone())
                .wrap(from_fn(middleware::middleware))
                .service(services::events)
                .service(services::resources)
                .service(services::thumbnail_image)
                .service(services::playlist_contents)
                .service(services::library_collections)
                .service(services::collection_contents)
                .service(services::show_contents)
                .service(services::season_contents)
                .service(services::video_stream)
                .service(services::update_playback_position)
                .service(services::video_page)
                .service(services::library_contents)
                .service(services::status_page)
                .service(services::sync_list)
                .service(services::delete_sync)
                .service(services::delete_server)
                .service(services::create_sync)
                .service(services::index_page)
        })
        .on_connect(on_connect)
        .bind((Ipv4Addr::UNSPECIFIED, port))?;

        if let (Some(cert_file), Some(key_file)) = (self.certificate, self.private_key) {
            let certs = CertificateDer::pem_file_iter(cert_file)?
                .map(|cert| cert.unwrap())
                .collect();
            let private_key = PrivateKeyDer::from_pem_file(key_file)?;
            let server_config = ServerConfig::builder()
                .with_no_client_auth()
                .with_single_cert(certs, private_key)
                .unwrap();

            http_server =
                http_server.bind_rustls_0_23((Ipv4Addr::UNSPECIFIED, 443), server_config)?;
        }

        let http_server = http_server.run();

        let http_handle = http_server.handle();

        tokio::spawn(http_server);

        loop {
            select! {
                _ = sighup.next() => dlna_server.restart(),
                _ = sigint.next() => break,
                _ = sigterm.next() => break,
            }
        }

        if let Some(background_task) = background_task {
            background_task.abort();
        }

        http_handle.stop(false).await;
        dlna_server.shutdown().await;

        Ok(())
    }
}
