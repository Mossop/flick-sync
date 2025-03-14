use std::net::Ipv4Addr;

use actix_web::{
    App, HttpServer, Responder,
    dev::{HttpServiceFactory, ServerHandle},
    get,
    web::Html,
};
use askama::Template;
use flick_sync::FlickSync;
use tokio::spawn;

mod error;

pub use error::Error;

#[get("/")]
async fn index() -> Result<impl Responder, Error> {
    #[derive(Template)]
    #[template(path = "index.html")]
    struct Index {}

    let template = Index {};

    Ok(Html::new(template.render()?))
}

pub fn spawn_server<S>(
    _flick_sync: FlickSync,
    upnp_service: S,
    port: u16,
) -> Result<ServerHandle, Error>
where
    S: HttpServiceFactory + Clone + Send + 'static,
{
    let http_server =
        HttpServer::new(move || App::new().service(upnp_service.clone()).service(index))
            .bind((Ipv4Addr::UNSPECIFIED, port))?
            .run();

    let handle = http_server.handle();

    spawn(http_server);

    Ok(handle)
}
