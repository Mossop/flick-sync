use std::{cmp::Ordering, collections::HashMap};

use plex_api::transcode::VideoTranscodeOptions;
use serde::{Deserialize, Serialize};

use crate::util::{derive_list_item, from_list, into_list, ListItem};

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
    pub(crate) only_unread: bool,
}

derive_list_item!(SyncItem);

#[derive(Deserialize, Serialize, Clone, Debug)]
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
    pub(crate) profile: Option<String>,
}

#[derive(Deserialize, Serialize, Default, Clone, Debug, PartialEq)]
pub(crate) struct TranscodeProfile {
    /// Maximum bitrate in kbps.
    pub(crate) bitrate: Option<u32>,
    /// width, height.
    pub(crate) dimensions: Option<(u32, u32)>,
}

impl TranscodeProfile {
    pub(crate) fn options(&self) -> VideoTranscodeOptions {
        let mut options = VideoTranscodeOptions::default();

        if let Some(br) = self.bitrate {
            options.bitrate = br;
        }

        if let Some(dim) = self.dimensions {
            options.width = dim.0;
            options.height = dim.1;
        }

        options
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

#[derive(Deserialize, Serialize, Default, Clone, Debug)]
pub(crate) struct Config {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) max_downloads: Option<usize>,
    #[serde(default)]
    pub(crate) servers: HashMap<String, ServerConfig>,
    pub(crate) device: Option<String>,
    #[serde(default)]
    pub(crate) profiles: HashMap<String, TranscodeProfile>,
}
