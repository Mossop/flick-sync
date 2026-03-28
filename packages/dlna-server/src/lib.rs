#![deny(unreachable_pub)]
//! A basic implementation of a DLNA media server

use std::net::Ipv4Addr;

use actix_web::{App, HttpServer, dev::ServerHandle};
use async_trait::async_trait;
use mime::Mime;
pub use services::DlnaServiceFactory;
use tokio::io::{AsyncRead, AsyncSeek};
pub use upnp::{Container, Icon, Item, Object, Resource, UpnpError};
use uuid::Uuid;

use crate::{
    rt::{TaskHandle, spawn},
    services::HttpAppData,
    ssdp::Ssdp,
};

/// The default port to use for HTTP communication
const DEFAULT_HTTP_PORT: u16 = 1980;

/// A custom service to advertise in SSDP announcements.
#[derive(Clone)]
pub struct CustomService {
    pub service_type: String,
    /// The location of this service. May be an absolute URL or a path relative to the DLNA
    /// server (e.g. `/state.json`). Used as the LOCATION in SSDP announcements.
    pub location: String,
}

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
pub struct StreamResponse<R> {
    /// The content type of the data.
    pub mime_type: Mime,
    /// If known the full size of the resource.
    pub resource_size: Option<u64>,
    /// The resource stream.
    pub reader: R,
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
    ) -> Result<StreamResponse<impl AsyncRead + 'static>, UpnpError>;

    /// Gets the metadata for a resource.
    async fn get_resource(&self, resource_id: &str) -> Result<Resource, UpnpError>;

    /// Requests a stream for a resource.
    async fn stream_resource(
        &self,
        resource_id: &str,
    ) -> Result<impl AsyncRead + AsyncSeek + Unpin + 'static, UpnpError>;
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
            manufacturer: None,
            manufacturer_url: None,
            server_version,
            http_port: DEFAULT_HTTP_PORT,
            icons: Vec::new(),
            handler,
            custom_services: Vec::new(),
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
    manufacturer: Option<String>,
    manufacturer_url: Option<String>,
    http_port: u16,
    icons: Vec<Icon>,
    handler: H,
    custom_services: Vec<CustomService>,
}

impl<H: DlnaRequestHandler> DlnaServerBuilder<H> {
    /// Builds the DLNA server and starts the SSDP listener. Returns the server and a service
    /// factory that must be added to an `actix_web` server instance. Note that if your server
    /// uses a http port other than 1980 you must configure it on this builder first or Upnp
    /// discovery will fail.
    pub async fn build_service(self) -> anyhow::Result<(DlnaServer, DlnaServiceFactory<H>)> {
        let additional_types: Vec<(String, String)> = self
            .custom_services
            .iter()
            .map(|s| (s.service_type.clone(), s.location.clone()))
            .collect();

        let service_factory = DlnaServiceFactory::new(HttpAppData {
            uuid: self.uuid,
            server_name: self.server_name,
            manufacturer: self.manufacturer,
            manufacturer_url: self.manufacturer_url,
            handler: self.handler,
            icons: self.icons,
        });

        Ok((
            DlnaServer {
                ssdp_handle: Ssdp::new(
                    self.uuid,
                    &self.server_version,
                    self.http_port,
                    additional_types,
                ),
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
    pub fn server_version<S: ToString>(mut self, version: S) -> Self {
        self.server_version = version.to_string();
        self
    }

    /// Sets a base name for the server.
    pub fn server_name<S: ToString>(mut self, name: S) -> Self {
        self.server_name = name.to_string();
        self
    }

    /// Sets a manufacturer for the server.
    pub fn manufacturer<S: ToString>(mut self, manufacturer: S) -> Self {
        self.manufacturer = Some(manufacturer.to_string());
        self
    }

    /// Sets a manufacturer URL for the server.
    pub fn manufacturer_url<S: ToString>(mut self, manufacturer_url: S) -> Self {
        self.manufacturer_url = Some(manufacturer_url.to_string());
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

    /// Adds a custom service to advertise in SSDP announcements and the device description.
    pub fn custom_service(mut self, svc: CustomService) -> Self {
        self.custom_services.push(svc);
        self
    }
}
