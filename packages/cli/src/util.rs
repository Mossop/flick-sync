use async_trait::async_trait;
use clap::Args;
use flick_sync::{FlickSync, ItemType, VideoStats};
use indicatif::{DecimalBytes, HumanDuration};

use crate::{Console, Result, Runnable};

#[derive(Args)]
pub struct Stats {}

fn percent<T: Into<u64>>(a: T, b: T) -> String {
    let a = a.into();
    let b = b.into();
    if a >= b {
        "100%".to_string()
    } else {
        format!("{}%", (a * 100) / b)
    }
}

#[async_trait]
impl Runnable for Stats {
    async fn run(self, flick_sync: FlickSync, console: Console) -> Result {
        let mut total = VideoStats::default();

        let servers = flick_sync.servers().await;
        for (pos, server) in servers.iter().enumerate() {
            let mut stats = VideoStats::default();

            for video in server.videos().await {
                stats += video.stats().await?;
            }

            if pos > 0 {
                console.println("");
            }

            console.println(format!("Server {}:", server.id()));
            console.println(format!(
                "  Downloaded videos: {} / {} ({})",
                stats.downloaded_parts,
                stats.total_parts,
                percent(stats.downloaded_parts, stats.total_parts)
            ));
            console.println(format!(
                "  Downloaded data: {} / {} ({})",
                DecimalBytes(stats.local_bytes),
                DecimalBytes(stats.remote_bytes),
                percent(stats.local_bytes, stats.remote_bytes)
            ));
            console.println(format!(
                "  Remaining data: {}",
                DecimalBytes(stats.remaining_bytes),
            ));
            console.println(format!(
                "  Duration available offline: {}",
                HumanDuration(stats.local_duration)
            ));
            console.println(format!(
                "  Total Duration: {}",
                HumanDuration(stats.remote_duration)
            ));

            total += stats;
        }

        if servers.len() > 1 {
            console.println("");
            console.println(format!(
                "Total downloaded videos: {} / {} ({})",
                total.downloaded_parts,
                total.total_parts,
                percent(total.downloaded_parts, total.total_parts)
            ));
            console.println(format!(
                "Total downloaded data: {} / {} ({})",
                DecimalBytes(total.local_bytes),
                DecimalBytes(total.remote_bytes),
                percent(total.local_bytes, total.remote_bytes)
            ));
            console.println(format!(
                "Total remaining data: {}",
                DecimalBytes(total.remaining_bytes),
            ));
            console.println(format!(
                "Total duration available offline: {}",
                HumanDuration(total.local_duration)
            ));
            console.println(format!(
                "Total Duration: {}",
                HumanDuration(total.remote_duration)
            ));
        }

        Ok(())
    }
}

#[derive(Args)]
pub struct List {}

#[async_trait]
impl Runnable for List {
    async fn run(self, flick_sync: FlickSync, console: Console) -> Result {
        let servers = flick_sync.servers().await;
        for (pos, server) in servers.iter().enumerate() {
            if pos > 0 {
                console.println("");
            }

            for item in server.list_syncs().await? {
                let type_name = match item.item_type {
                    ItemType::Playlist => "Playlist",
                    ItemType::MovieCollection => "Movie Collection",
                    ItemType::ShowCollection => "Show Collection",
                    ItemType::Show => "Show",
                    ItemType::Season => "Season",
                    ItemType::Episode => "Episode",
                    ItemType::Movie => "Movie",
                    ItemType::Unknown => "Unknown",
                };

                let selected = if item.only_unplayed {
                    "unplayed"
                } else {
                    "all"
                };

                console.println(format!(
                    "{:10} {:8} {type_name:16}  {:20} {selected:3} {:10}",
                    server.id(),
                    item.id,
                    item.title,
                    item.transcode_profile.unwrap_or_default(),
                ));
            }
        }

        Ok(())
    }
}
