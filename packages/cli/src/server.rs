use anyhow::{anyhow, bail};
use clap::Args;
use flick_sync::{
    FlickSync, Server, ServerConnection,
    plex_api::{
        self, HttpClient, MyPlex, MyPlexBuilder, Server as PlexServer,
        device::{Device, DeviceConnection},
        library::{Item, MetadataItem},
    },
};
use tracing::{error, warn};
use url::Url;

use crate::{Console, Result, Runnable};

#[derive(Args)]
pub struct Login {
    /// An identifier for the server.
    id: String,
    /// The default transcode profile to use for items.
    #[clap(short, long)]
    transcode_profile: Option<String>,
}

async fn myplex_auth(console: &Console, client: &HttpClient, username: &str) -> Result<MyPlex> {
    let password = console.password("Password");

    let myplex = match MyPlexBuilder::default()
        .set_client(client.clone())
        .set_username_and_password(username, password.clone())
        .build()
        .await
    {
        Ok(p) => p,
        Err(e) => {
            if matches!(e, plex_api::Error::OtpRequired) {
                let otp = console.input("OTP");
                MyPlexBuilder::default()
                    .set_client(client.clone())
                    .set_username_and_password(username, password.clone())
                    .set_otp(otp)
                    .build()
                    .await?
            } else {
                return Err(e.into());
            }
        }
    };

    Ok(myplex)
}

async fn reconnect_server(server: &Server, flick_sync: &FlickSync, console: &Console) -> Result {
    let connection = server.connection().await;

    match connection {
        ServerConnection::MyPlex {
            username,
            user_id,
            device_id,
        } => {
            let client = flick_sync.client().await;

            console.println(format!("Username: {username}"));
            let myplex = myplex_auth(console, &client, &username).await?;
            let auth_token = myplex.client().x_plex_token().to_owned();

            let home = myplex.home()?;
            let users = home.users().await?;

            let user = if let Some(user) = users.iter().find(|u| u.uuid == user_id) {
                console.println(format!("User: {}", user.title));
                user
            } else {
                let index = if users.len() == 1 {
                    console.println(format!("User: {}", users[0].title));
                    0
                } else {
                    let names = users
                        .iter()
                        .map(|u| u.title.clone())
                        .collect::<Vec<String>>();
                    console.select("Select user", &names)
                };

                &users[index]
            };

            let pin = if user.protected {
                console.input("Enter PIN")
            } else {
                "".to_string()
            };

            let myplex = home
                .switch_user(myplex, user.uuid.clone(), Some(&pin))
                .await?;

            let manager = myplex.device_manager()?;
            let device = manager
                .resources()
                .await?
                .into_iter()
                .find(|d| d.is_server() && d.identifier() == device_id);

            if let Some(device) = device {
                let server_connection = match device.connect().await? {
                    DeviceConnection::Server(server) => *server,
                    _ => panic!("Unexpected client connection"),
                };

                server
                    .update_connection(&auth_token, server_connection)
                    .await?;
            } else {
                console.println("Expected server no longer exists.")
            }
        }
        ServerConnection::Direct { .. } => {
            console.println("No need to re-authenticate with direct connection.");
        }
    }
    Ok(())
}

async fn create_server(args: Login, flick_sync: FlickSync, console: Console) -> Result {
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

        let server = PlexServer::new(url.clone(), client).await?;

        let connection = ServerConnection::Direct { url };
        let auth_token = server.client().x_plex_token().to_owned();

        flick_sync
            .add_server(
                &args.id,
                server,
                &auth_token,
                connection,
                args.transcode_profile,
            )
            .await?;
    } else {
        let username = console.input("Username");
        let myplex = myplex_auth(&console, &client, &username).await?;
        let auth_token = myplex.client().x_plex_token().to_owned();

        let home = myplex.home()?;
        let users = home.users().await?;

        let index = if users.len() == 1 {
            console.println(format!("User: {}", users[0].title));
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

        let myplex = home
            .switch_user(myplex, user.uuid.clone(), Some(&pin))
            .await?;

        let manager = myplex.device_manager()?;
        let devices: Vec<Device<'_>> = manager
            .resources()
            .await?
            .into_iter()
            .filter(|d| d.is_server())
            .collect();

        let device = if devices.is_empty() {
            bail!("No servers found in this account");
        } else if devices.len() == 1 {
            &devices[0]
        } else {
            let names: Vec<String> = devices.iter().map(|d| d.name().to_owned()).collect();
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
            user_id: user.uuid.clone(),
            device_id: server.machine_identifier().to_owned(),
        };

        flick_sync
            .add_server(
                &args.id,
                server,
                &auth_token,
                connection,
                args.transcode_profile,
            )
            .await?;
    }

    Ok(())
}

impl Runnable for Login {
    async fn run(self, flick_sync: FlickSync, console: Console) -> Result {
        match flick_sync.server(&self.id).await {
            Some(server) => reconnect_server(&server, &flick_sync, &console).await,
            None => create_server(self, flick_sync, console).await,
        }
    }
}

#[derive(Args)]
pub struct Add {
    /// The web url of the item to add to the list to sync.
    url: String,
    /// The transcode profile to use for this item.
    #[clap(short, long)]
    profile: Option<String>,
    /// Only sync unplayed items
    #[clap(short, long)]
    only_unplayed: bool,
}

impl Runnable for Add {
    async fn run(self, flick_sync: FlickSync, console: Console) -> Result {
        let unexpected = || anyhow!("Unexpected URL format");

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
            Some(idx) => key[idx + 1..].to_owned(),
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

            let item = plex_server.item_by_id(&rating_key).await?;
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
                bail!("Unsupported type: {}", item.title());
            }

            console.println(format!(
                "Adding '{}' to the sync list for {}",
                item.title(),
                server.id(),
            ));

            server
                .add_sync(&rating_key, self.profile, self.only_unplayed)
                .await?;

            return Ok(());
        }

        Err(anyhow!("No matching server found"))
    }
}

#[derive(Args)]
pub struct Remove {
    /// The server to remove from.
    server: String,
    /// The id item to remove.
    id: String,
}

impl Runnable for Remove {
    async fn run(self, flick_sync: FlickSync, console: Console) -> Result {
        let server = if let Some(server) = flick_sync.server(&self.server).await {
            server
        } else {
            console.println(format!("{} is not a known server.", self.server));
            return Ok(());
        };

        if server.remove_sync(&self.id).await? {
            if let Err(e) = server.update_state(false).await {
                error!(server=server.id(), error=?e, "Failed to update server");
                return Ok(());
            }

            if let Err(e) = server.prune().await {
                error!(server=server.id(), error=?e, "Failed to prune server directory");
                return Ok(());
            }
        }

        Ok(())
    }
}

#[derive(Args)]
pub struct Rebuild {}

impl Runnable for Rebuild {
    async fn run(self, flick_sync: FlickSync, console: Console) -> Result {
        for server in flick_sync.servers().await {
            if let Err(e) = reconnect_server(&server, &flick_sync, &console).await {
                error!(server=server.id(), error=?e, "Failed to reconnect server");
                continue;
            }

            if let Err(e) = server.update_state(false).await {
                error!(server=server.id(), error=?e, "Failed to update server");
                continue;
            }

            for video in server.videos().await {
                for part in video.parts().await {
                    if part.is_downloaded().await {
                        continue;
                    }

                    if let Err(e) = part.rebuild_download().await {
                        warn!(error=?e, "Failed to relocate video part");
                    }
                }
            }
        }

        Ok(())
    }
}
