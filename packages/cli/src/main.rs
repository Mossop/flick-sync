use std::{
    env::{self, current_dir},
    io,
    path::PathBuf,
    pin::Pin,
    result,
    task::{Context, Poll},
};

use anyhow::{anyhow, bail};
use bytes::Bytes;
use clap::{Parser, Subcommand};
use enum_dispatch::enum_dispatch;
use flick_sync::{CONFIG_FILE, FlickSync, STATE_FILE, Server};
use futures::Stream;
use pin_project::pin_project;
use rust_embed::{Embed, EmbeddedFile};
use sync::{Prune, Sync};
use tokio::fs::{metadata, read_dir};
use tracing::{error, trace};

mod console;
mod dlna;
mod serve;
mod server;
pub(crate) mod shared;
mod sync;
mod util;

use console::Console;
use serve::Serve;
use server::{Add, Login, Recover, Remove};
use sync::BuildMetadata;
use util::{List, Stats};

pub type Result<T = ()> = anyhow::Result<T>;

#[pin_project]
struct EmbeddedFileStream {
    position: usize,
    file: EmbeddedFile,
}

impl EmbeddedFileStream {
    fn new(file: EmbeddedFile) -> Self {
        Self { file, position: 0 }
    }
}

impl Stream for EmbeddedFileStream {
    type Item = result::Result<Bytes, io::Error>;

    fn poll_next(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.project();

        if *this.position >= this.file.data.len() {
            Poll::Ready(None)
        } else {
            let bytes = Bytes::copy_from_slice(&this.file.data);
            *this.position = this.file.data.len();
            Poll::Ready(Some(Ok(bytes)))
        }
    }
}

#[derive(Embed)]
#[folder = "resources"]
struct Resources;

#[enum_dispatch]
#[derive(Subcommand)]
pub enum Command {
    /// Logs in or re-logs in to a server.
    Login,
    /// Adds an item to sync.
    Add,
    /// Removes an item from the list to sync.
    Remove,
    /// Updates the lists of items to sync and then remove any local content no
    /// longer included.
    Prune,
    /// Performs a full sync.
    Sync,
    /// List download statistics.
    Stats,
    /// Lists sync items.
    List,
    /// Attempts to recover from a corrupt state file.
    Recover,
    /// Rebuilds metadata files.
    BuildMetadata,
    /// Serves downloaded media over DLNA.
    Serve,
}

#[enum_dispatch(Command)]
pub(crate) trait Runnable {
    async fn run(self, flick_sync: FlickSync, console: Console) -> Result;
}

pub async fn select_servers(
    flick_sync: &FlickSync,
    ids: &Vec<String>,
) -> anyhow::Result<Vec<Server>> {
    if ids.is_empty() {
        Ok(flick_sync.servers().await)
    } else {
        let mut servers = Vec::new();

        for id in ids {
            servers.push(
                flick_sync
                    .server(id)
                    .await
                    .ok_or_else(|| anyhow!("Unknown server: {id}"))?,
            );
        }

        Ok(servers)
    }
}

#[derive(Parser)]
#[clap(author, version)]
struct Args {
    /// The storage location to use.
    #[clap(short, long, env = "FLICK_SYNC_STORE")]
    store: Option<PathBuf>,

    #[clap(subcommand)]
    command: Command,
}

async fn validate_store(store: Option<PathBuf>) -> Result<PathBuf> {
    let path = store.unwrap_or_else(|| current_dir().unwrap());

    trace!(?path, "Checking for store directory");
    match metadata(&path).await {
        Ok(stats) => {
            if !stats.is_dir() {
                bail!("Store {} is not a directory", path.display());
            }
        }
        Err(_) => {
            bail!("Store {} is not a directory", path.display());
        }
    }

    let config = path.join(CONFIG_FILE);
    if let Ok(stats) = metadata(&config).await {
        if stats.is_file() {
            trace!("Store contained config file");
            return Ok(path);
        } else {
            bail!("Store contained a non-file where a config file was expected");
        }
    }

    let state = path.join(STATE_FILE);
    if let Ok(stats) = metadata(&state).await {
        if stats.is_file() {
            trace!("Store contained state file");
            return Ok(path);
        } else {
            bail!("Store contained a non-file where a state file was expected");
        }
    }

    trace!("No state file, checking for non-config files in a new store");
    let mut reader = read_dir(&path).await?;
    while let Some(entry) = reader.next_entry().await? {
        let file_name = entry.file_name();
        let name = match file_name.to_str() {
            Some(s) => s,
            None => {
                error!("Store contained an entry with a non-UTF8 invalid name");
                bail!("New store is not empty");
            }
        };

        let typ = entry.file_type().await?;
        if typ.is_file() {
            if name != CONFIG_FILE {
                error!("{} exists in a potential new store", name);
                bail!("New store is not empty");
            }
        } else {
            error!("{} exists in a potential new store", name);
            bail!("New store is not empty");
        }
    }

    Ok(path)
}

async fn wrapped_main(args: Args, console: Console) -> Result {
    let store = validate_store(args.store).await?;
    let flick_sync = FlickSync::new(&store).await?;

    args.command.run(flick_sync, console).await
}

#[tokio::main]
async fn main() -> Result {
    let args: Args = Args::parse();

    let console = Console::default();

    let log_filter = if cfg!(debug_assertions) {
        env::var("RUST_LOG")
            .unwrap_or_else(|_| "flick_sync=trace,dlna_server=debug,warn".to_string())
    } else {
        env::var("RUST_LOG")
            .unwrap_or_else(|_| "flick_sync=debug,dlna_server=info,warn".to_string())
    };

    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(&log_filter)
        .with_ansi(true)
        .pretty()
        .with_writer(console.clone())
        .finish();
    if let Err(e) = tracing::subscriber::set_global_default(subscriber) {
        eprintln!("Unable to set global default subscriber: {e}");
    }

    wrapped_main(args, console).await.map_err(|e| {
        error!("{}", e);
        e
    })
}
