use std::{env::current_dir, path::PathBuf};

use async_trait::async_trait;
use clap::{Parser, Subcommand};
use error::{err, Error};
use flexi_logger::Logger;
use plex_out::{PlexOut, CONFIG_FILE, STATE_FILE};
use tokio::fs::{metadata, read_dir};

mod console;
mod error;
mod server;

use crate::console::Console;
use server::{Add, Login};

pub type Result<T = ()> = std::result::Result<T, Error>;

#[async_trait]
pub trait Runnable {
    async fn run(self, plexout: PlexOut, console: Console) -> Result;
}

#[derive(Subcommand)]
pub enum Command {
    /// Logs in or re-logs in to a server.
    Login(Login),
    /// Adds an item to sync.
    Add(Add),
}

#[async_trait]
impl Runnable for Command {
    async fn run(self, plexout: PlexOut, console: Console) -> Result {
        match self {
            Command::Login(c) => c.run(plexout, console).await,
            Command::Add(c) => c.run(plexout, console).await,
        }
    }
}

#[derive(Parser)]
#[clap(author, version)]
struct Args {
    /// The storage location to use.
    #[clap(short, long, env)]
    store: Option<PathBuf>,

    #[clap(subcommand)]
    command: Command,
}

async fn validate_store(store: Option<PathBuf>) -> Result<PathBuf> {
    let path = store.unwrap_or_else(|| current_dir().unwrap());

    log::trace!("Checking for store directory at {}", path.display());
    match metadata(&path).await {
        Ok(stats) => {
            if !stats.is_dir() {
                return err(format!("Store {} is not a directory", path.display()));
            }
        }
        Err(_) => {
            return err(format!("Store {} is not a directory", path.display()));
        }
    }

    let state = path.join(STATE_FILE);
    if let Ok(stats) = metadata(&state).await {
        if stats.is_file() {
            log::trace!("Store contained state file");
            return Ok(path);
        } else {
            return err("Store contained a non-file where a state file was expected");
        }
    }

    log::trace!("No state file, checking for non-config files in a new store");
    let mut reader = read_dir(&path).await?;
    while let Some(entry) = reader.next_entry().await? {
        let file_name = entry.file_name();
        let name = match file_name.to_str() {
            Some(s) => s,
            None => {
                log::error!("Store contained an entry with a non-UTF8 invalid name");
                return err("New store is not empty");
            }
        };

        let typ = entry.file_type().await?;
        if typ.is_file() {
            if name != CONFIG_FILE {
                log::error!("{} exists in a potential new store", name);
                return err("New store is not empty");
            }
        } else {
            log::error!("{} exists in a potential new store", name);
            return err("New store is not empty");
        }
    }

    Ok(path)
}

async fn wrapped_main(args: Args, console: Console) -> Result {
    let store = validate_store(args.store).await?;
    let plexout = PlexOut::new(&store).await?;

    args.command.run(plexout, console).await
}

#[tokio::main]
async fn main() -> Result {
    let args = Args::parse();

    let console = Console::default();

    if let Err(e) = Logger::try_with_env_or_str("plex_out=trace,warn")
        .and_then(|logger| logger.log_to_writer(Box::new(console.clone())).start())
    {
        console.println(format!("Warning, failed to start logging: {}", e));
    }

    wrapped_main(args, console).await.map_err(|e| {
        log::error!("{}", e);
        e
    })
}
