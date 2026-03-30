use clap::Args;
use flick_sync::{DownloadProgress, FlickSync, Progress, SyncProgress, Video};
use tracing::{debug, error, instrument, warn};

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
    #[instrument(name = "Prune", skip_all)]
    async fn run(self, flick_sync: FlickSync, _console: Console) -> Result {
        flick_sync.prune_root().await;

        let servers = select_servers(&flick_sync, &self.ids).await?;

        for server in servers {
            if let Err(e) = server.update_state(true).await {
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
    #[instrument(name = "BuildMetadata", skip_all)]
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

#[derive(Clone)]
struct ProgressBar {
    bar: Bar,
}

impl Progress for ProgressBar {
    fn progress(&mut self, position: u64) {
        self.bar.set_position(position);
    }

    fn length(&mut self, length: u64) {
        self.bar.set_length(length);
    }

    fn finished(self) {
        if let Some(length) = self.bar.length() {
            self.bar.set_position(length);
        }
    }
}

struct ConsoleDownloadProgress {
    console: Console,
    title: String,
    overall: Bar,
}

impl DownloadProgress for ConsoleDownloadProgress {
    async fn transcode_started(&self) -> impl Progress + Clone + 'static {
        let bar = self
            .console
            .add_progress_bar(&format!("🔄 {}", self.title), ProgressType::Percent);
        bar.set_length(100);

        ProgressBar { bar }
    }

    async fn download_started(&self) -> impl Progress + Clone + 'static {
        let bar = self
            .console
            .add_progress_bar(&format!("💾 {}", self.title), ProgressType::Bytes);

        ProgressBar { bar }
    }

    async fn download_failed(self, #[expect(unused)] error: anyhow::Error) {
        self.overall.inc(1);
    }

    async fn finished(self) {
        self.overall.inc(1);
    }
}

#[derive(Clone)]
struct ConsoleProgress {
    console: Console,
    overall: Bar,
}

impl ConsoleProgress {
    fn new(console: Console) -> Self {
        let overall = console.add_progress_bar("  Total progress", ProgressType::Count);
        overall.set_position(0);

        Self { overall, console }
    }
}

impl SyncProgress for ConsoleProgress {
    type DP = ConsoleDownloadProgress;

    async fn jobs(&mut self, count: usize) {
        self.overall.set_length(count as u64);
    }

    async fn download_progress(&mut self, video: &Video) -> ConsoleDownloadProgress {
        let title = video.title().await;

        ConsoleDownloadProgress {
            console: self.console.clone(),
            title,
            overall: self.overall.clone(),
        }
    }
}

#[derive(Args)]
pub struct Sync {
    /// The servers to sync. Can be repeated. When not passed all servers are listed.
    #[clap(short = 's', long = "server")]
    ids: Vec<String>,
}

impl Runnable for Sync {
    #[instrument(name = "Sync", skip_all)]
    async fn run(self, flick_sync: FlickSync, console: Console) -> Result {
        let servers = select_servers(&flick_sync, &self.ids).await?;

        flick_sync.prune_root().await;

        let progress = ConsoleProgress::new(console);

        for server in &servers {
            if let Err(e) = server.update_state(true).await {
                error!(server=server.id(), error=?e, "Failed to update server");
                continue;
            }

            if let Err(e) = server.prune().await {
                error!(server=server.id(), error=?e, "Failed to prune server directory");
                continue;
            }

            debug!(server = server.id(), "Starting transfer jobs");

            if let Ok(false) = server.download(progress.clone()).await {
                warn!("Some items are not yet downloaded");
            }

            server.write_playlists().await;
        }

        Ok(())
    }
}
