use std::io::Write;

use uuid::Uuid;

use crate::cds::{
    SCHEMA_CONNECTION_MANAGER, SCHEMA_CONTENT_DIRECTORY,
    soap::SoapArgument,
    xml::{ToXml, WriterError, XmlWriter},
};

const NS_UPNP_DEVICE: &str = "urn:schemas-upnp-org:device-1-0";
pub(crate) const NS_UPNP_SERVICE: &str = "urn:schemas-upnp-org:service-1-0";

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
                    writer.element("minor").text(0)
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
                    writer.element("modelNumber").empty()?;
                    writer
                        .element("modelURL")
                        .text("https://github.com/Mossop/flick-sync")?;
                    writer.element("serialNumber").empty()?;

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
                            writer.element("controlURL").text("/soap")
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
                    writer.element("minor").text(0)
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
