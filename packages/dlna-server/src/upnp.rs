use std::{io::Write, time::Duration};

use actix_web::http::StatusCode;
use mime::Mime;
use url::Url;
use uuid::Uuid;

use crate::{
    ns,
    soap::SoapArgument,
    xml::{ToXml, WriterError, XmlWriter},
};

#[derive(Debug)]
pub enum UpnpError {
    InvalidAction,
    InvalidArgs,
    ActionFailed,
    ArgumentInvalid,
}

impl UpnpError {
    pub(crate) fn status_code(&self) -> StatusCode {
        match self {
            UpnpError::ActionFailed => StatusCode::INTERNAL_SERVER_ERROR,
            _ => StatusCode::BAD_REQUEST,
        }
    }

    pub(crate) fn status(&self) -> u16 {
        match self {
            UpnpError::InvalidAction => 401,
            UpnpError::InvalidArgs => 402,
            UpnpError::ActionFailed => 501,
            UpnpError::ArgumentInvalid => 600,
        }
    }

    pub fn unknown_object() -> Self {
        Self::ArgumentInvalid
    }
}

impl From<WriterError> for UpnpError {
    fn from(_: WriterError) -> Self {
        Self::ActionFailed
    }
}

/// Represents a container on the server.
#[derive(Debug)]
pub struct Container {
    /// The unique identifier for the container. The format is up to the caller however the value
    /// `"0"` is used to represent the root container and the value `"-1"` represents its parent.
    pub id: String,
    /// The parent identifier of this container, see the notes for `id`. Must be `"-1"` if the value
    /// for `id` is `"0"`
    pub parent_id: String,
    /// Optionally provide the number of children of this container.
    pub child_count: Option<usize>,
    /// The title of this container.
    pub title: String,
}

impl<W: Write> ToXml<W> for Container {
    fn write_xml(&self, writer: &mut XmlWriter<W>) -> Result<(), WriterError> {
        let mut builder = writer.element_ns((ns::DIDL, "container"));
        if let Some(child_count) = self.child_count {
            builder = builder.attr("childCount", child_count);
        }
        builder
            .attr("id", &self.id)
            .attr("parentID", &self.parent_id)
            .attr("restricted", "1")
            .attr("searchable", "0")
            .contents(|writer| {
                writer.element_ns((ns::DC, "title")).text(&self.title)?;
                writer
                    .element_ns((ns::UPNP, "class"))
                    .text("object.container")
            })
    }
}

#[derive(Debug)]
pub struct Resource {
    pub id: String,
    pub mime_type: Mime,
    pub size: Option<u64>,
    pub seekable: bool,
    pub duration: Option<Duration>,
}

impl<W: Write> ToXml<W> for Resource {
    fn write_xml(&self, writer: &mut XmlWriter<W>) -> Result<(), WriterError> {
        let base = writer.base();
        let uri = base.join(&format!("/resource/{}", self.id)).unwrap();

        let mut builder = writer
            .element_ns((ns::DIDL, "res"))
            .attr("protocolInfo", format!("http-get:*:{}:*", self.mime_type));

        if let Some(size) = self.size {
            builder = builder.attr("size", size);
        }

        builder.text(uri)
    }
}

/// Represents a media item on the server.
#[derive(Debug)]
pub struct Item {
    /// The unique identifier for the container. The format is up to the caller however the value
    /// `"0"` is used to represent the root container and the value `"-1"` represents its parent.
    pub id: String,
    /// The parent identifier of this item, see the notes for `id`. Must be `"-1"` if the value
    /// for `id` is `"0"`
    pub parent_id: String,
    /// The title of this item.
    pub title: String,
    pub resources: Vec<Resource>,
}

impl<W: Write> ToXml<W> for Item {
    fn write_xml(&self, writer: &mut XmlWriter<W>) -> Result<(), WriterError> {
        writer
            .element_ns((ns::DIDL, "item"))
            .attr("id", &self.id)
            .attr("parentID", &self.parent_id)
            .attr("restricted", "1")
            .contents(|writer| {
                writer.element_ns((ns::DC, "title")).text(&self.title)?;
                writer
                    .element_ns((ns::UPNP, "class"))
                    .text("object.item.videoItem")?;

                for resource in &self.resources {
                    resource.write_xml(writer)?;
                }

                Ok(())
            })
    }
}

#[derive(Debug)]
pub enum Object {
    Item(Item),
    Container(Container),
}

impl<W: Write> ToXml<W> for Object {
    fn write_xml(&self, writer: &mut XmlWriter<W>) -> Result<(), WriterError> {
        match self {
            Self::Item(o) => o.write_xml(writer),
            Self::Container(o) => o.write_xml(writer),
        }
    }
}

#[derive(Debug)]
pub(crate) struct DidlDocument<T> {
    base: Url,
    objects: Vec<T>,
}

impl<T> DidlDocument<T> {
    pub(crate) fn new(base: Url, objects: Vec<T>) -> Self {
        Self { base, objects }
    }
}

