use std::{convert::Infallible, net::SocketAddr, str::FromStr};

use actix_web::{
    Error, HttpRequest,
    body::MessageBody,
    dev::{HttpServiceFactory, ServiceRequest, ServiceResponse},
    get,
    middleware::Next,
    post,
    web::Data,
};
use serde::{Deserialize, Serialize};
use serde_with::{StringWithSeparator, formats::CommaSeparator, serde_as};
use tracing::{Instrument, Level, field, instrument, span, trace, warn};
use uuid::Uuid;

use crate::{
    DlnaRequestHandler, UpnpError, ns,
    soap::{ArgDirection, RequestContext, ResponseWrapper, SoapAction, SoapArgument, SoapResult},
    upnp::{self, DidlDocument},
    xml::Xml,
};

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
        trace!(parent: &span, status = status.as_u16())
    }

    Ok(res)
}

pub(crate) struct HttpAppData {
    pub(crate) uuid: Uuid,
    pub(crate) handler: Box<dyn DlnaRequestHandler>,
}

#[get("/device.xml")]
async fn device_root(app_data: Data<HttpAppData>) -> Xml<upnp::Root> {
    Xml::new(upnp::Root {
        uuid: app_data.uuid,
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

#[derive(Debug)]
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

    async fn execute<H: DlnaRequestHandler + ?Sized>(
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

    async fn execute<H: DlnaRequestHandler + ?Sized>(
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
enum ConnectionDirection {
    Output,
    Input,
}

#[derive(Debug, Serialize)]
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

    async fn execute<'a, H: DlnaRequestHandler + ?Sized>(
        &self,
        _context: RequestContext<'a, H>,
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
    filter: Vec<String>,
    starting_index: usize,
    requested_count: usize,
    #[serde_as(as = "StringWithSeparator::<CommaSeparator, Sort>")]
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

    async fn execute<H: DlnaRequestHandler + ?Sized>(
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
        let result = DidlDocument::new(context.base.clone(), objects);

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

    async fn execute<H: DlnaRequestHandler + ?Sized>(
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

    async fn execute<H: DlnaRequestHandler + ?Sized>(
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

    async fn execute<H: DlnaRequestHandler + ?Sized>(
        &self,
        _context: RequestContext<'_, H>,
    ) -> SoapResult<Self::Response> {
        Ok(GetSystemUpdateIDResponse { id: 1 })
    }
}

#[serde_as]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
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

    async fn execute<H: DlnaRequestHandler + ?Sized>(
        &self,
        _context: RequestContext<'_, H>,
    ) -> SoapResult<Self::Response> {
        Err(UpnpError::ActionFailed)
    }
}

#[post("")]
async fn unknown_action() -> ResponseWrapper<()> {
    ResponseWrapper::error(UpnpError::InvalidAction)
}

pub(super) fn soap_services() -> impl HttpServiceFactory {
    (
        GetProtocolInfo::factory(),
        GetCurrentConnectionIDs::factory(),
        GetCurrentConnectionInfo::factory(),
        Browse::factory(),
        GetSortCapabilities::factory(),
        GetSearchCapabilities::factory(),
        GetSystemUpdateID::factory(),
        Search::factory(),
        unknown_action,
    )
}
