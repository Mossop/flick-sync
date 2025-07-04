use std::{
    cmp,
    convert::Infallible,
    io::{self, SeekFrom},
    net::SocketAddr,
    pin::Pin,
    str::FromStr,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    task::{Context, Poll},
};

use actix_web::{
    Error, FromRequest, HttpMessage, HttpRequest, HttpResponse, HttpResponseBuilder, Responder,
    Scope,
    body::{BoxBody, SizedStream},
    dev::{AppService, HttpServiceFactory, ServiceRequest, ServiceResponse},
    get,
    http::{
        StatusCode,
        header::{
            self, ByteRangeSpec, CacheDirective, HeaderMap, HeaderName, HeaderValue,
            TryIntoHeaderValue,
        },
    },
    middleware::{Next, from_fn},
    web::{self, Data, Path, Payload, ReqData},
};
use futures::Stream;
use mime::Mime;
use pin_project::pin_project;
use serde::{Deserialize, Serialize};
use serde_with::{StringWithSeparator, formats::CommaSeparator, serde_as};
use tokio::io::{AsyncRead, AsyncSeek, AsyncSeekExt, ReadBuf};
use tokio_util::io::ReaderStream;
use tracing::{Instrument, Level, Span, debug, error, field, instrument, span, trace, warn};
use uuid::Uuid;

use crate::{
    DlnaRequestHandler, UpnpError, ns,
    soap::{ArgDirection, RequestContext, SoapAction, SoapArgument, SoapResult},
    upnp,
    xml::Xml,
};

const CACHE_AGE: u32 = 10 * 60;
const BUFFER_CAPACITY: usize = 8 * 1024;

#[pin_project]
struct TelemetryStream<R> {
    #[pin]
    stream: ReaderStream<R>,
    total: u64,
    span: Span,
}

impl<R> TelemetryStream<R>
where
    R: AsyncRead,
{
    fn new(reader: R) -> Self {
        Self {
            stream: ReaderStream::with_capacity(reader, BUFFER_CAPACITY),
            span: span!(Level::DEBUG, "TelemetryStream"),
            total: 0,
        }
    }
}

impl<R> Stream for TelemetryStream<R>
where
    R: AsyncRead,
{
    type Item = <ReaderStream<ByteRangeResponse<R>> as Stream>::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.project();

        let span = this.span.clone();
        let _span = span.clone().entered();

        let result = this.stream.poll_next(cx);

        match &result {
            Poll::Ready(Some(Ok(bytes))) => {
                *this.total += bytes.len() as u64;

                if bytes.len() < BUFFER_CAPACITY {
                    debug!(parent: &span, total = this.total, bytes = bytes.len(), "Under read of bytes from resource");
                }
            }
            Poll::Ready(Some(Err(error))) => {
                error!(parent: &span, %error, "Error streaming data");
            }
            Poll::Ready(None) => {
                trace!(parent: &span, total = this.total, "Streaming complete");
            }
            Poll::Pending => {
                // trace!(parent: &span, total = this.total, "Unable to supply data yet");
            }
        }

        result
    }
}

#[pin_project]
struct ByteRangeResponse<R> {
    #[pin]
    reader: R,
    total: u64,
    remaining: u64,
    span: Option<Span>,
}

impl<R> AsyncRead for ByteRangeResponse<R>
where
    R: AsyncRead,
{
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.project();

        let span = this
            .span
            .get_or_insert_with(|| span!(Level::DEBUG, "ByteRangeResponse"))
            .clone();
        let _entered = span.clone().entered();

        if *this.remaining == 0 {
            return Poll::Ready(Ok(()));
        }

        let count = buf.filled().len();
        let result = this.reader.poll_read(cx, buf);

        match &result {
            Poll::Ready(Ok(())) => {
                let new_count = buf.filled().len();
                let filled = (new_count - count) as u64;
                *this.total += filled;

                if filled > 0 {
                    if filled < BUFFER_CAPACITY as u64 {
                        debug!(parent: &span, total = this.total, bytes = filled, "Under read of bytes from resource");
                    }

                    if *this.remaining < filled {
                        buf.set_filled(*this.remaining as usize);
                        *this.remaining = 0;
                    } else {
                        *this.remaining -= filled;
                    }
                }
            }
            Poll::Ready(Err(error)) => {
                error!(parent: &span, %error, "Error reading data");
            }
            Poll::Pending => {
                // trace!(parent: &span, total = this.total, "Unable to read data yet");
            }
        }

        result
    }
}

