use actix_web::{Responder, get};
use tracing::trace;

#[get("/device.xml")]
async fn device_root() -> impl Responder {
    trace!("Saw request for device root");
    format!("Hello!")
}
