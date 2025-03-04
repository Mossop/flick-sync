use std::{
    fmt,
    io::{Read, Write},
    num::{ParseFloatError, ParseIntError},
    ops::Deref,
    pin::Pin,
    str::FromStr,
};

use ::serde::{Serialize, de::DeserializeOwned};
use actix_web::{
    FromRequest, HttpMessage, HttpRequest, HttpResponse, HttpResponseBuilder, Responder,
    ResponseError,
    body::BoxBody,
    dev::Payload,
    http::{StatusCode, header},
};
use bytes::Bytes;
use mime::Mime;
use thiserror::Error;
use tracing::{debug, error};
use xml::{
    EmitterConfig, EventReader, EventWriter,
    common::XmlVersion,
    name::{Name, OwnedName},
    reader::{self},
    writer,
};

use crate::cds::xml::serde::{XmlDeserializer, XmlSerializer};

mod serde;

type Map<K, V> = std::collections::BTreeMap<K, V>;

fn application_xml() -> Mime {
    Mime::from_str("application/xml").unwrap()
}

#[derive(Debug, Error)]
pub(crate) enum WriterError {
    #[error("{source}")]
    Xml {
        #[from]
        source: xml::writer::Error,
    },
    #[error("{message}")]
    Custom { message: String },
}

/// An XML name made up of a namespace and local name.
#[derive(Clone, PartialEq, Hash, Debug, Eq, PartialOrd, Ord)]
pub(crate) struct XmlName {
    pub(crate) namespace: Option<String>,
    pub(crate) local_name: String,
}

impl XmlName {
    /// Creates a new name with a defined namespace.
    pub(crate) fn qualified(namespace: &str, local_name: &str) -> Self {
        Self {
            namespace: Some(namespace.to_owned()),
            local_name: local_name.to_owned(),
        }
    }

    /// Gets a reference to the name in a way that is easy to match with a pattern expression.
    pub(crate) fn as_ref(&self) -> (Option<&str>, &str) {
        (self.namespace.as_deref(), &self.local_name)
    }
}

impl fmt::Display for XmlName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(ref ns) = self.namespace {
            write!(f, "{}#{}", ns, self.local_name)
        } else {
            write!(f, "{}", self.local_name)
        }
    }
}

impl From<OwnedName> for XmlName {
    fn from(name: OwnedName) -> Self {
        XmlName {
            namespace: name.namespace,
            local_name: name.local_name,
        }
    }
}

impl<'a> From<Name<'a>> for XmlName {
    fn from(name: Name<'a>) -> Self {
        XmlName {
            namespace: name.namespace.map(|ns| ns.to_owned()),
            local_name: name.local_name.to_owned(),
        }
    }
}

impl<'a> From<(&'a str, &'a str)> for XmlName {
    fn from((ns, local): (&'a str, &'a str)) -> XmlName {
        XmlName::qualified(ns, local)
    }
}

/// Represents an element in an XML document including its tag name and any attributes.
pub(crate) struct Element {
    pub(crate) name: XmlName,
    pub(crate) attributes: Map<XmlName, String>,
}

/// A simplified view of an XML document. It supports documents where every element either contains
/// only text or only child elements (with optional whitespace). Elements that contain both text and
/// child elements will throw an error.
pub(crate) struct XmlReader<R: Read> {
    reader: EventReader<R>,
}

impl<R: Read> XmlReader<R> {
    pub(crate) fn new(reader: R) -> Self {
        Self {
            reader: EventReader::new(reader),
        }
    }

    pub(crate) fn deserialize<T: DeserializeOwned>(&mut self) -> Result<T, ClientXmlError> {
        let deserializer = XmlDeserializer { reader: self };

        T::deserialize(deserializer)
    }

