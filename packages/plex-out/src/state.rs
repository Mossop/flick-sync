use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ServerState {
    pub token: String,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct State {
    pub client_id: String,
    #[serde(default)]
    pub servers: HashMap<String, ServerState>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            client_id: Uuid::new_v4().braced().to_string(),
            servers: Default::default(),
        }
    }
}
