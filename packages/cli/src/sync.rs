use async_trait::async_trait;
use clap::Args;
use flick_sync::{FlickSync, Progress, VideoPart};
use futures::future::join_all;
use tokio::sync::Semaphore;
use tracing::{error, warn};

use crate::{console::Bar, select_servers, Console, Result, Runnable};

#[derive(Args)]
pub struct Prune {
    /// The servers to prune. Can be repeated. When not passed all servers and
    /// the top level directory are pruned.
    #[clap(short = 's', long = "server")]
    ids: Vec<String>,
}

#[async_trait]
impl Runnable for Prune {
    async fn run(self, _flick_sync: FlickSync, _console: Console) -> Result {
        todo!();
    }
}

#[derive(Clone)]
struct SyncState<'a> {
    download_permits: &'a Semaphore,
}

struct DownloadProgress {
    bar: Bar,
}

impl Progress for DownloadProgress {
    fn progress(&mut self, position: u64, size: u64) {
        self.bar.set_position(position);
        self.bar.set_length(size);
    }
}

async fn prepare_download_part(part: &VideoPart) -> Result {
    if let Err(e) = part.verify_download().await {
        warn!("{e}");
    }

    if part.is_downloaded().await {
        return Ok(());
    }

    part.prepare_download().await?;

    Ok(())
}

async fn download_part(
    title: String,
    sync_state: SyncState<'_>,
    part: VideoPart,
    console: Console,
) {
    if let Err(e) = part.wait_for_download().await {
        error!(error=?e);
        return;
    }

    let _permit = sync_state.download_permits.acquire().await.unwrap();

    let bar = console.add_progress_bar(&title);
    if let Err(e) = part.download(DownloadProgress { bar: bar.clone() }).await {
        error!(error=?e);
    }

    bar.finish();
}

#[derive(Args)]
pub struct Sync {
    /// The servers to sync. Can be repeated. When not passed all servers are listed.
    #[clap(short = 's', long = "server")]
    ids: Vec<String>,
}

#[async_trait]
impl Runnable for Sync {
    async fn run(self, flick_sync: FlickSync, console: Console) -> Result {
        let servers = select_servers(&flick_sync, &self.ids).await?;

        let download_permits = Semaphore::new(5);
        let state = SyncState {
            download_permits: &download_permits,
        };

        let mut jobs = Vec::new();

        for server in servers {
            for video in server.videos().await {
                let title = video.title().await;
                for part in video.parts().await {
                    if let Err(e) = prepare_download_part(&part).await {
                        error!(error=?e);
                        continue;
                    }

                    jobs.push(download_part(
                        title.clone(),
                        state.clone(),
                        part,
                        console.clone(),
                    ));
                }
            }
        }

        join_all(jobs).await;

        Ok(())
    }
}
