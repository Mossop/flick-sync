use std::io;

use actix_web::{HttpRequest, HttpResponse, Responder, ResponseError, http::StatusCode, web::Html};
use askama::Template;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("IO Error: {0}")]
    Io(#[from] io::Error),
    #[error("Failed to render template: {0}")]
    Render(#[from] askama::Error),
}

impl ResponseError for Error {
    fn status_code(&self) -> StatusCode {
        match self {
            Error::Io(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Error::Render(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl Responder for Error {
    type Body = String;

    fn respond_to(self, req: &HttpRequest) -> HttpResponse<Self::Body> {
        #[derive(Debug, Template)]
        #[template(path = "error.html")]
        struct T {}

        let tmpl = T {};
        if let Ok(body) = tmpl.render() {
            (Html::new(body), self.status_code()).respond_to(req)
        } else {
            (String::new(), self.status_code()).respond_to(req)
        }
    }
}
