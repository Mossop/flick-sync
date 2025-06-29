use std::{
    env::current_dir,
    error::Error,
    io,
    path::PathBuf,
    pin::Pin,
    result,
    task::{Context, Poll},
    time::Duration,
};

use anyhow::{anyhow, bail};
use bytes::Bytes;
use clap::{Parser, Subcommand};
use console::Console;
use enum_dispatch::enum_dispatch;
use futures::Stream;
use opentelemetry::{KeyValue, global, trace::TracerProvider as _};
use opentelemetry_otlp::{Protocol, SpanExporter, WithExportConfig};
use opentelemetry_sdk::{Resource, propagation::TraceContextPropagator, trace::SdkTracerProvider};
use pin_project::pin_project;
use rust_embed::{Embed, EmbeddedFile};
use tokio::fs::{metadata, read_dir};
use tracing::{Level, error, trace};
use tracing_subscriber::{
    Layer, Registry, filter::Targets, layer::SubscriberExt, util::SubscriberInitExt,
};

mod console;
mod dlna;
mod serve;
mod server;
pub(crate) mod shared;
mod sync;
mod util;

use flick_sync::{CONFIG_FILE, FlickSync, STATE_FILE, Server};
use serve::Serve;
use server::{Add, Login, Recover, Remove};
use sync::BuildMetadata;
use sync::{Prune, Sync};
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

    /// The telemetry host to use.
    #[clap(short, long, env = "FLICK_SYNC_TELEMETRY")]
    telemetry: Option<String>,

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

fn init_logging(
    console: Console,
    telemetry: Option<&str>,
) -> result::Result<Option<SdkTracerProvider>, Box<dyn Error>> {
    let targets = if cfg!(debug_assertions) {
        Targets::new()
            .with_target("flick_sync", Level::TRACE)
            .with_target("flick_sync_cli", Level::TRACE)
            .with_target("dlna_server", Level::DEBUG)
            .with_default(Level::WARN)
    } else {
        Targets::new()
            .with_target("flick_sync", Level::DEBUG)
            .with_target("flick_sync_cli", Level::DEBUG)
            .with_target("dlna_server", Level::INFO)
            .with_default(Level::WARN)
    };

    let formatter = tracing_subscriber::fmt::layer()
        .with_ansi(true)
        .pretty()
        .with_writer(console)
        .with_filter(targets);

    let registry = Registry::default().with(formatter);

    if let Some(telemetry_host) = telemetry {
        global::set_text_map_propagator(TraceContextPropagator::new());

        let tracer_provider = SdkTracerProvider::builder()
            .with_batch_exporter(
                SpanExporter::builder()
                    .with_http()
                    .with_protocol(Protocol::HttpBinary)
                    .with_endpoint(telemetry_host)
                    .with_timeout(Duration::from_secs(3))
                    .build()?,
            )
            .with_resource(
                Resource::builder()
                    .with_attribute(KeyValue::new("service.name", "flick-sync"))
                    .build(),
            )
            .build();

        let tracer = tracer_provider.tracer("flick-sync");

        let filter = Targets::new()
            .with_target("flick_sync", Level::TRACE)
            .with_target("flick_sync_cli", Level::TRACE)
            .with_target("dlna_server", Level::TRACE)
            .with_default(Level::INFO);

        let telemetry = tracing_opentelemetry::layer()
            .with_error_fields_to_exceptions(true)
            .with_tracked_inactivity(true)
            .with_tracer(tracer)
            .with_filter(filter);

        registry.with(telemetry).init();

        Ok(Some(tracer_provider))
    } else {
        registry.init();

        Ok(None)
    }
}

#[tokio::main]
async fn main() -> Result {
    let args: Args = Args::parse();

    let console = Console::default();

    let provider = match init_logging(console.clone(), args.telemetry.as_deref()) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Failed to initialise logging: {e}");
            None
        }
    };

    let result = wrapped_main(args, console).await.map_err(|e| {
        error!("{}", e);
        e
    });

    if let Some(provider) = provider {
        let _ = provider.force_flush();
    }

    result
}
