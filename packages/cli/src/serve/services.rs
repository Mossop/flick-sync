use actix_web::{
    HttpResponse, Responder, get,
    http::header,
    web::{Html, Path},
};
use rinja::Template;

use crate::{EmbeddedFileStream, Resources, error::Error};

#[get("/resource/scripts/{path:.*}")]
pub(super) async fn scripts(path: Path<String>) -> Result<HttpResponse, Error> {
    let Some(file) = Resources::get(&format!("scripts/{path}")) else {
        return Ok(HttpResponse::NotFound().finish());
    };

    Ok(HttpResponse::Ok()
        .append_header(header::ContentLength(file.data.len()))
        .append_header(header::ContentType(mime::APPLICATION_JAVASCRIPT))
        .streaming(EmbeddedFileStream::new(file)))
}

#[get("/resource/styles/{path:.*}")]
pub(super) async fn styles(path: Path<String>) -> Result<HttpResponse, Error> {
    let Some(file) = Resources::get(&format!("styles/{path}")) else {
        return Ok(HttpResponse::NotFound().finish());
    };

    Ok(HttpResponse::Ok()
        .append_header(header::ContentLength(file.data.len()))
        .append_header(header::ContentType(mime::TEXT_CSS))
        .streaming(EmbeddedFileStream::new(file)))
}

#[get("/")]
pub(super) async fn index() -> Result<impl Responder, Error> {
    #[derive(Template)]
    #[template(path = "index.html")]
    struct Index {}

    let template = Index {};

    Ok(Html::new(template.render()?))
}
