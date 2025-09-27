use std::cmp::Ordering;

use askama::Template;
use flick_sync::{Collection, FlickSync, PlaybackState, Season, Show, Video, VideoStats};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use tracing::warn;

use crate::shared::uniform_title;

#[derive(Template)]
#[template(path = "thumbnail.html")]
struct ThumbnailTemplate<'a> {
    thumbnail: &'a Thumbnail,
}

#[derive(Clone)]
pub(crate) struct Thumbnail {
    pub(crate) id: String,
    pub(crate) url: String,
    pub(crate) image: String,
    pub(crate) name: String,
    pub(crate) position: Option<u128>,
    pub(crate) duration: Option<u128>,
}

impl PartialEq for Thumbnail {
    fn eq(&self, other: &Self) -> bool {
        uniform_title(&self.name) == uniform_title(&other.name)
    }
}

impl Eq for Thumbnail {}

impl Ord for Thumbnail {
    fn cmp(&self, other: &Self) -> Ordering {
        uniform_title(&self.name).cmp(&uniform_title(&other.name))
    }
}

impl PartialOrd for Thumbnail {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Thumbnail {
    pub(crate) async fn from_season(season: Season) -> Self {
        let server = season.server();
        let show = season.show().await;
        let library = show.library().await;

        Self {
            id: season.id().to_owned(),
            url: format!(
                "/library/{}/{}/season/{}",
                server.id(),
                library.id(),
                season.id()
            ),
            image: format!("/thumbnail/{}/show/{}", server.id(), show.id()),
            name: season.title().await,
            position: None,
            duration: None,
        }
    }

    pub(crate) async fn from_show(show: Show) -> Self {
        let server = show.server();
        let library = show.library().await;

        Self {
            id: show.id().to_owned(),
            url: format!(
                "/library/{}/{}/show/{}",
                server.id(),
                library.id(),
                show.id()
            ),
            image: format!("/thumbnail/{}/show/{}", server.id(), show.id()),
            name: show.title().await,
            position: None,
            duration: None,
        }
    }

    pub(crate) async fn from_video(video: Video) -> Self {
        let server = video.server();
        let library = video.library().await;
        let position = match video.playback_state().await {
            PlaybackState::Unplayed => 0,
            PlaybackState::InProgress { position } => position as u128,
            PlaybackState::Played => video.duration().await.as_millis(),
        };

        Self {
            id: video.id().to_owned(),
            url: format!(
                "/library/{}/{}/video/{}",
                server.id(),
                library.id(),
                video.id()
            ),
            image: format!("/thumbnail/{}/video/{}", server.id(), video.id()),
            name: video.title().await,
            position: Some(position),
            duration: Some(video.duration().await.as_millis()),
        }
    }

    pub(crate) async fn from_collection(collection: Collection) -> Self {
        let server = collection.server();
        let library = collection.library().await;

        Self {
            id: collection.id().to_owned(),
            url: format!(
                "/library/{}/{}/collection/{}",
                server.id(),
                library.id(),
                collection.id()
            ),
            image: format!("/thumbnail/{}/collection/{}", server.id(), collection.id()),
            name: collection.title().await,
            position: None,
            duration: None,
        }
    }
}

pub(super) struct SyncTemplate {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) duration: String,
    pub(super) size: u64,
    pub(super) percent: f64,
    pub(super) transcode_profile: Option<String>,
}

