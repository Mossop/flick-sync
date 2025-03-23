use flick_sync::{FlickSync, Server, VideoPart, VideoStats};
use rinja::Template;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use tracing::warn;

use crate::shared::uniform_title;

pub(super) struct SyncTemplate {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) duration: String,
    pub(super) size: u64,
    pub(super) percent: f64,
}

pub(super) struct ServerTemplate {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) duration: String,
    pub(super) size: u64,
    pub(super) syncs: Vec<SyncTemplate>,
}

fn format_duration(mut total: u64) -> String {
    let seconds = total % 60;
    total = (total - seconds) / 60;
    let minutes = total % 60;
    total = (total - minutes) / 60;
    let hours = total % 24;
    let days = (total - hours) / 24;

    if days > 0 {
        format!("{days} days, {hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{hours}:{minutes:02}:{seconds:02}")
    }
}

impl ServerTemplate {
    pub(super) async fn build(flick_sync: &FlickSync) -> Vec<Self> {
        let mut servers = Vec::new();
        for server in flick_sync.servers().await {
            let mut stats = VideoStats::default();

            for video in server.videos().await {
                stats += video.stats().await;
            }

            let mut syncs = Vec::new();

            for sync in server.list_syncs().await {
                let stats = sync.stats().await;

                syncs.push(SyncTemplate {
                    id: sync.id,
                    name: sync.title,
                    duration: format_duration(stats.local_duration.as_secs()),
                    size: stats.local_bytes,
                    percent: if stats.total_parts == 0 {
                        0.0
                    } else {
                        (100.0 * stats.downloaded_parts as f64) / stats.total_parts as f64
                    },
                });
            }

            syncs.sort_by(|sa, sb| uniform_title(&sa.name).cmp(&uniform_title(&sb.name)));

            servers.push(ServerTemplate {
                id: server.id().to_owned(),
                name: server.name().await,
                size: stats.local_bytes,
                duration: format_duration(stats.local_duration.as_secs()),
                syncs,
            });
        }

        servers.sort_by(|sa, sb| uniform_title(&sa.name).cmp(&uniform_title(&sb.name)));

        servers
    }
}

#[derive(Clone)]
pub(super) enum Event {
    SyncStart,
    SyncChange,
    SyncEnd,
    Log(SyncLogItem),
    Progress(Vec<SyncProgressBar>),
}

impl Event {
    fn event_name(&self) -> &'static str {
        match self {
            Self::SyncStart | Self::SyncEnd => "sync-status",
            Self::Log(_) => "sync-log",
            Self::SyncChange => "sync-change",
            Self::Progress(_) => "sync-progress",
        }
    }

    pub async fn event_data(&self, flick_sync: &FlickSync) -> Result<String, rinja::Error> {
        match self {
            Self::SyncStart => Ok(
                r#"<sl-icon id="spinner" class="spinning" name="arrow-repeat"></sl-icon> Syncing"#
                    .to_string(),
            ),
            Self::SyncEnd => Ok(
                r#"<sl-icon id="spinner" class="paused" name="arrow-repeat"></sl-icon> Status"#
                    .to_string(),
            ),
            Self::SyncChange => {
                #[derive(Template)]
                #[template(path = "syncservers.html")]
                struct SyncList {
                    servers: Vec<ServerTemplate>,
                }

                let template = SyncList {
                    servers: ServerTemplate::build(flick_sync).await,
                };

                template.render()
            }
            Self::Log(message) => message.template().await.render(),
            Self::Progress(bars) => {
                let mut lines = Vec::new();
                for bar in bars {
                    lines.push(bar.template().await.render()?)
                }

                Ok(lines.join("\n"))
            }
        }
    }

    pub(super) async fn to_string(&self, flick_sync: &FlickSync) -> Option<String> {
        match self.event_data(flick_sync).await {
            Ok(data) => {
                let lines: Vec<String> = data
                    .trim()
                    .split('\n')
                    .map(|l| format!("data: {l}"))
                    .collect();
                Some(format!(
                    "event: {}\n{}\n\n",
                    self.event_name(),
                    lines.join("\n")
                ))
            }
            Err(e) => {
                warn!(error=%e, "Failed to render event");
                None
            }
        }
    }
}

