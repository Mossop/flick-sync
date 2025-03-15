use actix_web::{
    HttpResponse, Responder, get,
    http::header,
    web::{Html, Path},
};
use rinja::Template;

use crate::{EmbeddedFileStream, Resources, error::Error};

#[get("/resources/{path:.*}")]
pub(super) async fn resources(path: Path<String>) -> Result<HttpResponse, Error> {
    let Some(file) = Resources::get(&format!("{path}")) else {
        return Ok(HttpResponse::NotFound().finish());
    };

    let mime = match path.rsplit_once('.') {
        Some((_, "js")) => mime::APPLICATION_JAVASCRIPT,
        Some((_, "css")) => mime::TEXT_CSS,
        _ => mime::APPLICATION_OCTET_STREAM,
    };

    Ok(HttpResponse::Ok()
        .append_header(header::ContentLength(file.data.len()))
        .append_header(header::ContentType(mime))
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
