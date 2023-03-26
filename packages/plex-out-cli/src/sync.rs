use async_trait::async_trait;
use clap::Args;
use plex_out::PlexOut;

use crate::{select_servers, Console, Result, Runnable};

#[derive(Args)]
pub struct Prune {
    /// The servers to prune. Can be repeated. When not passed all servers and
    /// the top level directory are pruned.
    #[clap(short = 's', long = "server")]
    ids: Vec<String>,
}

#[async_trait]
impl Runnable for Prune {
    async fn run(self, plexout: PlexOut, console: Console) -> Result {
        let servers = select_servers(&plexout, &self.ids).await?;

        todo!();
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
    async fn run(self, plexout: PlexOut, console: Console) -> Result {
        let servers = select_servers(&plexout, &self.ids).await?;

        todo!();
    }
}