impl<R> ByteRangeResponse<R>
where
    R: AsyncRead + AsyncSeek + Unpin + 'static,
{
    fn get_range(request: &HttpRequest) -> Option<ByteRangeSpec> {
        // Only supports the specific case of a single byte range request.
        let range_value = request.headers().get(header::RANGE)?;
        let range_str = range_value.to_str().ok()?;
        let range = header::Range::from_str(range_str).ok()?;

        if let header::Range::Bytes(spec) = range {
            if spec.len() == 1 {
                spec.into_iter().next()
            } else {
                None
            }
        } else {
            None
        }
    }

    fn build_response(status: StatusCode, headers: HeaderMap) -> HttpResponseBuilder {
        let mut response = HttpResponse::build(status);
        response.append_header((header::ACCEPT_RANGES, "bytes"));

        for (key, value) in headers {
            response.append_header((key, value));
        }

        response
    }

    fn build_stream(reader: R, length: u64) -> SizedStream<TelemetryStream<ByteRangeResponse<R>>> {
        SizedStream::new(
            length,
            TelemetryStream::new(Self {
                reader,
                remaining: length,
                total: 0,
                span: None,
            }),
        )
    }

    async fn build(
        request: &HttpRequest,
        content_size: u64,
        mut reader: R,
        headers: HeaderMap,
    ) -> HttpResponse {
        let Some(range_spec) = Self::get_range(request) else {
            return Self::build_response(StatusCode::OK, headers)
                .body(Self::build_stream(reader, content_size));
        };

        let (start, length) = match range_spec {
            ByteRangeSpec::FromTo(start, end) => {
                if start >= content_size {
                    return HttpResponse::RangeNotSatisfiable()
                        .append_header(header::ContentRange(header::ContentRangeSpec::Bytes {
                            range: None,
                            instance_length: Some(content_size),
                        }))
                        .finish();
                }

                reader.seek(SeekFrom::Start(start)).await.unwrap();
                let end = cmp::min(end, content_size - 1);
                (start, end - start + 1)
            }
            ByteRangeSpec::From(start) => {
                if start >= content_size {
                    return HttpResponse::RangeNotSatisfiable()
                        .append_header(header::ContentRange(header::ContentRangeSpec::Bytes {
                            range: None,
                            instance_length: Some(content_size),
                        }))
                        .finish();
                }

                reader.seek(SeekFrom::Start(start)).await.unwrap();
                (start, content_size - start)
            }
            ByteRangeSpec::Last(length) => {
                let length = cmp::min(length, content_size);
                (content_size - length, length)
            }
        };

        Self::build_response(StatusCode::PARTIAL_CONTENT, headers)
            .append_header(header::ContentRange(header::ContentRangeSpec::Bytes {
                range: Some((start, start + length - 1)),
                instance_length: Some(content_size),
            }))
            .body(Self::build_stream(reader, length))
    }
}

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
pub(crate) async fn middleware<H: DlnaRequestHandler>(
    req: ServiceRequest,
    next: Next<BoxBody>,
) -> Result<ServiceResponse<BoxBody>, Error> {
    let http_request = req.request();

    let app_data = Data::<HttpAppData<H>>::extract(http_request).await?;

    static REQUEST_ID: AtomicU64 = AtomicU64::new(0);
    let request_id = REQUEST_ID.fetch_add(1, Ordering::SeqCst);

    let request_data = RequestAppData {
        request_id,
        app_data: app_data.into_inner(),
    };

    let span = span!(
        Level::INFO,
        "HTTP request",
        "client.address" = field::Empty,
        "url.path" = req.path(),
        "request_id" = request_data.request_id,
        "user_agent.original" = field::Empty,
        "http.request.method" = %req.method(),
        "http.request.content_type" = field::Empty,
        "http.request.content_length" = field::Empty,
        "http.request.range" = field::Empty,
        "http.response.status_code" = field::Empty,
        "http.response.content_length" = field::Empty,
        "http.response.content_type" = field::Empty,
        "http.response.content_range" = field::Empty,
    );

    req.extensions_mut().insert(request_data);

    let headers = http_request.headers();

    record_header(&span, headers, header::USER_AGENT, "user_agent.original");
    record_header(
        &span,
        headers,
        header::CONTENT_TYPE,
        "http.request.content_type",
    );
    record_header(
        &span,
        headers,
        header::CONTENT_LENGTH,
        "http.request.content_length",
    );
    record_header(&span, headers, header::RANGE, "http.request.range");

    let client_addr = client_addr(req.request());

    if let Some(ref ip) = client_addr {
        span.record("client.address", ip);
    }

    let mut res = next.call(req).instrument(span.clone()).await?;

    let status = res.status();
    span.record("http.response.status_code", status.as_u16());
    res.response_mut()
        .headers_mut()
        .insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));

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
    record_header(
        &span,
        headers,
        header::CONTENT_RANGE,
        "http.response.content_range",
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

pub(crate) struct HttpAppData<H: DlnaRequestHandler> {
    pub(crate) uuid: Uuid,
    pub(crate) server_name: String,
    pub(crate) handler: H,
    pub(crate) icons: Vec<upnp::Icon>,
}

pub(crate) struct RequestAppData<H: DlnaRequestHandler> {
    pub(crate) request_id: u64,
    pub(crate) app_data: Arc<HttpAppData<H>>,
}

impl<H: DlnaRequestHandler> Clone for RequestAppData<H> {
    fn clone(&self) -> Self {
        Self {
            request_id: self.request_id,
            app_data: self.app_data.clone(),
        }
    }
}

pub(crate) async fn device_root<H: DlnaRequestHandler>(
    app_data: Data<HttpAppData<H>>,
) -> Xml<upnp::Root> {
    Xml::new(upnp::Root {
        uuid: app_data.uuid,
        server_name: app_data.server_name.clone(),
        icons: app_data.icons.clone(),
    })
}

#[get("/service/ConnectionManager.xml")]
async fn connection_manager() -> Xml<upnp::ServiceDescription> {
    Xml::new(upnp::ServiceDescription::new(vec![
        GetProtocolInfo::descriptor(),
        GetCurrentConnectionIDs::descriptor(),
        GetCurrentConnectionInfo::descriptor(),
    ]))
}

#[get("/service/ContentDirectory.xml")]
async fn content_directory() -> Xml<upnp::ServiceDescription> {
    Xml::new(upnp::ServiceDescription::new(vec![
        Browse::descriptor(),
        GetSortCapabilities::descriptor(),
        GetSearchCapabilities::descriptor(),
        GetSystemUpdateID::descriptor(),
        Search::descriptor(),
    ]))
}

pub(crate) async fn icon<H: DlnaRequestHandler>(
    app_data: Data<HttpAppData<H>>,
    id: Path<String>,
) -> HttpResponse {
    match app_data.handler.stream_icon(&id).await {
        Ok(stream_result) => {
            let mut builder = HttpResponse::Ok();

            builder
                .append_header(header::ContentType(stream_result.mime_type))
                .append_header(header::CacheControl(vec![CacheDirective::MaxAge(
                    CACHE_AGE,
                )]));

            if let Some(length) = stream_result.resource_size {
                builder.append_header(header::ContentLength(length as usize));
                builder.body(SizedStream::new(
                    length,
                    ReaderStream::with_capacity(stream_result.reader, BUFFER_CAPACITY),
                ))
            } else {
                builder.streaming(ReaderStream::with_capacity(
                    stream_result.reader,
                    BUFFER_CAPACITY,
                ))
            }
        }
        Err(err) => {
            let status = err.status_code();
            if status.is_client_error() {
                HttpResponse::NotFound().finish()
            } else {
                HttpResponse::BadRequest().finish()
            }
        }
    }
}

pub(crate) async fn resource_head<H: DlnaRequestHandler>(
    app_data: Data<HttpAppData<H>>,
    id: Path<String>,
) -> HttpResponse {
    match app_data.handler.get_resource(&id).await {
        Ok(resource) => {
            let mut builder = HttpResponse::Ok();
            builder.append_header(header::ContentType(resource.mime_type));

            if let Some(size) = resource.size {
                builder.append_header(header::ContentLength(size as usize));
            }

            builder.finish()
        }
        Err(err) => {
            let status = err.status_code();
            if status.is_client_error() {
                HttpResponse::NotFound().finish()
            } else {
                HttpResponse::BadRequest().finish()
            }
        }
    }
}

#[instrument(skip_all, fields(path = id.as_str()))]
pub(crate) async fn resource_get<H: DlnaRequestHandler>(
    req_data: ReqData<RequestAppData<H>>,
    req: HttpRequest,
    id: Path<String>,
) -> HttpResponse {
    let resource = match req_data.app_data.handler.get_resource(&id).await {
        Ok(r) => r,
        Err(err) => {
            let status = err.status_code();
            if status.is_client_error() {
                return HttpResponse::NotFound().finish();
            } else {
                return HttpResponse::BadRequest().finish();
            }
        }
    };

    match req_data.app_data.handler.stream_resource(&id).await {
        Ok(reader) => {
            if let Some(size) = resource.size {
                trace!(size, "Streaming resource with known size");
                let mut headers = HeaderMap::new();
                headers.append(
                    header::CONTENT_TYPE,
                    header::ContentType(resource.mime_type)
                        .try_into_value()
                        .unwrap(),
                );
                headers.append(
                    header::CACHE_CONTROL,
                    header::CacheControl(vec![CacheDirective::MaxAge(CACHE_AGE)])
                        .try_into_value()
                        .unwrap(),
                );
                ByteRangeResponse::build(&req, size, reader, headers).await
            } else {
                trace!("Streaming resource with unknown size");
                HttpResponse::Ok()
                    .append_header(header::ContentType(resource.mime_type))
                    .append_header(header::CacheControl(vec![CacheDirective::MaxAge(
                        CACHE_AGE,
                    )]))
                    .streaming(TelemetryStream::new(reader))
            }
        }
        Err(err) => {
            let status = err.status_code();
            if status.is_client_error() {
                HttpResponse::NotFound().finish()
            } else {
                HttpResponse::BadRequest().finish()
            }
        }
    }
}

#[derive(Debug)]
#[expect(unused)]
enum Sort {
    Ascending(String),
    Descending(String),
}

impl FromStr for Sort {
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some(stripped) = s.strip_prefix('+') {
            Ok(Sort::Ascending(stripped.to_owned()))
        } else if let Some(stripped) = s.strip_prefix('-') {
            Ok(Sort::Descending(stripped.to_owned()))
        } else {
            Ok(Sort::Ascending(s.to_owned()))
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct GetProtocolInfo {}

#[serde_as]
#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
struct GetProtocolInfoResponse {
    #[serde_as(as = "StringWithSeparator::<CommaSeparator, String>")]
    source: Vec<String>,
    #[serde_as(as = "StringWithSeparator::<CommaSeparator, String>")]
    sink: Vec<String>,
}

impl SoapAction for GetProtocolInfo {
    type Response = GetProtocolInfoResponse;

    fn schema() -> &'static str {
        ns::CONNECTION_MANAGER
    }

    fn name() -> &'static str {
        "GetProtocolInfo"
    }

    fn arguments() -> &'static [SoapArgument] {
        &[("Source", ArgDirection::Out), ("Sink", ArgDirection::Out)]
    }

    async fn execute<H: DlnaRequestHandler>(
        &self,
        _context: RequestContext<'_, H>,
    ) -> SoapResult<Self::Response> {
        Ok(GetProtocolInfoResponse {
            source: vec![
                "http-get:*:video/mp4:*".to_string(),
                "http-get:*:video/x-matroska:*".to_string(),
            ],
            sink: vec![],
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct GetCurrentConnectionIDs {}

#[serde_as]
#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
struct GetCurrentConnectionIDsResponse {
    #[serde(rename = "ConnectionIDs")]
    #[serde_as(as = "StringWithSeparator::<CommaSeparator, u32>")]
    connection_ids: Vec<u32>,
}

impl SoapAction for GetCurrentConnectionIDs {
    type Response = GetCurrentConnectionIDsResponse;

    fn schema() -> &'static str {
        ns::CONNECTION_MANAGER
    }

    fn name() -> &'static str {
        "GetCurrentConnectionIDs"
    }

    fn arguments() -> &'static [SoapArgument] {
        &[("ConnectionIDs", ArgDirection::Out)]
    }

    async fn execute<H: DlnaRequestHandler>(
        &self,
        _context: RequestContext<'_, H>,
    ) -> SoapResult<Self::Response> {
        Ok(GetCurrentConnectionIDsResponse {
            connection_ids: Vec::new(),
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct GetCurrentConnectionInfo {
    #[serde(rename = "ConnectionID")]
    _connection_id: u32,
}

#[derive(Debug, Serialize)]
#[expect(unused)]
enum ConnectionDirection {
    Output,
    Input,
}

#[derive(Debug, Serialize)]
#[expect(unused)]
enum ConnectionStatus {
    #[serde(rename = "OK")]
    Ok,
    ContentFormatMismatch,
    InsufficientBandwidth,
    UnreliableChannel,
    Unknown,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
struct GetCurrentConnectionInfoResponse {
    #[serde(rename = "RcsID")]
    rcs_id: u32,
    #[serde(rename = "AVTransportID")]
    av_transport_id: u32,
    protocol_info: String,
    peer_connection_manager: String,
    #[serde(rename = "PeerConnectionID")]
    peer_connection_id: u32,
    direction: ConnectionDirection,
    status: ConnectionStatus,
}

impl SoapAction for GetCurrentConnectionInfo {
    type Response = GetCurrentConnectionInfoResponse;

    fn schema() -> &'static str {
        ns::CONNECTION_MANAGER
    }

    fn name() -> &'static str {
        "GetCurrentConnectionInfo"
    }

    fn arguments() -> &'static [SoapArgument] {
        &[
            ("ConnectionID", ArgDirection::In),
            ("RcsID", ArgDirection::Out),
            ("AVTransportID", ArgDirection::Out),
            ("ProtocolInfo", ArgDirection::Out),
            ("PeerConnectionManager", ArgDirection::Out),
            ("PeerConnectionID", ArgDirection::Out),
            ("Direction", ArgDirection::Out),
            ("Status", ArgDirection::Out),
        ]
    }

    async fn execute<H: DlnaRequestHandler>(
        &self,
        _context: RequestContext<'_, H>,
    ) -> SoapResult<Self::Response> {
        Err(UpnpError::ActionFailed)
    }
}

#[derive(Debug, PartialEq, Deserialize)]
enum BrowseFlag {
    BrowseMetadata,
    BrowseDirectChildren,
}

#[serde_as]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Browse {
    #[serde(rename = "ObjectID")]
    object_id: String,
    browse_flag: BrowseFlag,
    #[serde_as(as = "StringWithSeparator::<CommaSeparator, String>")]
    #[expect(unused)]
    filter: Vec<String>,
    starting_index: usize,
    requested_count: usize,
    #[serde_as(as = "StringWithSeparator::<CommaSeparator, Sort>")]
    #[expect(unused)]
    sort_criteria: Vec<Sort>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
struct BrowseResponse {
    result: String,
    number_returned: usize,
    total_matches: usize,
    #[serde(rename = "UpdateID")]
    update_id: u32,
}

impl SoapAction for Browse {
    type Response = BrowseResponse;

    fn schema() -> &'static str {
        ns::CONTENT_DIRECTORY
    }

    fn name() -> &'static str {
        "Browse"
    }

    fn arguments() -> &'static [SoapArgument] {
        &[
            ("ObjectID", ArgDirection::In),
            ("BrowseFlag", ArgDirection::In),
            ("Filter", ArgDirection::In),
            ("StartingIndex", ArgDirection::In),
            ("RequestedCount", ArgDirection::In),
            ("SortCriteria", ArgDirection::In),
            ("Result", ArgDirection::Out),
            ("NumberReturned", ArgDirection::Out),
            ("TotalMatches", ArgDirection::Out),
            ("UpdateID", ArgDirection::Out),
        ]
    }

    async fn execute<H: DlnaRequestHandler>(
        &self,
        context: RequestContext<'_, H>,
    ) -> SoapResult<Self::Response> {
        let mut objects = if self.browse_flag == BrowseFlag::BrowseDirectChildren {
            context.handler.list_children(&self.object_id).await?
        } else {
            vec![context.handler.get_object(&self.object_id).await?]
        };

        let total_matches = objects.len();

        if self.starting_index > 0 {
            objects = objects.split_off(self.starting_index);
        }

        if self.requested_count < objects.len() {
            objects.truncate(self.requested_count);
        }

        let number_returned = objects.len();
        let result = upnp::DidlDocument::new(context.base.clone(), objects);

        Ok(BrowseResponse {
            number_returned,
            total_matches,
            update_id: 1,
            result: result.try_into()?,
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct GetSortCapabilities {}

#[serde_as]
#[derive(Debug, Serialize, Default)]
#[serde(rename_all = "PascalCase")]
struct GetSortCapabilitiesResponse {
    #[serde_as(as = "StringWithSeparator::<CommaSeparator, String>")]
    sort_caps: Vec<String>,
}

impl SoapAction for GetSortCapabilities {
    type Response = GetSortCapabilitiesResponse;

    fn schema() -> &'static str {
        ns::CONTENT_DIRECTORY
    }

    fn name() -> &'static str {
        "GetSortCapabilities"
    }

    fn arguments() -> &'static [SoapArgument] {
        &[("SortCaps", ArgDirection::Out)]
    }

    async fn execute<H: DlnaRequestHandler>(
        &self,
        _context: RequestContext<'_, H>,
    ) -> SoapResult<Self::Response> {
        Ok(Default::default())
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct GetSearchCapabilities {}

#[serde_as]
#[derive(Debug, Serialize, Default)]
#[serde(rename_all = "PascalCase")]
struct GetSearchCapabilitiesResponse {
    #[serde_as(as = "StringWithSeparator::<CommaSeparator, String>")]
    search_caps: Vec<String>,
}

impl SoapAction for GetSearchCapabilities {
    type Response = GetSearchCapabilitiesResponse;

    fn schema() -> &'static str {
        ns::CONTENT_DIRECTORY
    }

    fn name() -> &'static str {
        "GetSearchCapabilities"
    }

    fn arguments() -> &'static [SoapArgument] {
        &[("SearchCaps", ArgDirection::Out)]
    }

    async fn execute<H: DlnaRequestHandler>(
        &self,
        _context: RequestContext<'_, H>,
    ) -> SoapResult<Self::Response> {
        Ok(Default::default())
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct GetSystemUpdateID {}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
struct GetSystemUpdateIDResponse {
    id: u32,
}

impl SoapAction for GetSystemUpdateID {
    type Response = GetSystemUpdateIDResponse;

    fn schema() -> &'static str {
        ns::CONTENT_DIRECTORY
    }

    fn name() -> &'static str {
        "GetSystemUpdateID"
    }

    fn arguments() -> &'static [SoapArgument] {
        &[("Id", ArgDirection::Out)]
    }

    async fn execute<H: DlnaRequestHandler>(
        &self,
        _context: RequestContext<'_, H>,
    ) -> SoapResult<Self::Response> {
        Ok(GetSystemUpdateIDResponse { id: 1 })
    }
}

