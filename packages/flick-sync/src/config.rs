use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(tag = "type")]
pub enum ServerConnection {
    MyPlex { username: String, id: String },
    Direct { url: String },
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub(crate) struct ServerConfig {
    pub(crate) device: Option<String>,
    pub(crate) connection: ServerConnection,
    #[serde(default)]
    pub(crate) syncs: HashSet<u32>,
}

#[derive(Deserialize, Serialize, Default, Clone, Debug)]
pub(crate) struct Config {
    #[serde(default)]
    pub(crate) servers: HashMap<String, ServerConfig>,
}