    /// Gets the next child element from the current element returning null when the current element closes.
    pub(crate) fn next_element(&mut self) -> Result<Option<Element>, ClientXmlError> {
        loop {
            let event = self.reader.next()?;

            match event {
                reader::XmlEvent::EndDocument => {
                    return Err("Unexpected end of XML document".into());
                }
                reader::XmlEvent::StartElement {
                    name, attributes, ..
                } => {
                    let mut attrs = Map::new();
                    for attr in attributes {
                        attrs.insert(attr.name.into(), attr.value);
                    }

                    return Ok(Some(Element {
                        name: name.into(),
                        attributes: attrs,
                    }));
                }
                reader::XmlEvent::EndElement { .. } => {
                    return Ok(None);
                }
                reader::XmlEvent::CData(_) => return Err("Unexpected CDATA in element".into()),
                reader::XmlEvent::Characters(_) => return Err("Unexpected text in element".into()),
                _ => {}
            }
        }
    }

    /// Gets the text content of the current element.
    pub(crate) fn text(&mut self) -> Result<Option<String>, ClientXmlError> {
        let mut content = String::new();
        let mut saw_text = false;

        loop {
            let event = self.reader.next()?;

            match event {
                reader::XmlEvent::StartDocument { .. } => return Err("Unexpected XML state".into()),
                reader::XmlEvent::EndDocument => {
                    return Err("Unexpected end of XML document".into());
                }
                reader::XmlEvent::ProcessingInstruction { .. } => {
                    return Err("Unexpected processing instruction".into());
                }
                reader::XmlEvent::StartElement { name, .. } => {
                    return Err(format!("Unexpected element {name} when expecting text").into());
                }
                reader::XmlEvent::EndElement { .. } => break,
                reader::XmlEvent::CData(text) | reader::XmlEvent::Characters(text) => {
                    content += &text;
                    saw_text = true;
                }
                reader::XmlEvent::Whitespace(text) => {
                    content += &text;
                }
                reader::XmlEvent::Comment(_) => {}
            }
        }

        if saw_text {
            Ok(Some(content))
        } else {
            Ok(None)
        }
    }
}

/// A builder for a new element to be written to an XML document.
#[must_use]
pub(crate) struct ElementBuilder<'a, W: Write> {
    previous_prefixes: Map<String, String>,
    tag_name: XmlName,
    attributes: Map<XmlName, String>,
    new_prefixes: Map<String, String>,
    writer: &'a mut XmlWriter<W>,
}

