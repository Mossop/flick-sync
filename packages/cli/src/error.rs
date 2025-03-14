use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("{source}")]
    Io {
        #[from]
        source: std::io::Error,
    },
    #[error("{source}")]
    Url {
        #[from]
        source: url::ParseError,
    },
    #[error("{source}")]
    FlickSync {
        #[from]
        source: flick_sync::Error,
    },
    #[error("{source}")]
    Plex {
        #[from]
        source: flick_sync::plex_api::Error,
    },
    #[error("Unable to sync '{0}'")]
    UnsupportedType(String),
    #[error("Unknown server {0}")]
    UnknownServer(String),
    #[error("{source:?}")]
    Any {
        #[from]
        source: anyhow::Error,
    },
    #[error("Web server error: {source}")]
    WebServer {
        #[from]
        source: flick_sync_webserver::Error,
    },
    #[error("{0}")]
    Generic(String),
    #[error("Unknown error")]
    Unknown,
}

pub fn err<T, S: ToString>(s: S) -> Result<T, Error> {
    Err(Error::Generic(s.to_string()))
}