impl<T> TryInto<String> for DidlDocument<T>
where
    T: for<'a> ToXml<&'a mut Vec<u8>>,
{
    type Error = WriterError;

    fn try_into(self) -> Result<String, Self::Error> {
        let mut sink = Vec::<u8>::new();

        XmlWriter::write_document(&self, &mut sink, Some(self.base.clone()))?;

        Ok(String::from_utf8(sink)?)
    }
}

impl<W, T> ToXml<W> for DidlDocument<T>
where
    W: Write,
    T: ToXml<W>,
{
    fn write_xml(&self, writer: &mut XmlWriter<W>) -> Result<(), WriterError> {
        writer
            .element_ns((ns::DIDL, "DIDL-Lite"))
            .prefix("dc", ns::DC)
            .prefix("dlna", ns::DLNA)
            .prefix("upnp", ns::UPNP)
            .contents(|writer| {
                for object in self.objects.iter() {
                    object.write_xml(writer)?;
                }

                Ok(())
            })
    }
}

pub(crate) struct Root {
    pub(crate) uuid: Uuid,
}

impl<W: Write> ToXml<W> for Root {
    fn write_xml(&self, writer: &mut XmlWriter<W>) -> Result<(), WriterError> {
        writer
            .element_ns((ns::UPNP_DEVICE, "root"))
            .contents(|writer| {
                writer.element("specVersion").contents(|writer| {
                    writer.element("major").text(1)?;
                    writer.element("minor").text(1)
                })?;

                writer.element("device").contents(|writer| {
                    writer
                        .element("UDN")
                        .text(format!("uuid:{}", self.uuid.as_hyphenated()))?;
                    writer.element("friendlyName").text("Synced Flicks")?;
                    writer
                        .element("deviceType")
                        .text("urn:schemas-upnp-org:device:MediaServer:1")?;
                    writer.element("manufacturer").text("Dave Townsend")?;
                    writer
                        .element("manufacturerURL")
                        .text("https://github.com/Mossop/flick-sync")?;
                    writer.element("modelName").text("Synced Flicks")?;
                    writer
                        .element("modelDescription")
                        .text("Synced Flicks Media Server")?;

                    writer.element("serviceList").contents(|writer| {
                        writer.element("service").contents(|writer| {
                            writer.element("serviceType").text(ns::CONNECTION_MANAGER)?;
                            writer
                                .element("serviceId")
                                .text("urn:upnp-org:serviceId:ConnectionManager")?;
                            writer
                                .element("SCPDURL")
                                .text("/service/ConnectionManager.xml")?;
                            writer.element("controlURL").text("/soap")
                        })?;

                        writer.element("service").contents(|writer| {
                            writer.element("serviceType").text(ns::CONTENT_DIRECTORY)?;
                            writer
                                .element("serviceId")
                                .text("urn:upnp-org:serviceId:ContentDirectory")?;
                            writer
                                .element("SCPDURL")
                                .text("/service/ContentDirectory.xml")?;
                            writer.element("controlURL").text("/soap")?;
                            writer.element("eventSubURL").empty()
                        })
                    })
                })
            })
    }
}

pub(crate) struct ServiceDescription {
    descriptors: Vec<(&'static str, &'static [SoapArgument])>,
}

impl ServiceDescription {
    pub(crate) fn new(descriptors: Vec<(&'static str, &'static [SoapArgument])>) -> Self {
        Self { descriptors }
    }
}

impl<W: Write> ToXml<W> for ServiceDescription {
    fn write_xml(&self, writer: &mut XmlWriter<W>) -> Result<(), WriterError> {
        writer
            .element_ns((ns::UPNP_SERVICE, "scpd"))
            .contents(|writer| {
                writer.element("specVersion").contents(|writer| {
                    writer.element("major").text(1)?;
                    writer.element("minor").text(1)
                })?;

                writer.element("actionList").contents(|writer| {
                    for (name, args) in self.descriptors.iter() {
                        writer
                            .element_ns((ns::UPNP_SERVICE, "action"))
                            .contents(|writer| {
                                writer.element_ns((ns::UPNP_SERVICE, "name")).text(name)?;

                                if !args.is_empty() {
                                    writer
                                        .element_ns((ns::UPNP_SERVICE, "argumentList"))
                                        .contents(|writer| {
                                            for (name, direction) in *args {
                                                writer
                                                    .element_ns((ns::UPNP_SERVICE, "argument"))
                                                    .contents(|writer| {
                                                    writer
                                                        .element_ns((ns::UPNP_SERVICE, "name"))
                                                        .text(name)?;
                                                    writer
                                                        .element_ns((ns::UPNP_SERVICE, "direction"))
                                                        .text(direction)
                                                })?;
                                            }

                                            Ok(())
                                        })?;
                                }

                                Ok(())
                            })?;
                    }

                    Ok(())
                })?;

                writer.element("serviceStateTable").empty()
            })
    }
}
