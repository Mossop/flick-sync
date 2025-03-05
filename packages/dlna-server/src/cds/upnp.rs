use std::io::Write;

use uuid::Uuid;

use crate::cds::{
    SCHEMA_CONNECTION_MANAGER, SCHEMA_CONTENT_DIRECTORY,
    soap::SoapArgument,
    xml::{ToXml, WriterError, XmlWriter},
};

const NS_UPNP_DEVICE: &str = "urn:schemas-upnp-org:device-1-0";
pub(crate) const NS_UPNP_SERVICE: &str = "urn:schemas-upnp-org:service-1-0";
const NS_DIDL: &str = "urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/";
const NS_DC: &str = "http://purl.org/dc/elements/1.1/";
const NS_UPNP: &str = "urn:schemas-upnp-org:metadata-1-0/upnp/";
const NS_DLNA: &str = "urn:schemas-dlna-org:metadata-1-0/";

#[derive(Debug)]
pub struct Container {
    pub id: String,
    pub parent_id: String,
    pub child_count: Option<u32>,
    pub title: String,
}

impl<W: Write> ToXml<W> for Container {
    fn write_xml(&self, writer: &mut XmlWriter<W>) -> Result<(), WriterError> {
        let mut builder = writer.element_ns((NS_DIDL, "container"));
        if let Some(child_count) = self.child_count {
            builder = builder.attr("childCount", child_count);
        }
        builder
            .attr("id", &self.id)
            .attr("parentID", &self.parent_id)
            .attr("restricted", "1")
            .attr("searchable", "0")
            .contents(|writer| {
                writer.element_ns((NS_DC, "title")).text(&self.title)?;
                writer
                    .element_ns((NS_UPNP, "class"))
                    .text("object.container")
            })
    }
}

#[derive(Debug)]
pub struct Item {
    pub id: String,
    pub parent_id: String,
    pub title: String,
}

impl<W: Write> ToXml<W> for Item {
    fn write_xml(&self, writer: &mut XmlWriter<W>) -> Result<(), WriterError> {
        writer
            .element_ns((NS_DIDL, "item"))
            .attr("id", &self.id)
            .attr("parentID", &self.parent_id)
            .attr("restricted", "1")
            .contents(|writer| {
                writer.element_ns((NS_DC, "title")).text(&self.title)?;
                writer.element_ns((NS_UPNP, "class")).text("object.item")
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
pub(crate) struct BrowseResult {
    objects: Vec<Object>,
}

impl From<Vec<Object>> for BrowseResult {
    fn from(objects: Vec<Object>) -> Self {
        Self { objects }
    }
}

impl<W: Write> ToXml<W> for BrowseResult {
    fn write_xml(&self, writer: &mut XmlWriter<W>) -> Result<(), WriterError> {
        writer
            .element_ns((NS_DIDL, "DIDL-Lite"))
            .prefix("dc", NS_DC)
            .prefix("dlna", NS_DLNA)
            .prefix("upnp", NS_UPNP)
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
            .element_ns((NS_UPNP_DEVICE, "root"))
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
                            writer
                                .element("serviceType")
                                .text(SCHEMA_CONNECTION_MANAGER)?;
                            writer
                                .element("serviceId")
                                .text("urn:upnp-org:serviceId:ConnectionManager")?;
                            writer
                                .element("SCPDURL")
                                .text("/service/ConnectionManager.xml")?;
                            writer.element("controlURL").text("/soap")
                        })?;

                        writer.element("service").contents(|writer| {
                            writer
                                .element("serviceType")
                                .text(SCHEMA_CONTENT_DIRECTORY)?;
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
            .element_ns((NS_UPNP_SERVICE, "scpd"))
            .contents(|writer| {
                writer.element("specVersion").contents(|writer| {
                    writer.element("major").text(1)?;
                    writer.element("minor").text(1)
                })?;

                writer.element("actionList").contents(|writer| {
                    for (name, args) in self.descriptors.iter() {
                        writer
                            .element_ns((NS_UPNP_SERVICE, "action"))
                            .contents(|writer| {
                                writer.element_ns((NS_UPNP_SERVICE, "name")).text(name)?;

                                if !args.is_empty() {
                                    writer
                                        .element_ns((NS_UPNP_SERVICE, "argumentList"))
                                        .contents(|writer| {
                                            for (name, direction) in *args {
                                                writer
                                                    .element_ns((NS_UPNP_SERVICE, "argument"))
                                                    .contents(|writer| {
                                                        writer
                                                            .element_ns((NS_UPNP_SERVICE, "name"))
                                                            .text(name)?;
                                                        writer
                                                            .element_ns((
                                                                NS_UPNP_SERVICE,
                                                                "direction",
                                                            ))
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