#[serde_as]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
#[expect(unused)]
struct Search {
    #[serde(rename = "ContainerID")]
    container_id: String,
    search_criteria: String,
    #[serde_as(as = "StringWithSeparator::<CommaSeparator, String>")]
    filter: Vec<String>,
    starting_index: u32,
    requested_count: u32,
    #[serde_as(as = "StringWithSeparator::<CommaSeparator, Sort>")]
    sort_criteria: Vec<Sort>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
struct SearchResponse {
    result: String,
    number_returned: u32,
    total_matches: u32,
    #[serde(rename = "UpdateID")]
    update_id: u32,
}

impl SoapAction for Search {
    type Response = SearchResponse;

    fn schema() -> &'static str {
        ns::CONTENT_DIRECTORY
    }

    fn name() -> &'static str {
        "Search"
    }

    fn arguments() -> &'static [SoapArgument] {
        &[
            ("ContainerID", ArgDirection::In),
            ("SearchCriteria", ArgDirection::In),
            ("Filter", ArgDirection::In),
            ("StartingIndex", ArgDirection::In),
            ("RequestedCount", ArgDirection::In),
            ("SortCriteria", ArgDirection::In),
            ("Result", ArgDirection::Out),
            ("NumberReturned", ArgDirection::Out),
            ("TotalMatches", ArgDirection::Out),
            ("UpdateID", ArgDirection::Out),
        ]
    }

    async fn execute<H: DlnaRequestHandler>(
        &self,
        _context: RequestContext<'_, H>,
    ) -> SoapResult<Self::Response> {
        Err(UpnpError::ActionFailed)
    }
}

