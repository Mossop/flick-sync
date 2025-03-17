use clap::Args;
use flick_sync::{DownloadProgress, FlickSync, Progress, VideoPart};
use tracing::{debug, error};

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

struct ProgressBar {
    bar: Bar,
    total: u64,
}

impl Progress for ProgressBar {
    fn progress(&mut self, position: u64) {
        self.bar.set_position(position);
    }

    fn finished(&mut self) {
        self.bar.set_position(self.total);
    }
}

#[derive(Clone)]
struct ConsoleProgress {
    console: Console,
}

impl DownloadProgress for ConsoleProgress {
    async fn transcode_started(&self, video_part: &VideoPart) -> impl Progress {
        let title = video_part.video().await.title().await;

        let bar = self
            .console
            .add_progress_bar(&format!("🔄 {title}"), ProgressType::Percent);

        ProgressBar { bar, total: 100 }
    }

    async fn download_started(&self, video_part: &VideoPart, total: u64) -> impl Progress {
        let title = video_part.video().await.title().await;

        let bar = self
            .console
            .add_progress_bar(&format!("💾 {title}"), ProgressType::Bytes);
        bar.set_length(total);

        ProgressBar { bar, total }
    }
}

#[derive(Args)]
pub struct Sync {
    /// The servers to sync. Can be repeated. When not passed all servers are listed.
    #[clap(short = 's', long = "server")]
    ids: Vec<String>,
}

impl Runnable for Sync {
    async fn run(self, flick_sync: FlickSync, console: Console) -> Result {
        let servers = select_servers(&flick_sync, &self.ids).await?;

        flick_sync.prune_root().await;

        let progress = ConsoleProgress { console };

        for server in &servers {
            if let Err(e) = server.update_state().await {
                error!(server=server.id(), error=?e, "Failed to update server");
                continue;
            }

            if let Err(e) = server.prune().await {
                error!(server=server.id(), error=?e, "Failed to prune server directory");
                continue;
            }

            debug!(server = server.id(), "Starting transfer jobs");

            server.download(progress.clone()).await;
            server.write_playlists().await;
        }

        Ok(())
    }
}
