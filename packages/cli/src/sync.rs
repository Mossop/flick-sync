use std::sync::Arc;

use async_trait::async_trait;
use clap::Args;
use flick_sync::{FlickSync, Progress, TransferState, VideoPart};
use futures::future::join_all;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tracing::{error, instrument};

use crate::{
    console::{Bar, ProgressType},
    select_servers, Console, Result, Runnable,
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
    async fn run(self, _flick_sync: FlickSync, _console: Console) -> Result {
        todo!();
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
    transcode_permits: Arc<Semaphore>,
    download_permits: Arc<Semaphore>,
    title: String,
    part: VideoPart,
    console: Console,
    transcode_permit: Option<PermitHolder>,
}

struct PermitHolder {
    permit: Option<OwnedSemaphorePermit>,
    forget: bool,
}

impl PermitHolder {
    fn forget(permit: OwnedSemaphorePermit) -> Self {
        Self {
            permit: Some(permit),
            forget: true,
        }
    }
}

impl From<OwnedSemaphorePermit> for PermitHolder {
    fn from(permit: OwnedSemaphorePermit) -> Self {
        Self {
            permit: Some(permit),
            forget: false,
        }
    }
}

impl Drop for PermitHolder {
    fn drop(&mut self) {
        if let Some(permit) = self.permit.take() {
            if self.forget {
                permit.forget();
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

    complete_download(state).await
}

async fn complete_download(state: &PartTransferState) -> Result {
    let _permit = state.download_permits.acquire().await.unwrap();

    let bar = state
        .console
        .add_progress_bar(&format!("💾 {}", state.title), ProgressType::Bytes);
    state.part.download(DownloadProgress { bar }).await?;

    Ok(())
}

#[instrument(level = "trace", skip(state), fields(part=state.part.id()))]
async fn download_part(mut state: PartTransferState) {
    if state.part.transfer_state().await != TransferState::Downloading {
        let _permit = if let Some(permit) = state.transcode_permit.take() {
            permit
        } else {
            state
                .transcode_permits
                .clone()
                .acquire_owned()
                .await
                .unwrap()
                .into()
        };

        if let Err(e) = state.part.negotiate_transfer_type().await {
            error!(error=?e);
            return;
        }

        if state.part.transfer_state().await == TransferState::Transcoding {
            if let Err(e) = complete_transcode(&state).await {
                error!(error=?e);
            }
            return;
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

        let download_permits = Arc::new(Semaphore::new(flick_sync.max_downloads().await));
        let mut jobs = Vec::new();

        for server in servers {
            let mut transfers = Vec::new();
            let transcode_permits = Arc::new(Semaphore::new(server.max_transcodes().await));

            for video in server.videos().await {
                let title = video.title().await;
                for part in video.parts().await {
                    if part.verify_download().await.is_err() {
                        continue;
                    }

                    let transcode_permit = match part.transfer_state().await {
                        TransferState::Waiting => None,
                        TransferState::Transcoding => {
                            // This is only safe because there are no other tasks running at this
                            // point.
                            if transcode_permits.available_permits() == 0 {
                                transcode_permits.add_permits(1);
                                Some(PermitHolder::forget(
                                    transcode_permits.clone().acquire_owned().await.unwrap(),
                                ))
                            } else {
                                Some(PermitHolder::from(
                                    transcode_permits.clone().acquire_owned().await.unwrap(),
                                ))
                            }
                        }
                        TransferState::Downloading => None,
                        TransferState::Downloaded => continue,
                    };

                    transfers.push(PartTransferState {
                        download_permits: download_permits.clone(),
                        transcode_permits: transcode_permits.clone(),
                        part,
                        title: title.clone(),
                        console: console.clone(),
                        transcode_permit,
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
