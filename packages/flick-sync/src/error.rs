use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("{source}")]
    IoError {
        #[from]
        source: std::io::Error,
    },
    #[error("Unable to deserialize JSON: {source}")]
    DeserealiseError {
        #[from]
        source: serde_json::Error,
    },
    #[error("The Plex API returned an error: {source}")]
    PlexError {
        #[from]
        source: plex_api::Error,
    },
    #[error("A server with this identifier already exists")]
    ServerExists,
    #[error("The server is no longer registered to this account")]
    MyPlexServerNotFound,
    #[error("This server is no longer authenticated correctly. Try logging in again")]
    ServerNotAuthenticated,
    #[error("Item {0} was not found on the server")]
    ItemNotFound(u32),
    #[error("Item {0} is not supported.")]
    ItemNotSupported(u32),
    #[error("Plex returned incomplete information for item {0}: {1}")]
    ItemIncomplete(u32, String),
    #[error("Unknown error")]
    Unknown(String),
}

impl From<Error> for String {
    fn from(value: Error) -> Self {
        value.to_string()
    }
}
