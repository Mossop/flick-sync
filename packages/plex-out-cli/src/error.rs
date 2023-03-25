use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("{source}")]
    Io {
        #[from]
        source: std::io::Error,
    },
    #[error("{source}")]
    PlexOut {
        #[from]
        source: plex_out::Error,
    },
    #[error("{source}")]
    Plex {
        #[from]
        source: plex_out::plex_api::Error,
    },
    #[error("{0}")]
    ErrorMessage(String),
    #[error("Unknown error")]
    Unknown,
}

pub fn err<T, S: ToString>(s: S) -> Result<T, Error> {
    Err(Error::ErrorMessage(s.to_string()))
}
