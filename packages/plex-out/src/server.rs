use std::sync::Arc;

use plex_api::{device::DeviceConnection, MyPlexBuilder};

use crate::{Error, Inner, Result, ServerConnection};

pub struct Server {
    pub(crate) id: String,
    pub(crate) inner: Arc<Inner>,
}

impl Server {
    pub async fn connect(&self) -> Result<plex_api::Server> {
        let config = self.inner.config.read().await;
        let state = self.inner.state.read().await;

        let server_config = config.servers.get(&self.id).unwrap();
        let token = state.servers.get(&self.id).unwrap().token.clone();

        let client = self.inner.client().await;

        match &server_config.connection {
            ServerConnection::MyPlex { username: _, id } => {
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

                Err(Error::ServerNotFound)
            }
            ServerConnection::Direct { url } => Ok(plex_api::Server::new(url, client).await?),
        }
    }
}
