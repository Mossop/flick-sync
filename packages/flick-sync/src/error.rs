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
    ItemNotFound(String),
    #[error("Item {0} is not supported.")]
    ItemNotSupported(String),
    #[error("Plex returned incomplete information for item {0}: {1}")]
    ItemIncomplete(String, String),
    #[error("The item appears to be missing on the server")]
    MissingItem,
    #[error("Cannot download an item until the item is available (call wait_for_download)")]
    DownloadUnavailable,
    #[error("Server dropped the transcode session")]
    TranscodeLost,
    #[error("Server transcode failed")]
    TranscodeFailed,
    #[error("Unknown transcode profile {0}")]
    UnknownProfile(String),
    #[error("Unknown error")]
    Unknown(String),
}

impl From<Error> for String {
    fn from(value: Error) -> Self {
        value.to_string()
    }
}
