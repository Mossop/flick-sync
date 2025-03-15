use std::net::SocketAddr;

use actix_web::{
    HttpRequest,
    body::BoxBody,
    dev::{ServiceRequest, ServiceResponse},
    http::header::{self, HeaderMap, HeaderName},
    middleware::Next,
};
use tracing::{Instrument, Level, Span, field, instrument, span, trace, warn};

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

fn record_header(span: &Span, headers: &HeaderMap, header: HeaderName, field: &'static str) {
    if let Some(value) = headers.get(header).and_then(|h| h.to_str().ok()) {
        span.record(field, value);
    }
}

#[instrument(skip_all)]
pub(crate) async fn middleware(
    req: ServiceRequest,
    next: Next<BoxBody>,
) -> Result<ServiceResponse<BoxBody>, actix_web::Error> {
    let http_request = req.request();
    if http_request.path().starts_with("/upnp/") {
        return next.call(req).await;
    }

    let span = span!(
        Level::INFO,
        "HTTP request",
        "client.address" = field::Empty,
        "url.path" = req.path(),
        "user_agent.original" = field::Empty,
        "http.request.method" = %req.method(),
        "http.response.status_code" = field::Empty,
        "http.response.content_length" = field::Empty,
        "http.response.content_type" = field::Empty,
    );

    let headers = http_request.headers();

    record_header(&span, headers, header::USER_AGENT, "user_agent.original");

    let client_addr = client_addr(req.request());

    if let Some(ref ip) = client_addr {
        span.record("client.address", ip);
    }

    let res = next.call(req).instrument(span.clone()).await?;

    let status = res.status();
    span.record("http.response.status_code", status.as_u16());

    let headers = res.response().headers();

    record_header(
        &span,
        headers,
        header::CONTENT_TYPE,
        "http.response.content_type",
    );
    record_header(
        &span,
        headers,
        header::CONTENT_LENGTH,
        "http.response.content_length",
    );

    if status.is_server_error() {
        warn!(parent: &span, status = status.as_u16(), "Server failure")
    } else if status.is_client_error() {
        warn!(parent: &span, status = status.as_u16(), "Bad request")
    } else {
        trace!(parent: &span, status = status.as_u16())
    }

    Ok(res)
}
