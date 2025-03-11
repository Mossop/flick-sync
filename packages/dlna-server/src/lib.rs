#![deny(unreachable_pub)]
//! A basic implementation of a DLNA media server

use std::{io, net::Ipv4Addr};

use actix_web::{
    App, HttpServer,
    dev::ServerHandle,
    middleware::from_fn,
    web::{self, Data},
};
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

/// The default port to use for HTTP communication
const HTTP_PORT: u16 = 1980;

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
    web_handle: ServerHandle,
}

impl DlnaServer {
    /// Builds a default DLNA server listing on all interfaces on IPv4 and
    /// starts it listening using the chosen runtime.
    pub async fn new<H: DlnaRequestHandler>(handler: H) -> anyhow::Result<Self> {
        Self::builder(handler).build().await
    }

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
            icons: Vec::new(),
            handler,
        }
    }

    pub async fn shutdown(self) {
        self.ssdp_handle.shutdown().await;
        self.web_handle.stop(true).await;
    }
}

/// A builder allowing configuration of the DLNA server.
pub struct DlnaServerBuilder<H: DlnaRequestHandler> {
    uuid: Uuid,
    server_version: String,
    server_name: String,
    icons: Vec<Icon>,
    handler: H,
}

impl<H: DlnaRequestHandler> DlnaServerBuilder<H> {
    /// Builds the DLNA server and starts it listening using the chosen runtime.
    pub async fn build(self) -> anyhow::Result<DlnaServer> {
        let app_data = Data::new(HttpAppData {
            uuid: self.uuid,
            server_name: self.server_name,
            handler: self.handler,
            icons: self.icons,
        });

        let http_server = HttpServer::new(move || {
            App::new()
                .app_data(app_data.clone())
                .wrap(from_fn(services::middleware::<H>))
                .route("/device.xml", web::get().to(services::device_root::<H>))
                .service(services::connection_manager)
                .service(services::content_directory)
                .route("/soap", web::post().to(services::soap_request::<H>))
                .route("/icon/{path:.*}", web::get().to(services::icon::<H>))
                .route(
                    "/resource/{path:.*}",
                    web::head().to(services::resource_head::<H>),
                )
                .route(
                    "/resource/{path:.*}",
                    web::get().to(services::resource_get::<H>),
                )
        })
        .bind((Ipv4Addr::UNSPECIFIED, HTTP_PORT))?;

        let server = http_server.run();
        let web_handle = server.handle();

        spawn(server);

        Ok(DlnaServer {
            ssdp_handle: Ssdp::new(self.uuid, &self.server_version, HTTP_PORT),
            web_handle,
        })
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
