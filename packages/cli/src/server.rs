use async_trait::async_trait;
use clap::Args;
use flick_sync::{
    plex_api::{
        self,
        device::{Device, DeviceConnection},
        library::{Item, MetadataItem},
        media_container::devices::Feature,
        MyPlexBuilder, Server,
    },
    FlickSync, ServerConnection,
};
use tracing::error;
use url::Url;

use crate::{error::err, select_servers, Console, Error, Result, Runnable};

#[derive(Args)]
pub struct Login {
    /// An identifier for the server.
    id: String,
}

#[async_trait]
impl Runnable for Login {
    async fn run(self, flick_sync: FlickSync, console: Console) -> Result {
        match flick_sync.server(&self.id).await {
            Some(_server) => {
                todo!();
            }
            None => {
                let client = flick_sync.client().await;

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

                    flick_sync.add_server(&self.id, server, connection).await?;
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

                    flick_sync.add_server(&self.id, server, connection).await?;
                }
            }
        }

        Ok(())
    }
}

#[derive(Args)]
pub struct Add {
    /// The web url of the item to add to the list to sync.
    url: String,
    /// The transcode profile to use for this item.
    profile: Option<String>,
}

#[async_trait]
impl Runnable for Add {
    async fn run(self, flick_sync: FlickSync, console: Console) -> Result {
        let unexpected = || Error::ErrorMessage("Unexpected URL format".to_string());

        let url = Url::parse(&self.url)?;
        let fragment = url.fragment().ok_or_else(unexpected)?;
        if fragment.get(0..1) != Some("!") {
            return Err(unexpected());
        }
        let fragment = &fragment[1..];

        let url = Url::options()
            .base_url(Some(&Url::parse("https://nowhere.flick-sync")?))
            .parse(fragment)?;

        let mut segments = url.path_segments().ok_or_else(unexpected)?;
        if !matches!(segments.next(), Some("server")) {
            return Err(unexpected());
        }

        let id = segments.next().ok_or_else(unexpected)?;
        let key = url
            .query_pairs()
            .find_map(|(k, v)| if k == "key" { Some(v) } else { None })
            .ok_or_else(unexpected)?;

        let rating_key = match key.rfind('/') {
            Some(idx) => key[idx + 1..].parse::<u32>().map_err(|_| unexpected())?,
            None => return Err(unexpected()),
        };

        for server in flick_sync.servers().await {
            let plex_server = match server.connect().await {
                Ok(s) => s,
                Err(e) => {
                    error!(server=server.id(), error=?e, "Unable to connect to server");
                    continue;
                }
            };

            if plex_server.machine_identifier() != id {
                continue;
            }

            let item = plex_server.item_by_id(rating_key).await?;
            if matches!(
                item,
                Item::Photo(_)
                    | Item::Artist(_)
                    | Item::MusicAlbum(_)
                    | Item::Track(_)
                    | Item::PhotoPlaylist(_)
                    | Item::MusicPlaylist(_)
                    | Item::UnknownItem(_)
            ) {
                return Err(Error::UnsupportedType(item.title().to_owned()));
            }

            console.println(format!(
                "Adding '{}' to the sync list for {}",
                item.title(),
                server.id(),
            ));

            server.add_sync(rating_key, self.profile).await?;

            return Ok(());
        }

        Err(Error::ErrorMessage("No matching server found".to_string()))
    }
}

#[derive(Args)]
pub struct List {
    /// The servers to list. Can be repeated. When not passed all servers are listed.
    #[clap(short = 's', long = "server")]
    ids: Vec<String>,
}

#[async_trait]
impl Runnable for List {
    async fn run(self, flick_sync: FlickSync, _console: Console) -> Result {
        let servers = select_servers(&flick_sync, &self.ids).await?;

        for server in servers {
            if let Err(e) = server.update_state().await {
                error!(server=server.id(), error=?e, "Failed to update server");
            }
        }

        Ok(())
    }
}
