use std::{cmp::Ordering, collections::HashMap};

use plex_api::{
    media_container::server::library::{AudioCodec, ContainerFormat, VideoCodec},
    transcode::{
        AudioSetting, Constraint, ContainerSetting, Limitation, VideoSetting, VideoTranscodeOptions,
    },
};
use serde::{Deserialize, Serialize};
use serde_plain::derive_display_from_serialize;

use crate::{
    Result,
    schema::{JsonObject, MigratableStore},
    util::{ListItem, derive_list_item, from_list, into_list},
};

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(tag = "type")]
pub enum ServerConnection {
    MyPlex {
        username: String,
        user_id: String,
        device_id: String,
    },
    Direct {
        url: String,
    },
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncItem {
    pub(crate) id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) transcode_profile: Option<String>,
    #[serde(default)]
    pub(crate) only_unplayed: bool,
}

derive_list_item!(SyncItem);

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ServerConfig {
    pub(crate) connection: ServerConnection,
    #[serde(
        default,
        skip_serializing_if = "HashMap::is_empty",
        serialize_with = "into_list",
        deserialize_with = "from_list"
    )]
    pub(crate) syncs: HashMap<String, SyncItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) max_transcodes: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) transcode_profile: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum H264Profile {
    Baseline,
    Main,
    High,
}

derive_display_from_serialize!(H264Profile);

#[derive(Deserialize, Serialize, Default, Clone, Debug, PartialEq)]
pub(crate) struct TranscodeProfile {
    /// Maximum bitrate in kbps.
    pub(crate) bitrate: Option<u32>,
    /// width, height.
    pub(crate) dimensions: Option<(u32, u32)>,
    /// Valid video container formats.
    pub(crate) containers: Option<Vec<ContainerFormat>>,
    /// Valid video codecs.
    pub(crate) video_codecs: Option<Vec<VideoCodec>>,
    /// Valid audio codecs.
    pub(crate) audio_codecs: Option<Vec<AudioCodec>>,
    /// Audio channels
    pub(crate) audio_channels: Option<u16>,
    /// Allowable h264 profiles (baseline, main, high)
    pub(crate) h264_profiles: Option<Vec<H264Profile>>,
    /// Maximum h264 level
    pub(crate) h264_level: Option<String>,
}

impl TranscodeProfile {
    pub(crate) fn options(&self) -> VideoTranscodeOptions {
        let mut video_limitations: Vec<Limitation<VideoCodec, VideoSetting>> = Vec::new();
        let mut audio_limitations: Vec<Limitation<AudioCodec, AudioSetting>> = Vec::new();
        let mut container_limitations: Vec<Limitation<ContainerFormat, ContainerSetting>> =
            Vec::new();

        if let Some(channels) = self.audio_channels {
            audio_limitations.push(
                (
                    AudioSetting::Channels,
                    Constraint::Max(channels.to_string()),
                )
                    .into(),
            );
        }

        if let Some(ref profiles) = self.h264_profiles {
            video_limitations.push(
                (
                    VideoCodec::H264,
                    VideoSetting::Profile,
                    Constraint::MatchList(profiles.iter().map(|p| p.to_string()).collect()),
                )
                    .into(),
            );
        }

        if let Some(ref level) = self.h264_level {
            video_limitations.push(
                (
                    VideoCodec::H264,
                    VideoSetting::Level,
                    Constraint::Max(level.clone()),
                )
                    .into(),
            );
        }

        let mut limit: Limitation<ContainerFormat, ContainerSetting> = (
            ContainerFormat::Mp4,
            ContainerSetting::OptimizedForStreaming,
            Constraint::Match("1".to_string()),
        )
            .into();
        limit.is_required = true;
        container_limitations.push(limit);

        let (width, height) = self.dimensions.unwrap_or((1280, 720));

        VideoTranscodeOptions {
            bitrate: self.bitrate.unwrap_or(2000),
            width,
            height,
            audio_boost: None,
            burn_subtitles: true,
            containers: self
                .containers
                .clone()
                .unwrap_or_else(|| vec![ContainerFormat::Mp4]),
            container_limitations,
            video_codecs: self
                .video_codecs
                .clone()
                .unwrap_or_else(|| vec![VideoCodec::H264]),
            video_limitations,
            audio_codecs: self
                .audio_codecs
                .clone()
                .unwrap_or_else(|| vec![AudioCodec::Aac, AudioCodec::Mp3]),
            audio_limitations,
        }
    }
}

impl PartialOrd for TranscodeProfile {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        let mut fallback: Option<Ordering> = None;

        if let (Some(a), Some(b)) = (&self.bitrate, &other.bitrate) {
            if a != b {
                return Some(a.cmp(b));
            }
            fallback = Some(Ordering::Equal);
        }

        if let (Some((ax, ay)), Some((bx, by))) = (self.dimensions, other.dimensions) {
            return Some((ax * ay).cmp(&(bx * by)));
        }

        fallback
    }
}

#[derive(Default, Deserialize, Debug, Serialize, PartialEq, Clone, Copy, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum OutputStyle {
    #[default]
    Minimal,
    Standardized,
}

impl OutputStyle {
    fn is_default(&self) -> bool {
        matches!(self, OutputStyle::Minimal)
    }
}

#[derive(Deserialize, Serialize, Default, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Config {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) max_downloads: Option<usize>,
    #[serde(default)]
    pub(crate) servers: HashMap<String, ServerConfig>,
    pub(crate) device: Option<String>,
    #[serde(default)]
    pub(crate) profiles: HashMap<String, TranscodeProfile>,
    #[serde(default, skip_serializing_if = "OutputStyle::is_default")]
    pub(crate) output_style: OutputStyle,
}

impl MigratableStore for Config {
    fn migrate(_data: &mut JsonObject) -> Result<bool> {
        Ok(false)
    }
}
