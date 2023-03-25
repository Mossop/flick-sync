use std::{env::current_dir, fmt, path::PathBuf};

use clap::Parser;
use commands::{Command, Runnable};
use flexi_logger::Logger;
use plex_out::{PlexOut, CONFIG_FILE, STATE_FILE};
use tokio::fs::{metadata, read_dir};

mod commands;
mod console;

use crate::console::Console;

fn d_to_s<D: fmt::Display>(d: D) -> String {
    d.to_string()
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

async fn validate_store(store: Option<PathBuf>) -> Result<PathBuf, String> {
    let path = store.unwrap_or_else(|| current_dir().unwrap());

    log::trace!("Checking for store directory at {}", path.display());
    match metadata(&path).await {
        Ok(stats) => {
            if !stats.is_dir() {
                return Err(format!("Store {} is not a directory", path.display()));
            }
        }
        Err(_) => {
            return Err(format!("Store {} is not a directory", path.display()));
        }
    }

    let state = path.join(STATE_FILE);
    if let Ok(stats) = metadata(&state).await {
        if stats.is_file() {
            log::trace!("Store contained state file");
            return Ok(path);
        } else {
            return Err("Store contained a non-file where a state file was expected".to_string());
        }
    }

    log::trace!("No state file, checking for non-config files in a new store");
    let mut reader = read_dir(&path).await.map_err(d_to_s)?;
    while let Some(entry) = reader.next_entry().await.map_err(d_to_s)? {
        let file_name = entry.file_name();
        let name = match file_name.to_str() {
            Some(s) => s,
            None => {
                log::error!("Store contained an entry with a non-UTF8 invalid name");
                return Err("New store is not empty".to_string());
            }
        };

        let typ = entry.file_type().await.map_err(d_to_s)?;
        if typ.is_file() {
            if name != CONFIG_FILE {
                log::error!("{} exists in a potential new store", name);
                return Err("New store is not empty".to_string());
            }
        } else {
            log::error!("{} exists in a potential new store", name);
            return Err("New store is not empty".to_string());
        }
    }

    Ok(path)
}

async fn wrapped_main(args: Args, console: Console) -> Result<(), String> {
    let store = validate_store(args.store).await?;
    let plexout = PlexOut::new(&store).await?;

    args.command.run(plexout, console).await
}

#[tokio::main]
async fn main() -> Result<(), String> {
    let args = Args::parse();

    let console = Console::new();

    if let Err(e) = Logger::try_with_env_or_str("trace")
        .and_then(|logger| logger.log_to_writer(Box::new(console.clone())).start())
    {
        console.println(format!("Warning, failed to start logging: {}", e));
    }

    wrapped_main(args, console).await.map_err(|e| {
        log::error!("{}", e);
        e
    })
}
