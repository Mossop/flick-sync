use std::{convert::Infallible, str::FromStr};

use actix_web::{dev::HttpServiceFactory, get, post, web::Data};
use serde::{Deserialize, Serialize};
use serde_with::StringWithSeparator;
use serde_with::formats::CommaSeparator;
use serde_with::serde_as;
use uuid::Uuid;

use crate::{
    DlnaRequestHandler,
    cds::{
        soap::{ArgDirection, SoapArgument, SoapResult, UpnpError},
        upnp::BrowseResult,
        xml::Xml,
    },
};
pub(super) use soap::SoapAction;

pub(super) mod middleware;
mod soap;
pub(crate) mod upnp;
mod xml;

const SCHEMA_CONNECTION_MANAGER: &str = "urn:schemas-upnp-org:service:ConnectionManager:1";
const SCHEMA_CONTENT_DIRECTORY: &str = "urn:schemas-upnp-org:service:ContentDirectory:1";

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
        SCHEMA_CONNECTION_MANAGER
    }

    fn name() -> &'static str {
        "GetProtocolInfo"
    }

    fn arguments() -> &'static [SoapArgument] {
        &[("Source", ArgDirection::Out), ("Sink", ArgDirection::Out)]
    }

    async fn execute<H: DlnaRequestHandler + ?Sized>(
        &self,
        _handler: &H,
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
        SCHEMA_CONNECTION_MANAGER
    }

    fn name() -> &'static str {
        "GetCurrentConnectionIDs"
    }

    fn arguments() -> &'static [SoapArgument] {
        &[("ConnectionIDs", ArgDirection::Out)]
    }

    async fn execute<H: DlnaRequestHandler + ?Sized>(
        &self,
        _handler: &H,
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
        SCHEMA_CONNECTION_MANAGER
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

    async fn execute<H: DlnaRequestHandler + ?Sized>(
        &self,
        _handler: &H,
    ) -> SoapResult<Self::Response> {
        Err(UpnpError::ActionFailed)
    }
}

#[derive(Debug, Deserialize)]
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
    starting_index: u32,
    requested_count: u32,
    #[serde_as(as = "StringWithSeparator::<CommaSeparator, Sort>")]
    sort_criteria: Vec<Sort>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
struct BrowseResponse {
    result: Xml<BrowseResult>,
    number_returned: u32,
    total_matches: u32,
    #[serde(rename = "UpdateID")]
    update_id: u32,
}

impl SoapAction for Browse {
    type Response = BrowseResponse;

    fn schema() -> &'static str {
        SCHEMA_CONTENT_DIRECTORY
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
        handler: &H,
    ) -> SoapResult<Self::Response> {
        let objects = handler.list_children(&self.object_id).await;

        Ok(BrowseResponse {
            number_returned: objects.len() as u32,
            total_matches: objects.len() as u32,
            update_id: 1,
            result: Xml::new(objects.into()),
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
        SCHEMA_CONTENT_DIRECTORY
    }

    fn name() -> &'static str {
        "GetSortCapabilities"
    }

    fn arguments() -> &'static [SoapArgument] {
        &[("SortCaps", ArgDirection::Out)]
    }

    async fn execute<H: DlnaRequestHandler + ?Sized>(
        &self,
        _handler: &H,
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
        SCHEMA_CONTENT_DIRECTORY
    }

    fn name() -> &'static str {
        "GetSearchCapabilities"
    }

    fn arguments() -> &'static [SoapArgument] {
        &[("SearchCaps", ArgDirection::Out)]
    }

    async fn execute<H: DlnaRequestHandler + ?Sized>(
        &self,
        _handler: &H,
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
        SCHEMA_CONTENT_DIRECTORY
    }

    fn name() -> &'static str {
        "GetSystemUpdateID"
    }

    fn arguments() -> &'static [SoapArgument] {
        &[("Id", ArgDirection::Out)]
    }

    async fn execute<H: DlnaRequestHandler + ?Sized>(
        &self,
        _handler: &H,
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
        SCHEMA_CONTENT_DIRECTORY
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
        _handler: &H,
    ) -> SoapResult<Self::Response> {
        Err(UpnpError::ActionFailed)
    }
}

#[post("")]
async fn unknown_action() -> UpnpError {
    UpnpError::InvalidAction
}

pub(super) fn services() -> impl HttpServiceFactory {
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
