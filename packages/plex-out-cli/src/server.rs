use async_trait::async_trait;
use clap::Args;
use plex_out::{
    plex_api::{
        self,
        device::{Device, DeviceConnection},
        Feature, MyPlexBuilder, Server,
    },
    PlexOut, ServerConnection,
};

use crate::{console::Console, error::err, Result, Runnable};

#[derive(Args)]
pub struct Login {
    /// An identifier for the server.
    id: String,
}

#[async_trait]
impl Runnable for Login {
    async fn run(self, plexout: PlexOut, console: Console) -> Result {
        match plexout.server(&self.id).await {
            Some(_server) => {
                todo!();
            }
            None => {
                let client = plexout.client().await;

                let method = console.select(
                    "Select how to connect to the new server",
                    &["MyPlex", "Direct"],
                );

                if method == 1 {
                    let mut url = console.input("Enter the server address (IP:port or URL)");
                    if !url.contains("://") {
                        if !url.contains(':') {
                            url = format!("http://{}:32400", url);
                        } else {
                            url = format!("http://{}", url);
                        }
                    }

                    let server = Server::new(url.clone(), client).await?;

                    let connection = ServerConnection::Direct { url };

                    plexout.add_server(&self.id, server, connection).await?;
                } else {
                    let username = console.input("Username");
                    let password = console.password("Password");

                    let myplex = match MyPlexBuilder::default()
                        .set_client(client.clone())
                        .set_username_and_password(&username, password.clone())
                        .build()
                        .await
                    {
                        Ok(p) => p,
                        Err(e) => {
                            if matches!(e, plex_api::Error::OtpRequired) {
                                let otp = console.input("OTP");
                                MyPlexBuilder::default()
                                    .set_client(client.clone())
                                    .set_username_and_password(&username, password.clone())
                                    .set_otp(otp)
                                    .build()
                                    .await?
                            } else {
                                return Err(e.into());
                            }
                        }
                    };

                    let home = myplex.home()?;
                    let users = home.users().await?;

                    let index = if users.len() == 1 {
                        0
                    } else {
                        let names = users
                            .iter()
                            .map(|u| u.title.clone())
                            .collect::<Vec<String>>();
                        console.select("Select user", &names)
                    };

                    let user = &users[index];
                    let pin = if user.protected {
                        console.input("Enter PIN")
                    } else {
                        "".to_string()
                    };

                    let myplex = home.switch_user(myplex, user, Some(&pin)).await?;

                    let manager = myplex.device_manager()?;
                    let devices: Vec<Device<'_>> = manager
                        .devices()
                        .await?
                        .into_iter()
                        .filter(|d| d.provides(Feature::Server))
                        .collect();

                    let device = if devices.is_empty() {
                        return err("No servers found in this account");
                    } else if devices.len() == 1 {
                        &devices[0]
                    } else {
                        let names: Vec<String> =
                            devices.iter().map(|d| d.name().to_owned()).collect();
                        let index = console.select("Select server", &names);
                        &devices[index]
                    };

                    console.println(format!("Got device {}", device.identifier()));

                    let server = match device.connect().await? {
                        DeviceConnection::Server(server) => *server,
                        _ => panic!("Unexpected client connection"),
                    };

                    let connection = ServerConnection::MyPlex {
                        username,
                        id: server.machine_identifier().to_owned(),
                    };

                    plexout.add_server(&self.id, server, connection).await?;
                }
            }
        }

        Ok(())
    }
}
