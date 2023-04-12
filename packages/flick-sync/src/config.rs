use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::util::{derive_list_item, from_list, into_list, ListItem};

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(tag = "type")]
pub enum ServerConnection {
    MyPlex { username: String, id: String },
    Direct { url: String },
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub(crate) struct SyncItem {
    pub(crate) id: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) transcode_profile: Option<String>,
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
    pub(crate) syncs: HashMap<u32, SyncItem>,
}

#[derive(Deserialize, Serialize, Default, Clone, Debug)]
pub(crate) struct TranscodeProfile {}

#[derive(Deserialize, Serialize, Default, Clone, Debug)]
pub(crate) struct Config {
    #[serde(default)]
    pub(crate) servers: HashMap<String, ServerConfig>,
    pub(crate) device: Option<String>,
    #[serde(default)]
    pub(crate) profiles: HashMap<String, TranscodeProfile>,
}
