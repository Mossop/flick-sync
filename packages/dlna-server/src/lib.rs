#![deny(unreachable_pub)]
//! A basic implementation of a DLNA media server

use std::{io, net::Ipv4Addr};

use actix_web::{App, HttpServer, dev::ServerHandle};
use async_trait::async_trait;
use bytes::Bytes;
use futures::Stream;
use mime::Mime;
pub use upnp::{Container, Icon, Item, Object, Resource, UpnpError};
use uuid::Uuid;

use crate::{
    rt::{TaskHandle, spawn},
    services::HttpAppData,
    ssdp::Ssdp,
};
pub use services::DlnaServiceFactory;

/// The default port to use for HTTP communication
const DEFAULT_HTTP_PORT: u16 = 1980;

#[cfg_attr(feature = "rt-async", path = "rt/async_std.rs")]
#[cfg_attr(feature = "rt-tokio", path = "rt/tokio.rs")]
mod rt;
mod services;
mod soap;
mod ssdp;
mod upnp;
mod xml;

#[cfg(not(any(feature = "rt-tokio", feature = "rt-async")))]
compile_error!("An async runtime must be selected");

mod ns {
    pub(crate) const CONNECTION_MANAGER: &str = "urn:schemas-upnp-org:service:ConnectionManager:1";
    pub(crate) const CONTENT_DIRECTORY: &str = "urn:schemas-upnp-org:service:ContentDirectory:1";
    pub(crate) const SOAP_ENVELOPE: &str = "http://schemas.xmlsoap.org/soap/envelope/";
    pub(crate) const SOAP_ENCODING: &str = "http://schemas.xmlsoap.org/soap/encoding/";
    pub(crate) const UPNP_ROOT: &str = "upnp:rootdevice";
    pub(crate) const UPNP_MEDIASERVER: &str = "urn:schemas-upnp-org:device:MediaServer:1";
    pub(crate) const UPNP_CONTENTDIRECTORY: &str =
        "urn:schemas-upnp-org:service:ContentDirectory:1";
    pub(crate) const UPNP_DEVICE: &str = "urn:schemas-upnp-org:device-1-0";
    pub(crate) const UPNP_SERVICE: &str = "urn:schemas-upnp-org:service-1-0";
    pub(crate) const DIDL: &str = "urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/";
    pub(crate) const DC: &str = "http://purl.org/dc/elements/1.1/";
    pub(crate) const UPNP: &str = "urn:schemas-upnp-org:metadata-1-0/upnp/";
    pub(crate) const DLNA: &str = "urn:schemas-dlna-org:metadata-1-0/";
}

/// The range included in the stream.
#[derive(Debug)]
pub struct Range {
    pub start: u64,
    pub length: u64,
}

/// A response to a request to stream some data.
pub struct StreamResponse<S> {
    /// The content type of the data.
    pub mime_type: Mime,
    /// If the content is not the full resource then this indicates the range included.
    pub range: Option<Range>,
    /// If known the full size of the resource.
    pub resource_size: Option<u64>,
    /// The resource stream.
    pub stream: S,
}

/// Some perhaps useful information about the DLNA client.
pub struct DlnaContext {
    /// A unique identifier for this request.
    pub request_id: u64,
}

/// This handler is called when the DLNA server needs to respond to client requests.
#[async_trait]
pub trait DlnaRequestHandler
where
    Self: Send + Sync + 'static,
{
    /// Get the metadata for the object with the given ID.
    async fn get_object(&self, object_id: &str) -> Result<Object, UpnpError>;
    /// Get the metadata for the objects that are direct children of the object with the given ID.
    async fn list_children(&self, parent_id: &str) -> Result<Vec<Object>, UpnpError>;

    /// Requests a stream for an icon.
    async fn stream_icon(
        &self,
        icon_id: &str,
    ) -> Result<StreamResponse<impl Stream<Item = Result<Bytes, io::Error>> + 'static>, UpnpError>;

    /// Gets the information for a resource, used for HEAD requests.
    async fn get_resource(&self, resource_id: &str) -> Result<Resource, UpnpError>;

    /// Requests a stream for a resource.
    async fn stream_resource(
        &self,
        resource_id: &str,
        seek: Option<u64>,
        length: Option<u64>,
        context: DlnaContext,
    ) -> Result<StreamResponse<impl Stream<Item = Result<Bytes, io::Error>> + 'static>, UpnpError>;
}