impl<W: Write> ElementBuilder<'_, W> {
    fn to_name(&self, xml_name: &XmlName) -> OwnedName {
        if let Some(ref ns) = xml_name.namespace {
            let prefix = self.writer.known_prefixes.get(ns).unwrap().clone();

            OwnedName::qualified(
                &xml_name.local_name,
                ns,
                if prefix.is_empty() {
                    None
                } else {
                    Some(prefix)
                },
            )
        } else {
            OwnedName::local(&xml_name.local_name)
        }
    }

    fn has_new_prefix(&self, new_prefix: &str) -> bool {
        for prefix in self.new_prefixes.values() {
            if prefix == new_prefix {
                return true;
            }
        }

        false
    }

    fn ensure_prefix(&mut self, namespace: String, is_element: bool) {
        if self.previous_prefixes.contains_key(&namespace)
            || self.new_prefixes.contains_key(&namespace)
        {
            return;
        }

        if is_element {
            self.new_prefixes.insert(namespace, "".to_string());
        } else {
            for new_prefix in 'a'..='z' {
                let new_prefix = new_prefix.to_string();
                if !self.has_new_prefix(&new_prefix) {
                    self.new_prefixes.insert(namespace, new_prefix);
                    return;
                }
            }

            panic!("Too many prefixes already reserved");
        }
    }

    fn build(&mut self) -> Result<(), WriterError> {
        if let Some(ref ns) = self.tag_name.namespace {
            self.ensure_prefix(ns.clone(), true);
        }

        let attr_namespaces: Vec<String> = self
            .attributes
            .keys()
            .filter_map(|k| k.namespace.clone())
            .collect();
        for attr_namespace in attr_namespaces {
            self.ensure_prefix(attr_namespace, false);
        }

        for (uri, prefix) in &self.new_prefixes {
            self.writer
                .known_prefixes
                .insert(uri.clone(), prefix.clone());
        }

        let element_name = self.to_name(&self.tag_name);
        let mut event = writer::XmlEvent::start_element(element_name.borrow());

        for (uri, prefix) in &self.new_prefixes {
            event = if prefix.is_empty() {
                event.default_ns(uri)
            } else {
                event.ns(prefix, uri)
            };
        }

        let attrs: Vec<(OwnedName, &String)> = self
            .attributes
            .iter()
            .map(|(name, value)| (self.to_name(name), value))
            .collect();

        for (name, value) in attrs.iter() {
            event = event.attr(name.borrow(), value);
        }

        self.writer.writer.write(event)?;
        Ok(())
    }

    fn done(self) -> Result<(), WriterError> {
        self.writer.writer.write(writer::XmlEvent::end_element())?;
        self.writer.known_prefixes = self.previous_prefixes;
        Ok(())
    }

    /// Adds an attribute to this element.
    pub(crate) fn attr<N: Into<XmlName>, D: ToString>(mut self, name: N, value: D) -> Self {
        self.attributes.insert(name.into(), value.to_string());
        self
    }

    /// Adds a namespace prefix mapping to this element.
    pub(crate) fn prefix(mut self, prefix: &str, uri: &str) -> Self {
        self.new_prefixes.insert(uri.to_owned(), prefix.to_owned());
        self
    }

    /// Writes out an empty element.
    pub(crate) fn empty(self) -> Result<(), WriterError> {
        self.contents(|_w| Ok(()))
    }

    /// Writes out an containing text.
    pub(crate) fn text<T: ToString>(self, text: T) -> Result<(), WriterError> {
        self.contents(|writer| {
            writer
                .writer
                .write(writer::XmlEvent::characters(&text.to_string()))?;
            Ok(())
        })
    }

    /// Writes out an element with contents built with the provided closure.
    pub(crate) fn contents<F>(mut self, cb: F) -> Result<(), WriterError>
    where
        F: for<'b> FnOnce(&'b mut XmlWriter<W>) -> Result<(), WriterError>,
    {
        self.build()?;
        cb(self.writer)?;
        self.done()
    }
}

pub(crate) struct XmlWriter<W: Write> {
    writer: EventWriter<W>,
    known_prefixes: Map<String, String>,
}

impl<W: Write> XmlWriter<W> {
    fn write_document<X: ToXml<W>>(source: X, sink: W) -> Result<(), WriterError> {
        let mut writer = EmitterConfig::new()
            .perform_indent(true)
            .create_writer(sink);

        writer.write(writer::XmlEvent::StartDocument {
            version: XmlVersion::Version10,
            encoding: Some("UTF-8"),
            standalone: None,
        })?;

        source.write_xml(&mut XmlWriter {
            writer,
            known_prefixes: Map::new(),
        })
    }

    pub(crate) fn serialize<S: Serialize>(
        &mut self,
        strct: S,
        namespace: Option<&str>,
    ) -> Result<(), WriterError> {
        let serializer = XmlSerializer::new(&mut self.writer, namespace);

        strct.serialize(serializer)
    }

    /// Creates a new named element in this document.
    pub(crate) fn element_ns<T>(&mut self, tag_name: T) -> ElementBuilder<'_, W>
    where
        T: Into<XmlName>,
    {
        ElementBuilder {
            previous_prefixes: self.known_prefixes.clone(),
            tag_name: tag_name.into(),
            attributes: Map::new(),
            new_prefixes: Map::new(),
            writer: self,
        }
    }

    /// Creates a new element in this document using whatever is the current default namespace.
    pub(crate) fn element(&mut self, tag_name: &str) -> ElementBuilder<'_, W> {
        let default_ns = self
            .known_prefixes
            .iter()
            .find(|(_, prefix)| prefix.is_empty())
            .map(|(url, _)| url)
            .unwrap();
        let name = XmlName::qualified(default_ns, tag_name);

        self.element_ns(name)
    }
}

