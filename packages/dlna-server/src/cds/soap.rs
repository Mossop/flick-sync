use std::{fmt, io::Write, marker::PhantomData, ops::Deref, str::FromStr};

use actix_web::{
    Resource,
    dev::{AppService, HttpServiceFactory},
    guard::{self, GuardContext},
    web::Data,
};
use mime::Mime;
use serde::{Serialize, de::DeserializeOwned};
use tracing::{Instrument, Level, error, span, trace};

use crate::{
    HttpAppData,
    cds::xml::{
        ClientXmlError, Element, FromXml, ToXml, WriterError, Xml, XmlElement, XmlName, XmlReader,
        XmlWriter,
    },
};

const NS_SOAP_ENVELOPE: &str = "http://schemas.xmlsoap.org/soap/envelope/";
const SOAP_ENCODING: &str = "http://schemas.xmlsoap.org/soap/encoding/";

struct ActionWrapper<A> {
    action: A,
}

impl<A> ActionWrapper<A> {
    fn action(&self) -> &A {
        &self.action
    }
}

impl<A> FromXml for ActionWrapper<A>
where
    A: SoapAction + DeserializeOwned,
{
    fn from_element<R: std::io::Read>(
        _element: Element,
        reader: &mut XmlReader<R>,
    ) -> Result<Self, ClientXmlError> {
        if let Some(element) = reader.next_element()? {
            if element.name.as_ref() == (Some(NS_SOAP_ENVELOPE), "Body") {
                if let Some(element) = reader.next_element()? {
                    if element.name.as_ref() != (Some(A::schema()), A::name()) {
                        return Err("Unexpected body element".into());
                    }

                    let action: A = reader.deserialize()?;
                    Ok(Self { action })
                } else {
                    Err("Missing SOAP request element".into())
                }
            } else {
                Err("Missing body element".into())
            }
        } else {
            Err("Missing body element".into())
        }
    }
}

impl<A> XmlElement for ActionWrapper<A> {
    fn name() -> XmlName {
        (NS_SOAP_ENVELOPE, "Envelope").into()
    }
}

impl<A> Deref for ActionWrapper<A> {
    type Target = A;

    fn deref(&self) -> &Self::Target {
        &self.action
    }
}

struct ResponseWrapper<R: Serialize> {
    name: XmlName,
    response: SoapResult<R>,
}

impl<R, W> ToXml<W> for ResponseWrapper<R>
where
    W: Write,
    R: Serialize,
{
    fn write_xml(&self, writer: &mut XmlWriter<W>) -> Result<(), WriterError> {
        writer
            .element_ns((NS_SOAP_ENVELOPE, "Envelope"))
            .prefix("s", NS_SOAP_ENVELOPE)
            .attr((NS_SOAP_ENVELOPE, "encodingStyle"), SOAP_ENCODING)
            .contents(|writer| {
                writer
                    .element_ns((NS_SOAP_ENVELOPE, "Body"))
                    .contents(|writer| match &self.response {
                        Ok(response) => writer
                            .element_ns(self.name.clone())
                            .prefix("u", self.name.namespace.as_deref().unwrap())
                            .contents(|writer| writer.serialize(response, None)),
                        Err(fault) => fault.write_xml(writer),
                    })
            })
    }
}

pub(crate) struct SoapFactory<T: SoapAction> {
    _type: PhantomData<T>,
}

impl<T: SoapAction> Default for SoapFactory<T> {
    fn default() -> Self {
        Self { _type: PhantomData }
    }
}

impl<T> SoapFactory<T>
where
    T: SoapAction + DeserializeOwned + fmt::Debug + 'static,
    T::Response: Serialize + fmt::Debug,
{
    async fn service(
        app_data: Data<HttpAppData>,
        envelope: Xml<ActionWrapper<T>>,
    ) -> Xml<ResponseWrapper<T::Response>> {
        let span = span!(Level::INFO, "SOAP request", "action" = T::name());

        trace!(parent: &span, request = ?envelope.inner().action());
        let response = envelope.execute().instrument(span.clone()).await;

        match &response {
            Ok(r) => {
                trace!(parent: &span, response = ?r);
            }
            Err(e) => {
                error!(parent: &span, error=?e);
            }
        }
        let name: XmlName = (T::schema(), format!("{}Response", T::name()).as_str()).into();

        Xml::new(ResponseWrapper { name, response })
    }
}

impl<T> HttpServiceFactory for SoapFactory<T>
where
    T: SoapAction + DeserializeOwned + fmt::Debug + 'static,
    T::Response: Serialize + fmt::Debug,
{
    fn register(self, config: &mut AppService) {
        let resource = Resource::new("/soap")
            .name(T::name())
            .guard(guard::Post())
            .guard(T::guard)
            .to(Self::service);
        HttpServiceFactory::register(resource, config)
    }
}

pub(crate) enum ArgDirection {
    In,
    Out,
}

impl fmt::Display for ArgDirection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ArgDirection::In => write!(f, "in"),
            ArgDirection::Out => write!(f, "out"),
        }
    }
}

pub(crate) type SoapArgument = (&'static str, ArgDirection);

pub(crate) trait SoapAction {
    type Response;

    fn schema() -> &'static str;
    fn name() -> &'static str;
    async fn execute(&self) -> SoapResult<Self::Response>;
    fn arguments() -> &'static [SoapArgument];

    fn guard(ctx: &GuardContext) -> bool {
        if let Some(soap_action) = ctx
            .head()
            .headers()
            .get("SOAPAction")
            .and_then(|hv| hv.to_str().ok())
        {
            let expected = format!("{}#{}", Self::schema(), Self::name());
            if expected != soap_action.trim_matches('"') {
                return false;
            }
        } else {
            return false;
        }

        if let Some(mime) = ctx
            .head()
            .headers()
            .get("content-type")
            .and_then(|hv| hv.to_str().ok())
            .and_then(|st| Mime::from_str(st).ok())
        {
            mime.subtype() == mime::XML
                && (mime.type_() == mime::APPLICATION || mime.type_() == mime::TEXT)
        } else {
            false
        }
    }

    fn descriptor() -> (&'static str, &'static [SoapArgument]) {
        (Self::name(), Self::arguments())
    }

    fn factory() -> SoapFactory<Self>
    where
        Self: Sized,
    {
        Default::default()
    }
}

#[derive(Debug)]
pub(crate) struct SoapFault {
    fault_code: String,
    fault_string: String,
}

impl<W: Write> ToXml<W> for SoapFault {
    fn write_xml(&self, writer: &mut XmlWriter<W>) -> Result<(), WriterError> {
        writer
            .element_ns((NS_SOAP_ENVELOPE, "Fault"))
            .contents(|writer| {
                writer.element("faultcode").text(&self.fault_code)?;
                writer.element("faultstring").text(&self.fault_string)
            })
    }
}

pub(crate) type SoapResult<T> = Result<T, SoapFault>;