pub(super) struct ServerTemplate {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) duration: String,
    pub(super) size: u64,
    pub(super) percent: f64,
    pub(super) syncs: Vec<SyncTemplate>,
    pub(super) transcode_profile: String,
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
                    percent: if stats.remote_videos == 0 {
                        0.0
                    } else {
                        (100.0 * stats.local_bytes as f64) / stats.remote_bytes as f64
                    },
                    transcode_profile: sync.transcode_profile,
                });
            }

            syncs.sort_by(|sa, sb| uniform_title(&sa.name).cmp(&uniform_title(&sb.name)));

            servers.push(ServerTemplate {
                id: server.id().to_owned(),
                name: server.name().await,
                size: stats.local_bytes,
                percent: if stats.remote_bytes == 0 {
                    100.0
                } else {
                    (stats.local_bytes as f64 * 100.0) / (stats.remote_bytes as f64)
                },
                duration: format_duration(stats.local_duration.as_secs()),
                syncs,
                transcode_profile: server.transcode_profile().await,
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
    ThumbnailUpdate(Thumbnail),
}

impl Event {
    fn event_name(&self) -> String {
        match self {
            Self::SyncStart | Self::SyncEnd => "sync-status".to_owned(),
            Self::Log(_) => "sync-log".to_owned(),
            Self::SyncChange => "sync-change".to_owned(),
            Self::Progress(_) => "sync-progress".to_owned(),
            Self::ThumbnailUpdate(thumb) => format!("thumbnail-{}", thumb.id),
        }
    }

    pub async fn event_data(&self, flick_sync: &FlickSync) -> Result<String, askama::Error> {
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
                    profiles: Vec<String>,
                }

                let mut profiles = flick_sync.transcode_profiles().await;
                profiles.sort();

                let template = SyncList {
                    servers: ServerTemplate::build(flick_sync).await,
                    profiles,
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
            Self::ThumbnailUpdate(thumb) => {
                let template = ThumbnailTemplate { thumbnail: thumb };
                template.render()
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
    SyncStarted(String),
    SyncFailed((String, String)),
    SyncFinished((String, bool)),
    DownloadStarted(Video),
    DownloadComplete(Video),
    DownloadFailed((Video, String)),
    TranscodeStarted(Video),
    TranscodeComplete(Video),
    TranscodeFailed((Video, String)),
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
                message: format!("Syncing started for {server}."),
            },
            SyncLogMessage::SyncFailed((server, message)) => SyncLogTemplate {
                timestamp,
                message_type: "error",
                message: format!("Syncing failed for {server}: {message}"),
            },
            SyncLogMessage::SyncFinished((server, complete)) => {
                if *complete {
                    SyncLogTemplate {
                        timestamp,
                        message_type: "success",
                        message: format!("Syncing finished for {server}."),
                    }
                } else {
                    SyncLogTemplate {
                        timestamp,
                        message_type: "success",
                        message: format!(
                            "Syncing finished for {server}, some items were not fully synced.",
                        ),
                    }
                }
            }
            SyncLogMessage::DownloadStarted(video_part) => SyncLogTemplate {
                timestamp,
                message_type: "info",
                message: format!("Download started for {}.", video_part.title().await),
            },
            SyncLogMessage::DownloadComplete(video_part) => SyncLogTemplate {
                timestamp,
                message_type: "success",
                message: format!("Download complete for {}.", video_part.title().await),
            },
            SyncLogMessage::DownloadFailed((video_part, message)) => SyncLogTemplate {
                timestamp,
                message_type: "error",
                message: format!(
                    "Download failed for {}: {message}",
                    video_part.title().await
                ),
            },
            SyncLogMessage::TranscodeStarted(video_part) => SyncLogTemplate {
                timestamp,
                message_type: "info",
                message: format!("Transcode started for {}.", video_part.title().await),
            },
            SyncLogMessage::TranscodeComplete(video_part) => SyncLogTemplate {
                timestamp,
                message_type: "success",
                message: format!("Transcode complete for {}.", video_part.title().await),
            },
            SyncLogMessage::TranscodeFailed((video_part, message)) => SyncLogTemplate {
                timestamp,
                message_type: "error",
                message: format!(
                    "Transcode failed for {}: {message}",
                    video_part.title().await
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
    pub(super) video: Video,
    pub(super) position: u64,
    pub(super) length: Option<u64>,
}

impl SyncProgressBar {
    pub(crate) async fn template(&self) -> ProgressBarTemplate {
        ProgressBarTemplate {
            id: self.video.id().to_owned(),
            is_download: self.is_download,
            video: self.video.title().await,
            position: self.position,
            length: self.length,
        }
    }
}