/// Represents a type that can be written to an XML document.
pub(crate) trait ToXml<W: Write> {
    /// Writes this type to the XML document using the provided writer.
    fn write_xml(&self, writer: &mut XmlWriter<W>) -> Result<(), WriterError>;
}

impl<W: Write, F> ToXml<W> for F
where
    F: for<'a> Fn(&'a mut XmlWriter<W>) -> Result<(), WriterError>,
{
    fn write_xml(&self, writer: &mut XmlWriter<W>) -> Result<(), WriterError> {
        self(writer)
    }
}

/// Represents a type that can be read from an XML document.
pub(crate) trait FromXml: Sized {
    /// Parses this type from the XML document.
    fn from_element<R: Read>(
        element: Element,
        reader: &mut XmlReader<R>,
    ) -> Result<Self, ClientXmlError>;
}

/// Represents a type that is contained in a single XML element.
pub(crate) trait XmlElement {
    /// Return the expected root element tag name.
    fn name() -> XmlName;
}

/// An XML document that can be read from a HTTP request body or written to a HTTP response body.
pub(crate) struct Xml<T>(T);

impl<T> Deref for Xml<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> Xml<T> {
    pub(crate) fn new(document: T) -> Self {
        Self(document)
    }

    pub(crate) fn inner(&self) -> &T {
        &self.0
    }
}

impl<T> Responder for Xml<T>
where
    T: for<'a> ToXml<&'a mut Vec<u8>>,
{
    type Body = BoxBody;

    fn respond_to(self, _req: &HttpRequest) -> HttpResponse<Self::Body> {
        let mut body = Vec::<u8>::new();

        {
            if let Err(e) = XmlWriter::write_document(self.0, &mut body) {
                error!(error=%e, "Failed to serialize XML document");
                return HttpResponseBuilder::new(StatusCode::INTERNAL_SERVER_ERROR).finish();
            }
        }

        HttpResponseBuilder::new(StatusCode::OK)
            .insert_header(header::ContentType(application_xml()))
            .insert_header(header::ContentLength(body.len()))
            .body(body)
    }
}

#[derive(Debug, Error)]
pub(crate) enum ClientXmlError {
    #[error("{source}")]
    Xml {
        #[from]
        source: xml::reader::Error,
    },
    #[error("{source}")]
    Web {
        #[from]
        source: actix_web::Error,
    },
    #[error("{source}")]
    Float {
        #[from]
        source: ParseFloatError,
    },
    #[error("{source}")]
    Int {
        #[from]
        source: ParseIntError,
    },
    #[error("{message}")]
    Custom { message: String },
}

impl From<&str> for ClientXmlError {
    fn from(value: &str) -> Self {
        Self::Custom {
            message: value.to_owned(),
        }
    }
}

impl From<String> for ClientXmlError {
    fn from(value: String) -> Self {
        Self::Custom { message: value }
    }
}

impl ResponseError for ClientXmlError {
    fn status_code(&self) -> StatusCode {
        StatusCode::BAD_REQUEST
    }
}

impl<T> FromRequest for Xml<T>
where
    T: XmlElement + FromXml,
{
    type Error = ClientXmlError;

    type Future = Pin<Box<dyn Future<Output = Result<Self, Self::Error>>>>;

    fn from_request(req: &HttpRequest, payload: &mut Payload) -> Self::Future {
        let req = req.clone();
        let mut payload = payload.take();

        Box::pin(async move {
            let content_type = req.content_type();
            debug!(content_type);
            if content_type != "application/xml" && content_type != "text/xml" {
                return Err(format!("Unexpected content-type: {content_type}").into());
            }

            let bytes = Bytes::from_request(&req, &mut payload).await?;
            debug!(bytes = bytes.len());

            let mut reader = XmlReader::new(bytes.as_ref());

            let result = if let Some(element) = reader.next_element()? {
                let expected_name = T::name();
                if element.name == expected_name {
                    T::from_element(element, &mut reader)?
                } else {
                    return Err(format!("Unexpected document element {}", element.name).into());
                }
            } else {
                return Err("Unexpected end of XML document".into());
            };

            Ok(Xml::new(result))
        })
    }
}

