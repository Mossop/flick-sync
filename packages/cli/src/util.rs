use async_trait::async_trait;
use clap::Args;
use flick_sync::{FlickSync, VideoStats};
use indicatif::{DecimalBytes, HumanDuration};

use crate::{Console, Result, Runnable};

#[derive(Args)]
pub struct Stats {}

fn percent<T: Into<u64>>(a: T, b: T) -> String {
    format!("{}%", (a.into() * 100) / b.into())
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
                DecimalBytes(stats.downloaded_bytes),
                DecimalBytes(stats.total_bytes),
                percent(stats.downloaded_bytes, stats.total_bytes)
            ));
            console.println(format!(
                "  Duration available: {}",
                HumanDuration(stats.total_duration)
            ));

            total += stats;
        }

        if servers.len() > 1 {
            console.println(format!(
                "Total downloaded videos: {} / {} ({})",
                total.downloaded_parts,
                total.total_parts,
                percent(total.downloaded_parts, total.total_parts)
            ));
            console.println(format!(
                "Total downloaded data: {} / {} ({})",
                DecimalBytes(total.downloaded_bytes),
                DecimalBytes(total.total_bytes),
                percent(total.downloaded_bytes, total.total_bytes)
            ));
            console.println(format!(
                "Total duration available: {}",
                HumanDuration(total.total_duration)
            ));
        }

        Ok(())
    }
}
