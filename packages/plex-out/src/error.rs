use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Unknown error")]
    Unknown,
}

impl From<Error> for String {
    fn from(value: Error) -> Self {
        value.to_string()
    }
}
