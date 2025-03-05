use std::{fmt, io::Write, marker::PhantomData, ops::Deref, str::FromStr};

use actix_web::{
    FromRequest, HttpRequest, HttpResponse, HttpResponseBuilder, Resource, Responder,
    body::BoxBody,
    dev::{AppService, HttpServiceFactory},
    guard::{self, GuardContext},
    http::{StatusCode, header},
    web::{Data, Payload},
};
use mime::Mime;
use serde::{Serialize, de::DeserializeOwned};
use tracing::{Instrument, Level, error, field, span};
use url::Url;

use crate::{
    DlnaRequestHandler, HttpAppData,
    cds::{
        upnp::UpnpError,
        xml::{
            ClientXmlError, Element, FromXml, ToXml, WriterError, Xml, XmlElement, XmlName,
            XmlReader, XmlWriter,
        },
    },
};

const NS_SOAP_ENVELOPE: &str = "http://schemas.xmlsoap.org/soap/envelope/";
const SOAP_ENCODING: &str = "http://schemas.xmlsoap.org/soap/encoding/";

struct ActionWrapper<A>(A);

impl<A> ActionWrapper<A> {
    fn action(&self) -> &A {
        &self.0
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
                    Ok(Self(action))
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
        &self.0
    }
}

pub(crate) struct ResponseWrapper<R: Serialize> {
    name: XmlName,
    response: SoapResult<R>,
}

impl ResponseWrapper<()> {
    pub(crate) fn error(err: UpnpError) -> Self {
        Self {
            name: ("", "").into(),
            response: Err(err),
        }
    }
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
            .attr_ns((NS_SOAP_ENVELOPE, "encodingStyle"), SOAP_ENCODING)
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

impl<R> Responder for ResponseWrapper<R>
where
    R: Serialize,
{
    type Body = BoxBody;

    fn respond_to(self, req: &HttpRequest) -> HttpResponse<Self::Body> {
        let mut body = Vec::<u8>::new();

        let status_code = match &self.response {
            Ok(_) => StatusCode::OK,
            Err(error) => error.status_code(),
        };

        if let Err(e) = XmlWriter::write_document(&self, &mut body, Some(req.full_url())) {
            error!(error=%e, "Failed to serialize XML document");
            return HttpResponseBuilder::new(StatusCode::INTERNAL_SERVER_ERROR).finish();
        }

        HttpResponseBuilder::new(status_code)
            .insert_header(header::ContentType(mime::TEXT_XML))
            .insert_header(header::ContentLength(body.len()))
            .body(body)
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
        request: HttpRequest,
        payload: Payload,
    ) -> ResponseWrapper<T::Response> {
        let span = span!(
            Level::INFO,
            "SOAP request",
            "action" = T::name(),
            "arguments" = field::Empty
        );

        let name: XmlName = (T::schema(), format!("{}Response", T::name()).as_str()).into();
        let mut payload = payload.into_inner();

        let parse_result = Xml::<ActionWrapper<T>>::from_request(&request, &mut payload)
            .instrument(span.clone())
            .await;

        let envelope = match parse_result {
            Ok(e) => e,
            Err(ClientXmlError::ArgumentError { message }) => {
                error!(parent: &span, error=message, "Client sent invalid arguments.");
                return ResponseWrapper {
                    name,
                    response: Err(UpnpError::InvalidArgs),
                };
            }
            Err(e) => {
                error!(parent: &span, error=%e, "Unable to parse SOAP envelope.");
                return ResponseWrapper {
                    name,
                    response: Err(UpnpError::InvalidArgs),
                };
            }
        };

        span.record("arguments", format!("{:?}", envelope.action()));

        let context = RequestContext {
            base: request.full_url(),
            handler: &*app_data.handler,
        };

        let response = envelope
            .execute(context)
            .instrument(span.clone())
            .await
            .inspect_err(|e| error!(parent: &span, error=?e));

        ResponseWrapper { name, response }
    }
}

impl<T> HttpServiceFactory for SoapFactory<T>
where
    T: SoapAction + DeserializeOwned + fmt::Debug + 'static,
    T::Response: Serialize + fmt::Debug,
{
    fn register(self, config: &mut AppService) {
        let resource = Resource::new("")
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

pub(crate) struct RequestContext<'a, H: DlnaRequestHandler + ?Sized> {
    pub(crate) base: Url,
    pub(crate) handler: &'a H,
}

pub(crate) trait SoapAction {
    type Response;

    fn schema() -> &'static str;
    fn name() -> &'static str;
    async fn execute<H: DlnaRequestHandler + ?Sized>(
        &self,
        context: RequestContext<'_, H>,
    ) -> SoapResult<Self::Response>;
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

impl<W: Write> ToXml<W> for UpnpError {
    fn write_xml(&self, writer: &mut XmlWriter<W>) -> Result<(), WriterError> {
        let status = self.status();
        let fault_code = if matches!(status, 400..500) {
            "Client"
        } else {
            "Server"
        };

        writer
            .element_ns((NS_SOAP_ENVELOPE, "Fault"))
            .contents(|writer| {
                writer
                    .element("faultcode")
                    .text(format!("s:{fault_code}"))?;
                writer.element("faultstring").text("UPnPError")?;
                writer.element("detail").contents(|writer| {
                    writer
                        .element_ns(("urn:schemas-upnp-org:control-1-0", "UPnPError"))
                        .contents(|writer| writer.element("errorCode").text(status))
                })
            })
    }
}

pub(crate) type SoapResult<T> = Result<T, UpnpError>;

// impl Responder for UpnpError {
//     type Body = <ResponseWrapper<()> as Responder>::Body;

//     fn respond_to(self, req: &HttpRequest) -> HttpResponse<Self::Body> {
//         let wrapper = ResponseWrapper::<()> {
//             name: ("", "").into(),
//             response: Err(self),
//         };
//         wrapper.respond_to(req)
//     }
// }
