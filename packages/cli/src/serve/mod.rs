use std::{
    collections::{HashMap, VecDeque},
    net::Ipv4Addr,
    sync::{Arc, Mutex},
    time::Duration,
};

use actix_web::{
    App, HttpServer,
    middleware::from_fn,
    web::{Data, ThinData},
};
use clap::Args;
use flick_sync::{DownloadProgress, FlickSync, Progress, Server, VideoPart};
use futures::{StreamExt, select};
use tokio::{
    signal::unix::{SignalKind, signal},
    sync::broadcast,
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
}

#[derive(Default)]
struct SyncStatus {
    is_syncing: bool,
    log: VecDeque<SyncLogItem>,
    progress: HashMap<String, SyncProgressBar>,
}

struct SyncProgress {
    is_download: bool,
    video_part: VideoPart,
    task: SyncTask,
    position: u64,
    length: Option<u64>,
}

impl SyncProgress {
    fn new(task: SyncTask, video_part: VideoPart, is_download: bool) -> Self {
        let this = Self {
            task,
            video_part,
            is_download,
            position: 0,
            length: if is_download { None } else { Some(100) },
        };

        this.task.add_progress(&this);

        this
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
        } else {
            self.position = position;
        }
    }

    fn length(&mut self, length: u64) {
        self.length = Some(length);
        self.task.update_progress(self);
    }

    fn finished(self) {
        self.task.remove_progress(&self);

        if self.is_download {
            self.task
                .log(SyncLogMessage::DownloadComplete(self.video_part));
        } else {
            self.task
                .log(SyncLogMessage::TranscodeComplete(self.video_part));
        }
    }

    fn failed(self, error: anyhow::Error) {
        self.task.remove_progress(&self);

        if self.is_download {
            self.task.log(SyncLogMessage::DownloadFailed((
                self.video_part,
                error.to_string(),
            )));
        } else {
            self.task.log(SyncLogMessage::TranscodeFailed((
                self.video_part,
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
        let this = Self {
            event_sender,
            status,
        };
        this.send_event(Event::SyncStart);
        this
    }

    fn add_progress(&self, progress: &SyncProgress) {
        let mut status = self.status.lock().unwrap();

        status.progress.insert(
            progress.video_part.id().to_owned(),
            SyncProgressBar {
                is_download: progress.is_download,
                video_part: progress.video_part.clone(),
                position: progress.position,
                length: progress.length,
            },
        );

        self.send_event(Event::Progress(status.progress.values().cloned().collect()));
    }

    fn update_progress(&self, progress: &SyncProgress) {
        let mut status = self.status.lock().unwrap();

        if let Some(bar) = status.progress.get_mut(progress.video_part.id()) {
            bar.position = progress.position;
            bar.length = progress.length;
            self.send_event(Event::Progress(status.progress.values().cloned().collect()));
        }
    }

    fn remove_progress(&self, progress: &SyncProgress) {
        let mut status = self.status.lock().unwrap();

        if status.progress.remove(progress.video_part.id()).is_some() {
            self.send_event(Event::Progress(status.progress.values().cloned().collect()));
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

    fn sync_started(&self, server: Server) {
        self.log(SyncLogMessage::SyncStarted(server));
    }

    fn sync_failed(&self, server: Server, error: anyhow::Error) {
        self.log(SyncLogMessage::SyncFailed((server, error.to_string())));
    }

    fn sync_finished(&self, server: Server, complete: bool) {
        self.log(SyncLogMessage::SyncFinished((server, complete)));
    }
}

impl DownloadProgress for SyncTask {
    async fn transcode_started(&self, video_part: &VideoPart) -> impl Progress {
        self.log(SyncLogMessage::TranscodeStarted(video_part.clone()));

        SyncProgress::new(self.clone(), video_part.clone(), false)
    }

    async fn download_started(&self, video_part: &VideoPart) -> impl Progress {
        self.log(SyncLogMessage::DownloadStarted(video_part.clone()));

        SyncProgress::new(self.clone(), video_part.clone(), true)
    }

    async fn download_failed(&self, video_part: &VideoPart, error: anyhow::Error) {
        self.log(SyncLogMessage::DownloadFailed((
            video_part.clone(),
            error.to_string(),
        )));
    }
}

async fn background_task(
    flick_sync: FlickSync,
    status: Arc<Mutex<SyncStatus>>,
    event_sender: broadcast::Sender<Event>,
) {
    loop {
        status.lock().unwrap().is_syncing = true;
        let task = SyncTask::new(status.clone(), event_sender.clone());

        flick_sync.prune_root().await;

        for server in flick_sync.servers().await {
            task.sync_started(server.clone());

            if let Err(e) = server.update_state().await {
                warn!(server=server.id(), error=?e, "Failed to update server");
                task.sync_failed(server, e);
                continue;
            }

            if let Err(e) = server.prune().await {
                warn!(server=server.id(), error=?e, "Failed to prune server directory");
            }

            match server.download(task.clone()).await {
                Ok(complete) => {
                    server.write_playlists().await;
                    task.sync_finished(server, complete);
                }
                Err(e) => task.sync_failed(server, e),
            }
        }

        status.lock().unwrap().is_syncing = false;
        task.send_event(Event::SyncEnd);

        time::sleep(Duration::from_secs(30 * 60)).await;
    }
}

impl Runnable for Serve {
    async fn run(self, flick_sync: FlickSync, _console: Console) -> Result {
        let port = self.port.unwrap_or(80);

        let (dlna_server, service_factory) = build_dlna(flick_sync.clone(), port).await?;

        let (event_sender, _) = broadcast::channel::<Event>(20);

        let status: Arc<Mutex<SyncStatus>> = Default::default();

        let background_task = tokio::spawn(background_task(
            flick_sync.clone(),
            status.clone(),
            event_sender.clone(),
        ));

        let status = Data::from(status);

        let http_server = HttpServer::new(move || {
            App::new()
                .app_data(ThinData(flick_sync.clone()))
                .app_data(ThinData(event_sender.clone()))
                .app_data(status.clone())
                .service(service_factory.clone())
                .wrap(from_fn(middleware::middleware))
                .service(services::events)
                .service(services::resources)
                .service(services::thumbnail)
                .service(services::playlist_contents)
                .service(services::library_collections)
                .service(services::collection_contents)
                .service(services::show_contents)
                .service(services::season_contents)
                .service(services::video_page)
                .service(services::library_contents)
                .service(services::sync_list)
                .service(services::index_page)
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
