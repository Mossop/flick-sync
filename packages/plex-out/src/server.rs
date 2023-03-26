use std::sync::Arc;

use plex_api::{device::DeviceConnection, MyPlexBuilder};

use crate::{Error, Inner, Result, ServerConnection};

pub struct Server {
    pub(crate) id: String,
    pub(crate) inner: Arc<Inner>,
}

impl Server {
    /// The PlexOut identifier for this server.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Connects to the Plex API for this server.
    pub async fn connect(&self) -> Result<plex_api::Server> {
        let config = self.inner.config.read().await;
        let state = self.inner.state.read().await;

        let server_config = config.servers.get(&self.id).unwrap();

        let mut client = self.inner.client().await;

        match &server_config.connection {
            ServerConnection::MyPlex { username: _, id } => {
                let token = state
                    .servers
                    .get(&self.id)
                    .ok_or_else(|| Error::ServerNotAuthenticated)?
                    .token
                    .clone();

                let myplex = MyPlexBuilder::default()
                    .set_client(client)
                    .set_token(token)
                    .build()
                    .await?;

                let manager = myplex.device_manager()?;
                for device in manager.devices().await? {
                    if device.identifier() == id {
                        match device.connect().await? {
                            DeviceConnection::Server(server) => return Ok(*server),
                            _ => panic!("Unexpected client connection"),
                        }
                    }
                }

                Err(Error::MyPlexServerNotFound)
            }
            ServerConnection::Direct { url } => {
                let token = state
                    .servers
                    .get(&self.id)
                    .map(|s| s.token.clone())
                    .unwrap_or_default();
                client = client.set_x_plex_token(token);

                Ok(plex_api::Server::new(url, client).await?)
            }
        }
    }

    /// Adds an item to sync based on its rating key.
    pub async fn add_sync(&self, rating_key: u32) -> Result {
        let mut config = self.inner.config.write().await;

        let server_config = config.servers.get_mut(&self.id).unwrap();
        server_config.syncs.insert(rating_key);

        self.inner.persist_config(&config).await
    }
}