pub(crate) async fn soap_request<H: DlnaRequestHandler>(
    request: HttpRequest,
    payload: Payload,
    app_data: Data<HttpAppData<H>>,
) -> HttpResponse {
    let headers = request.headers();

    let Some(mime) = headers
        .get(header::CONTENT_TYPE)
        .and_then(|hv| hv.to_str().ok())
        .and_then(|st| Mime::from_str(st).ok())
    else {
        return HttpResponse::BadRequest().finish();
    };

    if mime.subtype() != mime::XML
        || (mime.type_() != mime::APPLICATION && mime.type_() != mime::TEXT)
    {
        return HttpResponse::BadRequest().finish();
    }

    let Some(soap_action) = headers
        .get("SOAPAction")
        .and_then(|hv| hv.to_str().ok())
        .map(|st| st.trim_matches('"'))
    else {
        return HttpResponse::BadRequest().finish();
    };

    if soap_action == GetProtocolInfo::soap_action() {
        return GetProtocolInfo::service(request, payload, app_data).await;
    }

    if soap_action == GetCurrentConnectionIDs::soap_action() {
        return GetCurrentConnectionIDs::service(request, payload, app_data).await;
    }

    if soap_action == GetCurrentConnectionInfo::soap_action() {
        return GetCurrentConnectionInfo::service(request, payload, app_data).await;
    }

    if soap_action == Browse::soap_action() {
        return Browse::service(request, payload, app_data).await;
    }

    if soap_action == GetSortCapabilities::soap_action() {
        return GetSortCapabilities::service(request, payload, app_data).await;
    }

    if soap_action == GetSearchCapabilities::soap_action() {
        return GetSearchCapabilities::service(request, payload, app_data).await;
    }

    if soap_action == GetSystemUpdateID::soap_action() {
        return GetSystemUpdateID::service(request, payload, app_data).await;
    }

    if soap_action == Search::soap_action() {
        return Search::service(request, payload, app_data).await;
    }

    UpnpError::InvalidAction.respond_to(&request)
}

pub struct DlnaServiceFactory<H: DlnaRequestHandler> {
    app_data: Data<HttpAppData<H>>,
}

impl<H: DlnaRequestHandler> DlnaServiceFactory<H> {
    pub(crate) fn new(app_data: HttpAppData<H>) -> Self {
        Self {
            app_data: Data::new(app_data),
        }
    }
}

impl<H: DlnaRequestHandler> Clone for DlnaServiceFactory<H> {
    fn clone(&self) -> Self {
        Self {
            app_data: self.app_data.clone(),
        }
    }
}

impl<H: DlnaRequestHandler> HttpServiceFactory for DlnaServiceFactory<H> {
    fn register(self, config: &mut AppService) {
        let scope = Scope::new("/upnp")
            .app_data(self.app_data.clone())
            .wrap(from_fn(middleware::<H>))
            .route("/device.xml", web::get().to(device_root::<H>))
            .service(connection_manager)
            .service(content_directory)
            .route("/soap", web::post().to(soap_request::<H>))
            .route("/icon/{path:.*}", web::get().to(icon::<H>))
            .route("/resource/{path:.*}", web::head().to(resource_head::<H>))
            .route("/resource/{path:.*}", web::get().to(resource_get::<H>));

        scope.register(config);
    }
}
