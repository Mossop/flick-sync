use async_trait::async_trait;
use clap::{Args, Subcommand};
use enum_dispatch::enum_dispatch;
use plex_out::PlexOut;

use crate::console::Console;

#[derive(Args)]
pub struct Login {
    /// An identifier for the server.
    id: String,
}

#[async_trait]
impl Runnable for Login {
    async fn run(self, plexout: PlexOut, console: Console) -> Result<(), String> {
        todo!()
    }
}

#[enum_dispatch]
#[derive(Subcommand)]
pub enum Command {
    /// Logs in or re-logs in to a server.
    Login,
}

#[async_trait]
#[enum_dispatch(Command)]
pub trait Runnable {
    async fn run(self, plexout: PlexOut, console: Console) -> Result<(), String>;
}
