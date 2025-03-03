use std::net::SocketAddr;

use actix_web::{
    Error, HttpRequest,
    body::MessageBody,
    dev::{ServiceRequest, ServiceResponse},
    middleware::Next,
};
use tracing::{Instrument, Level, debug, field, instrument, span, warn};

fn is_safe(addr: SocketAddr) -> bool {
    match addr {
        SocketAddr::V4(addr) => {
            let ip = addr.ip();
            ip.is_private() || ip.is_loopback() || ip.is_link_local()
        }
        SocketAddr::V6(addr) => addr.ip().is_loopback(),
    }
}

fn client_addr(req: &HttpRequest) -> Option<String> {
    if let Some(addr) = req.peer_addr() {
        if is_safe(addr) {
            if let Some(ip) = req.connection_info().realip_remote_addr() {
                return Some(ip.to_owned());
            }
        }

        Some(addr.to_string())
    } else {
        None
    }
}

#[instrument(skip_all)]
pub(crate) async fn middleware<B: MessageBody>(
    req: ServiceRequest,
    next: Next<B>,
) -> Result<ServiceResponse<B>, Error> {
    let http_request = req.request();

    let span = span!(
        Level::INFO,
        "HTTP request",
        "client.address" = field::Empty,
        "url.path" = req.path(),
        "user_agent.original" = field::Empty,
        "http.request.method" = %req.method(),
        "http.request.content_type" = field::Empty,
        "http.request.content_length" = field::Empty,
        "http.response.status_code" = field::Empty,
    );

    let headers = http_request.headers();

    if let Some(user_agent) = headers.get("user-agent").and_then(|h| h.to_str().ok()) {
        span.record("user_agent.original", user_agent);
    }

    if let Some(content_type) = headers.get("content-type").and_then(|h| h.to_str().ok()) {
        span.record("http.request.content_type", content_type);
    }

    if let Some(content_length) = headers.get("content-length").and_then(|h| h.to_str().ok()) {
        span.record("http.request.content_length", content_length);
    }

    let client_addr = client_addr(req.request());

    if let Some(ref ip) = client_addr {
        span.record("client.address", ip);
    }

    let res = next.call(req).instrument(span.clone()).await?;

    let status = res.status();
    span.record("http.response.status_code", status.as_u16());

    if status.is_server_error() {
        warn!(parent: &span, status = status.as_u16(), "Server failure")
    } else if status.is_client_error() {
        warn!(parent: &span, status = status.as_u16(), "Bad request")
    } else {
        debug!(parent: &span, status = status.as_u16())
    }

    Ok(res)
}
