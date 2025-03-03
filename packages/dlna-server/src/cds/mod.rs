use actix_web::{dev::HttpServiceFactory, get, web::Data};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    DlnaRequestHandler,
    cds::{
        soap::{ArgDirection, SoapArgument, SoapResult},
        xml::Xml,
    },
};
pub(super) use soap::SoapAction;

pub(super) mod middleware;
mod soap;
mod upnp;
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(super) struct GetProtocolInfo {}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(super) struct GetProtocolInfoResponse {
    source: String,
    sink: String,
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

    async fn execute(&self) -> SoapResult<Self::Response> {
        Ok(GetProtocolInfoResponse {
            source: "http-get:*:video/mp4:*".to_string(),
            sink: "".to_string(),
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(super) struct GetCurrentConnectionIDs {}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(super) struct GetCurrentConnectionIDsResponse {
    #[serde(rename = "ConnectionIDs")]
    connection_ids: String,
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

    async fn execute(&self) -> SoapResult<Self::Response> {
        Ok(GetCurrentConnectionIDsResponse {
            connection_ids: String::new(),
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(super) struct GetCurrentConnectionInfo {
    #[serde(rename = "ConnectionID")]
    _connection_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(super) struct GetCurrentConnectionInfoResponse {}

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

    async fn execute(&self) -> SoapResult<Self::Response> {
        todo!()
    }
}

#[derive(Debug, Deserialize)]
pub(super) enum BrowseFlag {
    BrowseMetadata,
    BrowseDirectChildren,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(super) struct Browse {
    #[serde(rename = "ObjectID")]
    object_id: String,
    browse_flag: BrowseFlag,
    filter: String,
    starting_index: u32,
    requested_count: u32,
    sort_criteria: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(super) struct BrowseResponse {
    result: String,
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

    async fn execute(&self) -> SoapResult<Self::Response> {
        todo!()
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(super) struct GetSortCapabilities {}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(super) struct GetSortCapabilitiesResponse {
    sort_caps: String,
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

    async fn execute(&self) -> SoapResult<Self::Response> {
        todo!()
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(super) struct GetSearchCapabilities {}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(super) struct GetSearchCapabilitiesResponse {
    search_caps: String,
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

    async fn execute(&self) -> SoapResult<Self::Response> {
        todo!()
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(super) struct GetSystemUpdateID {}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(super) struct GetSystemUpdateIDResponse {
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

    async fn execute(&self) -> SoapResult<Self::Response> {
        Ok(GetSystemUpdateIDResponse { id: 1 })
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(super) struct Search {
    #[serde(rename = "ContainerID")]
    container_id: String,
    search_criteria: String,
    filter: String,
    starting_index: u32,
    requested_count: u32,
    sort_criteria: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(super) struct SearchResponse {
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

    async fn execute(&self) -> SoapResult<Self::Response> {
        todo!()
    }
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
    )
}
