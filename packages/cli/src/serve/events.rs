use flick_sync::{Server, VideoPart};
use rinja::Template;
use tracing::warn;

#[derive(Clone)]
pub(super) enum Event {
    SyncStart,
    SyncEnd,
    Log(SyncLog),
    Progress(Vec<SyncProgressBar>),
}

impl Event {
    fn event_name(&self) -> &'static str {
        match self {
            Self::SyncStart | Self::SyncEnd => "sync-status",
            Self::Log(_) => "sync-log",
            Self::Progress(_) => "sync-progress",
        }
    }

    pub async fn event_data(&self) -> Result<String, rinja::Error> {
        match self {
            Self::SyncStart => {
                Ok(r#"<sl-icon class="spin" name="arrow-repeat"></sl-icon> Syncing"#.to_string())
            }
            Self::SyncEnd => Ok(r#"<sl-icon name="arrow-repeat"></sl-icon> Syncs"#.to_string()),
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

    pub(super) async fn to_string(&self) -> Option<String> {
        match self.event_data().await {
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
pub(crate) enum SyncLog {
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

#[derive(Template)]
#[template(path = "synclogitem.html")]
pub(crate) struct SyncLogTemplate {
    message_type: &'static str,
    message: String,
}

impl SyncLog {
    pub(crate) async fn template(&self) -> SyncLogTemplate {
        match self {
            SyncLog::SyncStarted(server) => SyncLogTemplate {
                message_type: "info",
                message: format!("Syncing started for {}.", server.name().await),
            },
            SyncLog::SyncFailed((server, message)) => SyncLogTemplate {
                message_type: "error",
                message: format!("Syncing failed for {}: {message}", server.name().await),
            },
            SyncLog::SyncFinished((server, complete)) => {
                if *complete {
                    SyncLogTemplate {
                        message_type: "success",
                        message: format!("Syncing finished for {}.", server.name().await),
                    }
                } else {
                    SyncLogTemplate {
                        message_type: "success",
                        message: format!(
                            "Syncing finished for {}, some items were not fully synced.",
                            server.name().await
                        ),
                    }
                }
            }
            SyncLog::DownloadStarted(video_part) => SyncLogTemplate {
                message_type: "info",
                message: format!(
                    "Download started for {}.",
                    video_part.video().await.title().await
                ),
            },
            SyncLog::DownloadComplete(video_part) => SyncLogTemplate {
                message_type: "success",
                message: format!(
                    "Download complete for {}.",
                    video_part.video().await.title().await
                ),
            },
            SyncLog::DownloadFailed((video_part, message)) => SyncLogTemplate {
                message_type: "error",
                message: format!(
                    "Download failed for {}: {message}",
                    video_part.video().await.title().await
                ),
            },
            SyncLog::TranscodeStarted(video_part) => SyncLogTemplate {
                message_type: "info",
                message: format!(
                    "Transcode started for {}.",
                    video_part.video().await.title().await
                ),
            },
            SyncLog::TranscodeComplete(video_part) => SyncLogTemplate {
                message_type: "success",
                message: format!(
                    "Transcode complete for {}.",
                    video_part.video().await.title().await
                ),
            },
            SyncLog::TranscodeFailed((video_part, message)) => SyncLogTemplate {
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
            is_download: self.is_download,
            video: self.video_part.video().await.title().await,
            position: self.position,
            length: self.length,
        }
    }
}