#[cfg(test)]
mod test {
    use serde::{Deserialize, Serialize};

    use crate::cds::xml::XmlReader;

    use super::{WriterError, XmlWriter};

    fn write_xml_to_string<F>(source: F) -> String
    where
        F: for<'a> Fn(&mut XmlWriter<&'a mut Vec<u8>>) -> Result<(), WriterError>,
    {
        let mut body = Vec::<u8>::new();
        XmlWriter::write_document(source, &mut body).unwrap();
        body.push(b'\n');
        String::from_utf8(body).unwrap()
    }

    #[derive(Deserialize, Serialize)]
    #[serde(rename_all = "PascalCase")]
    struct Serializable {
        a: String,
        b: u32,
        c: bool,
    }

    #[test]
    fn test_deserialization() {
        let mut reader = XmlReader::new(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<root xmlns="urn:schemas-upnp-org:device-1-0" xmlns:a="urn:schemas-dlna-org:device-1-0" a:other="45" test="56">
    <A>hello</A>
    <B>23</B>
    <C>0</C>
</root>
"#.as_bytes(),
        );

        let element = reader.next_element().unwrap().unwrap();
        assert_eq!(
            element.name.namespace.as_deref(),
            Some("urn:schemas-upnp-org:device-1-0")
        );
        assert_eq!(element.name.local_name, "root");

        let mut attrs = element.attributes.iter();
        let (attr_name, value) = attrs.next().unwrap();
        assert_eq!(attr_name.namespace, None);
        assert_eq!(attr_name.local_name, "test");
        assert_eq!(value, "56");

        let (attr_name, value) = attrs.next().unwrap();
        assert_eq!(
            attr_name.namespace.as_deref(),
            Some("urn:schemas-dlna-org:device-1-0")
        );
        assert_eq!(attr_name.local_name, "other");
        assert_eq!(value, "45");

        assert!(attrs.next().is_none());

        let test_struct: Serializable = reader.deserialize().unwrap();

        assert_eq!(test_struct.a, "hello");
        assert_eq!(test_struct.b, 23);
        assert!(!test_struct.c);
    }

    #[test]
    fn test_serialization() {
        let serialized = write_xml_to_string(|writer| {
            writer
                .element_ns(("urn:schemas-upnp-org:device-1-0", "root"))
                .attr(("urn:schemas-upnp-org:device-1-0", "test"), "56")
                .attr(("urn:schemas-dlna-org:device-1-0", "other"), 45)
                .contents(|writer| {
                    writer
                        .element_ns(("urn:schemas-upnp-org:device-1-0", "major"))
                        .text("1")?;
                    writer
                        .element_ns(("urn:schemas-dlna-org:device-1-0", "minor"))
                        .text(0)?;
                    writer
                        .element_ns(("urn:schemas-dlna-org:inner-1-0", "patch"))
                        .prefix("i", "urn:schemas-dlna-org:inner-1-0")
                        .text("boo")?;
                    writer
                        .element_ns(("urn:schemas-dlna-org:device-1-0", "TestStruct"))
                        .contents(|writer| {
                            let struct_test = Serializable {
                                a: "foo".to_string(),
                                b: 32,
                                c: true,
                            };

                            writer.serialize(struct_test, None)
                        })
                })
        });

        assert_eq!(
            serialized,
            r#"<?xml version="1.0" encoding="UTF-8"?>
<root xmlns="urn:schemas-upnp-org:device-1-0" xmlns:a="urn:schemas-dlna-org:device-1-0" a:other="45" test="56">
  <major>1</major>
  <a:minor>0</a:minor>
  <i:patch xmlns:i="urn:schemas-dlna-org:inner-1-0">boo</i:patch>
  <a:TestStruct>
    <A>foo</A>
    <B>32</B>
    <C>1</C>
  </a:TestStruct>
</root>
"#
        )
    }
}
