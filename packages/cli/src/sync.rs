use std::sync::Arc;

use async_trait::async_trait;
use clap::Args;
use flick_sync::{FlickSync, Progress, TransferState, VideoPart};
use futures::future::join_all;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tracing::{debug, error, instrument};

use crate::{
    Console, Result, Runnable,
    console::{Bar, ProgressType},
    select_servers,
};

#[derive(Args)]
pub struct Prune {
    /// The servers to prune. Can be repeated. When not passed all servers and
    /// the top level directory are pruned.
    #[clap(short = 's', long = "server")]
    ids: Vec<String>,
}

#[async_trait]
impl Runnable for Prune {
    async fn run(self, flick_sync: FlickSync, _console: Console) -> Result {
        flick_sync.prune_root().await;

        let servers = select_servers(&flick_sync, &self.ids).await?;

        for server in servers {
            if let Err(e) = server.update_state().await {
                error!(server=server.id(), error=?e, "Failed to update server");
                continue;
            }

            if let Err(e) = server.prune().await {
                error!(server=server.id(), error=?e, "Failed to prune server directory");
                continue;
            }
        }

        Ok(())
    }
}

#[derive(Args)]
pub struct BuildMetadata {
    /// The servers to rebuild. Can be repeated. When not passed all servers are
    /// rebuilt
    #[clap(short = 's', long = "server")]
    ids: Vec<String>,
}

#[async_trait]
impl Runnable for BuildMetadata {
    async fn run(self, flick_sync: FlickSync, _console: Console) -> Result {
        let servers = select_servers(&flick_sync, &self.ids).await?;

        for server in servers {
            if let Err(e) = server.rebuild_metadata().await {
                error!(server=server.id(), error=?e, "Failed to rebuild metadata for server");
                continue;
            }
        }

        Ok(())
    }
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

struct PartTransferState {
    transcode_permits: TranscodePermits,
    download_permits: Arc<Semaphore>,
    title: String,
    part: VideoPart,
    console: Console,
}

struct TranscodePermit {
    inner: Option<OwnedSemaphorePermit>,
    forget: bool,
}

impl From<OwnedSemaphorePermit> for TranscodePermit {
    fn from(permit: OwnedSemaphorePermit) -> Self {
        Self {
            inner: Some(permit),
            forget: false,
        }
    }
}

impl TranscodePermit {
    fn forget(permit: OwnedSemaphorePermit) -> Self {
        Self {
            inner: Some(permit),
            forget: true,
        }
    }
}

impl Drop for TranscodePermit {
    fn drop(&mut self) {
        if self.forget {
            if let Some(inner) = self.inner.take() {
                inner.forget();
            }
        }
    }
}

struct TranscodePermits {
    semaphore: Arc<Semaphore>,
    reserved: Option<TranscodePermit>,
}

impl Clone for TranscodePermits {
    fn clone(&self) -> Self {
        Self {
            semaphore: self.semaphore.clone(),
            reserved: None,
        }
    }
}

impl TranscodePermits {
    fn new(count: usize) -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(count)),
            reserved: None,
        }
    }

    /// This is unsafe if other threads attempt to acquire permits at the same time.
    fn reserve(&mut self) {
        if let Ok(permit) = self.semaphore.clone().try_acquire_owned() {
            self.reserved = Some(permit.into());
        } else {
            self.semaphore.add_permits(1);
            self.reserved = Some(TranscodePermit::forget(
                self.semaphore.clone().try_acquire_owned().unwrap(),
            ))
        }
    }

    async fn acquire(&mut self) -> TranscodePermit {
        if let Some(permit) = self.reserved.take() {
            permit
        } else {
            TranscodePermit {
                inner: Some(self.semaphore.clone().acquire_owned().await.unwrap()),
                forget: false,
            }
        }
    }
}

async fn complete_transcode(state: &PartTransferState) -> Result {
    let bar = state
        .console
        .add_progress_bar(&format!("🔄 {}", state.title), ProgressType::Percent);
    state
        .part
        .wait_for_download_to_be_available(DownloadProgress { bar })
        .await?;

    Ok(())
}

async fn complete_download(state: &PartTransferState) -> Result {
    let _permit = state.download_permits.acquire().await.unwrap();

    let bar = state
        .console
        .add_progress_bar(&format!("💾 {}", state.title), ProgressType::Bytes);
    state.part.download(DownloadProgress { bar }).await?;

    Ok(())
}

#[instrument(level = "trace", skip(state), fields(video=state.part.id(), part=state.part.index()))]
async fn download_part(mut state: PartTransferState) {
    if state.part.transfer_state().await != TransferState::Downloading {
        let _permit = state.transcode_permits.acquire().await;

        if let Err(e) = state.part.negotiate_transfer_type().await {
            error!(error=?e);
            return;
        }

        if state.part.transfer_state().await == TransferState::Transcoding {
            if let Err(e) = complete_transcode(&state).await {
                error!(error=?e);
                return;
            }
        }
    }

    if let Err(e) = complete_download(&state).await {
        error!(error=?e);
    }
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

        let max_downloads = flick_sync.max_downloads().await;
        let download_permits = Arc::new(Semaphore::new(max_downloads));
        let mut jobs = Vec::new();

        flick_sync.prune_root().await;

        for server in servers {
            if let Err(e) = server.update_state().await {
                error!(server=server.id(), error=?e, "Failed to update server");
                continue;
            }

            if let Err(e) = server.prune().await {
                error!(server=server.id(), error=?e, "Failed to prune server directory");
                continue;
            }

            let max_transcodes = server.max_transcodes().await;

            let mut transfers = Vec::new();
            let transcode_permits = TranscodePermits::new(max_transcodes);

            debug!(
                server = server.id(),
                max_downloads, max_transcodes, "Starting transfer jobs"
            );

            for video in server.videos().await {
                let title = video.title().await;
                for part in video.parts().await {
                    if part.verify_download().await.is_err() {
                        continue;
                    }

                    let mut transcode_permits = transcode_permits.clone();

                    match part.transfer_state().await {
                        TransferState::Transcoding => {
                            transcode_permits.reserve();
                        }
                        TransferState::Downloaded => continue,
                        TransferState::Downloading | TransferState::Waiting => (),
                    };

                    transfers.push(PartTransferState {
                        download_permits: download_permits.clone(),
                        part,
                        title: title.clone(),
                        console: console.clone(),
                        transcode_permits,
                    });
                }
            }

            for transfer in transfers {
                jobs.push(download_part(transfer));
            }
        }

        join_all(jobs).await;

        Ok(())
    }
}
