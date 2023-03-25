use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(tag = "type")]
pub enum ServerConnection {
    MyPlex { username: String, id: String },
    Direct { url: String },
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ServerConfig {
    pub connection: ServerConnection,
}

#[derive(Deserialize, Serialize, Default, Clone, Debug)]
pub struct Config {
    #[serde(default)]
    pub servers: HashMap<String, ServerConfig>,
}