/// A handle to the DLNA server allowing for shutting the server down.
pub struct DlnaServer {
    ssdp_handle: Ssdp,
    web_handle: Option<ServerHandle>,
}

impl DlnaServer {
    /// Builds a default DLNA server listing on all interfaces on IPv4 and
    /// starts it listening using the chosen runtime.
    pub async fn new<H: DlnaRequestHandler>(handler: H) -> anyhow::Result<Self> {
        Self::builder(handler).build().await
    }

    /// Creates a default builder that can be further customized.
    pub fn builder<H: DlnaRequestHandler>(handler: H) -> DlnaServerBuilder<H> {
        let server_version = format!(
            "RustDlna/{}.{}",
            env!("CARGO_PKG_VERSION_MAJOR"),
            env!("CARGO_PKG_VERSION_MINOR")
        );

        DlnaServerBuilder {
            uuid: Uuid::new_v4(),
            server_name: "Dlna".to_string(),
            server_version,
            http_port: DEFAULT_HTTP_PORT,
            icons: Vec::new(),
            handler,
        }
    }

    /// Restarts the UPnP listener service. This is required if the set of available network
    /// interfaces changes.
    pub fn restart(&self) {
        self.ssdp_handle.restart();
    }

    /// Shuts down the server.
    pub async fn shutdown(self) {
        self.ssdp_handle.shutdown().await;
        if let Some(web_handle) = self.web_handle {
            web_handle.stop(true).await;
        }
    }
}

/// A builder allowing configuration of the DLNA server.
pub struct DlnaServerBuilder<H: DlnaRequestHandler> {
    uuid: Uuid,
    server_version: String,
    server_name: String,
    http_port: u16,
    icons: Vec<Icon>,
    handler: H,
}

impl<H: DlnaRequestHandler> DlnaServerBuilder<H> {
    /// Builds the DLNA server and starts the SSDP listener. Returns the server and a service
    /// factory that must be added to an `actix_web` server instance. Note that if your server
    /// uses a http port other than 1980 you must configure it on this builder first or Upnp
    /// discovery will fail.
    pub async fn build_service(self) -> anyhow::Result<(DlnaServer, DlnaServiceFactory<H>)> {
        let service_factory = DlnaServiceFactory::new(HttpAppData {
            uuid: self.uuid,
            server_name: self.server_name,
            handler: self.handler,
            icons: self.icons,
        });

        Ok((
            DlnaServer {
                ssdp_handle: Ssdp::new(self.uuid, &self.server_version, self.http_port),
                web_handle: None,
            },
            service_factory,
        ))
    }

    /// Builds the DLNA server and starts a http server using the chosen runtime.
    pub async fn build(self) -> anyhow::Result<DlnaServer> {
        let http_port = self.http_port;
        let (mut dlna_server, web_scope) = self.build_service().await?;

        let http_server = HttpServer::new(move || App::new().service(web_scope.clone()))
            .bind((Ipv4Addr::UNSPECIFIED, http_port))?
            .run();

        dlna_server.web_handle = Some(http_server.handle());

        spawn(http_server);

        Ok(dlna_server)
    }

    /// Sets a specific server version. Should match the form `Name/<major>.<minor>`. Defaults to
    /// `RustDlna/<pkg major>.<pkg minor>`
    pub fn server_version(mut self, version: &str) -> Self {
        self.server_version = version.to_owned();
        self
    }

    /// Sets a base name for the server.
    pub fn server_name(mut self, name: &str) -> Self {
        self.server_name = name.to_owned();
        self
    }

    /// Sets a specific HTTP port. If not called the default of 1980 is used.
    pub fn http_port(mut self, port: u16) -> Self {
        self.http_port = port;
        self
    }

    /// Sets a specific UUID. If not called a unique ID is generated everytime a new server is
    /// created.
    pub fn uuid(mut self, uuid: Uuid) -> Self {
        self.uuid = uuid;
        self
    }

    /// Adds an icon to represent this server. Can be called multiple times for different
    /// resolutions and mimetypes.
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icons.push(icon);
        self
    }
}
