use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("{source}")]
    IoError {
        #[from]
        source: std::io::Error,
    },
    #[error("Unable to deserialize JSON: {source}.")]
    DeserealiseError {
        #[from]
        source: serde_json::Error,
    },
    #[error("A server with this identifier already exists")]
    ServerExists,
    #[error("Unknown error")]
    Unknown,
}

impl From<Error> for String {
    fn from(value: Error) -> Self {
        value.to_string()
    }
}
