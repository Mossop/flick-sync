use actix_web::{HttpRequest, HttpResponse, Responder, ResponseError, http::StatusCode, web::Html};
use rinja::Template;
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
    #[error("Template error: {source}")]
    Render {
        #[from]
        source: rinja::Error,
    },
    #[error("{0}")]
    Generic(String),
    #[error("Unknown error")]
    Unknown,
}

pub fn err<T, S: ToString>(s: S) -> Result<T, Error> {
    Err(Error::Generic(s.to_string()))
}

impl ResponseError for Error {
    fn status_code(&self) -> StatusCode {
        StatusCode::INTERNAL_SERVER_ERROR
    }
}

impl Responder for Error {
    type Body = String;

    fn respond_to(self, req: &HttpRequest) -> HttpResponse<Self::Body> {
        #[derive(Debug, Template)]
        #[template(path = "error.html")]
        struct T {
            error_message: String,
        }

        let tmpl = T {
            error_message: self.to_string(),
        };

        match tmpl.render() {
            Ok(body) => (Html::new(body), self.status_code()).respond_to(req),
            Err(e) => (e.to_string(), StatusCode::INTERNAL_SERVER_ERROR).respond_to(req),
        }
    }
}
