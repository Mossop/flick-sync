use thiserror::Error;

use crate::sync::Timeout;

#[derive(Debug, Error)]
pub enum Error {
    #[error("{source}")]
    IoError {
        #[from]
        source: std::io::Error,
    },
    #[error("Unable to deserialize JSON: {source}")]
    DeserealizeError {
        #[from]
        source: serde_json::Error,
    },
    #[error("Unexpected schema version")]
    SchemaError,
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
    #[error("Transcoding was skipped")]
    TranscodeSkipped,
    #[error("Unknown transcode profile {0}")]
    UnknownProfile(String),
    #[error("Error writing metadata: {source}")]
    XmlError {
        #[from]
        source: xml::writer::Error,
    },
    #[error("Unable to lock, other operations are in progress")]
    LockFailed,
    #[error("Unknown error")]
    Unknown(String),
}

impl From<Timeout> for Error {
    fn from(_: Timeout) -> Error {
        Error::LockFailed
    }
}

impl From<Error> for String {
    fn from(value: Error) -> Self {
        value.to_string()
    }
}