#[derive(Clone)]
pub(crate) enum SyncLogMessage {
    SyncStarted(Server),
    SyncFailed((Server, String)),
    SyncFinished((Server, bool)),
    DownloadStarted(VideoPart),
    DownloadComplete(VideoPart),
    DownloadFailed((VideoPart, String)),
    TranscodeStarted(VideoPart),
    TranscodeComplete(VideoPart),
    TranscodeFailed((VideoPart, String)),
}

#[derive(Clone)]
pub(crate) struct SyncLogItem {
    timestamp: OffsetDateTime,
    message: SyncLogMessage,
}

impl From<SyncLogMessage> for SyncLogItem {
    fn from(message: SyncLogMessage) -> Self {
        SyncLogItem {
            timestamp: OffsetDateTime::now_utc(),
            message,
        }
    }
}

#[derive(Template)]
#[template(path = "synclogitem.html")]
pub(crate) struct SyncLogTemplate {
    timestamp: String,
    message_type: &'static str,
    message: String,
}

impl SyncLogItem {
    pub(crate) async fn template(&self) -> SyncLogTemplate {
        let timestamp = self.timestamp.format(&Rfc3339).unwrap();

        match &self.message {
            SyncLogMessage::SyncStarted(server) => SyncLogTemplate {
                timestamp,
                message_type: "info",
                message: format!("Syncing started for {}.", server.name().await),
            },
            SyncLogMessage::SyncFailed((server, message)) => SyncLogTemplate {
                timestamp,
                message_type: "error",
                message: format!("Syncing failed for {}: {message}", server.name().await),
            },
            SyncLogMessage::SyncFinished((server, complete)) => {
                if *complete {
                    SyncLogTemplate {
                        timestamp,
                        message_type: "success",
                        message: format!("Syncing finished for {}.", server.name().await),
                    }
                } else {
                    SyncLogTemplate {
                        timestamp,
                        message_type: "success",
                        message: format!(
                            "Syncing finished for {}, some items were not fully synced.",
                            server.name().await
                        ),
                    }
                }
            }
            SyncLogMessage::DownloadStarted(video_part) => SyncLogTemplate {
                timestamp,
                message_type: "info",
                message: format!(
                    "Download started for {}.",
                    video_part.video().await.title().await
                ),
            },
            SyncLogMessage::DownloadComplete(video_part) => SyncLogTemplate {
                timestamp,
                message_type: "success",
                message: format!(
                    "Download complete for {}.",
                    video_part.video().await.title().await
                ),
            },
            SyncLogMessage::DownloadFailed((video_part, message)) => SyncLogTemplate {
                timestamp,
                message_type: "error",
                message: format!(
                    "Download failed for {}: {message}",
                    video_part.video().await.title().await
                ),
            },
            SyncLogMessage::TranscodeStarted(video_part) => SyncLogTemplate {
                timestamp,
                message_type: "info",
                message: format!(
                    "Transcode started for {}.",
                    video_part.video().await.title().await
                ),
            },
            SyncLogMessage::TranscodeComplete(video_part) => SyncLogTemplate {
                timestamp,
                message_type: "success",
                message: format!(
                    "Transcode complete for {}.",
                    video_part.video().await.title().await
                ),
            },
            SyncLogMessage::TranscodeFailed((video_part, message)) => SyncLogTemplate {
                timestamp,
                message_type: "error",
                message: format!(
                    "Transcode failed for {}: {message}",
                    video_part.video().await.title().await
                ),
            },
        }
    }
}

#[derive(Template)]
#[template(path = "progressbar.html")]
pub(crate) struct ProgressBarTemplate {
    id: String,
    is_download: bool,
    video: String,
    position: u64,
    length: Option<u64>,
}

#[derive(Clone)]
pub(crate) struct SyncProgressBar {
    pub(super) is_download: bool,
    pub(super) video_part: VideoPart,
    pub(super) position: u64,
    pub(super) length: Option<u64>,
}

impl SyncProgressBar {
    pub(crate) async fn template(&self) -> ProgressBarTemplate {
        ProgressBarTemplate {
            id: self.video_part.id().to_owned(),
            is_download: self.is_download,
            video: self.video_part.video().await.title().await,
            position: self.position,
            length: self.length,
        }
    }
}
