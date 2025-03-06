use std::{fmt, io::Write, ops::Deref};

use actix_web::{
    FromRequest, HttpRequest, HttpResponse, HttpResponseBuilder, Responder,
    body::BoxBody,
    http::{StatusCode, header},
    web::{Data, Payload},
};
use serde::{Serialize, de::DeserializeOwned};
use tracing::{Instrument, Level, error, field, span};
use url::Url;

use crate::{
    DlnaRequestHandler, HttpAppData, ns,
    upnp::UpnpError,
    xml::{
        ClientXmlError, Element, FromXml, ToXml, WriterError, Xml, XmlElement, XmlName, XmlReader,
        XmlWriter,
    },
};

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
            if element.name.as_ref() == (Some(ns::SOAP_ENVELOPE), "Body") {
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
        (ns::SOAP_ENVELOPE, "Envelope").into()
    }
}

impl<A> Deref for ActionWrapper<A> {
    type Target = A;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub(crate) struct Envelope<B>(B);

impl<B, W> ToXml<W> for Envelope<B>
where
    W: Write,
    B: ToXml<W>,
{
    fn write_xml(&self, writer: &mut XmlWriter<W>) -> Result<(), WriterError> {
        writer
            .element_ns((ns::SOAP_ENVELOPE, "Envelope"))
            .prefix("s", ns::SOAP_ENVELOPE)
            .attr_ns((ns::SOAP_ENVELOPE, "encodingStyle"), ns::SOAP_ENCODING)
            .contents(|writer| {
                writer
                    .element_ns((ns::SOAP_ENVELOPE, "Body"))
                    .contents(|writer| self.0.write_xml(writer))
            })
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

pub(crate) struct RequestContext<'a, H: DlnaRequestHandler> {
    pub(crate) base: Url,
    pub(crate) handler: &'a H,
}

pub(crate) struct SoapResponse<T: SoapAction> {
    response: SoapResult<T::Response>,
}

impl<W, T> ToXml<W> for SoapResponse<T>
where
    T: SoapAction,
    T::Response: Serialize,
    W: Write,
{
    fn write_xml(&self, writer: &mut XmlWriter<W>) -> Result<(), WriterError> {
        match &self.response {
            Ok(r) => {
                let name = format!("{}Response", T::name());
                writer
                    .element_ns((T::schema(), name.as_str()))
                    .prefix("u", T::schema())
                    .contents(|writer| writer.serialize(r, None))
            }
            Err(e) => e.write_xml(writer),
        }
    }
}

impl<T> Responder for SoapResponse<T>
where
    T: SoapAction,
    T::Response: Serialize,
{
    type Body = BoxBody;

    fn respond_to(self, req: &HttpRequest) -> HttpResponse<Self::Body> {
        let mut body = Vec::<u8>::new();

        let status_code = match &self.response {
            Ok(_) => StatusCode::OK,
            Err(error) => error.status_code(),
        };

        if let Err(e) = XmlWriter::write_document(&Envelope(self), &mut body, Some(req.full_url()))
        {
            error!(error=%e, "Failed to serialize XML document");
            return HttpResponseBuilder::new(StatusCode::INTERNAL_SERVER_ERROR).finish();
        }

        HttpResponseBuilder::new(status_code)
            .insert_header(header::ContentType(mime::TEXT_XML))
            .insert_header(header::ContentLength(body.len()))
            .body(body)
    }
}

pub(crate) trait SoapAction
where
    Self: Sized + fmt::Debug + DeserializeOwned,
{
    type Response: Serialize;

    fn schema() -> &'static str;
    fn name() -> &'static str;
    async fn execute<H: DlnaRequestHandler>(
        &self,
        context: RequestContext<'_, H>,
    ) -> SoapResult<Self::Response>;
    fn arguments() -> &'static [SoapArgument];

    fn soap_action() -> String {
        format!("{}#{}", Self::schema(), Self::name())
    }

    fn descriptor() -> (&'static str, &'static [SoapArgument]) {
        (Self::name(), Self::arguments())
    }

    async fn service<H: DlnaRequestHandler>(
        request: HttpRequest,
        payload: Payload,
        app_data: Data<HttpAppData<H>>,
    ) -> HttpResponse {
        let span = span!(
            Level::INFO,
            "SOAP request",
            "action" = Self::name(),
            "arguments" = field::Empty
        );

        let mut payload = payload.into_inner();

        let parse_result = Xml::<ActionWrapper<Self>>::from_request(&request, &mut payload)
            .instrument(span.clone())
            .await;

        let envelope = match parse_result {
            Ok(e) => e,
            Err(ClientXmlError::ArgumentError { message }) => {
                error!(parent: &span, error=message, "Client sent invalid arguments.");
                return UpnpError::InvalidArgs.respond_to(&request);
            }
            Err(e) => {
                error!(parent: &span, error=%e, "Unable to parse SOAP envelope.");
                return UpnpError::InvalidArgs.respond_to(&request);
            }
        };

        span.record("arguments", format!("{:?}", envelope.action()));

        let context = RequestContext {
            base: request.full_url(),
            handler: &app_data.handler,
        };

        let response = envelope
            .execute(context)
            .instrument(span.clone())
            .await
            .inspect_err(|e| error!(parent: &span, error=?e));

        SoapResponse::<Self> { response }.respond_to(&request)
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
            .element_ns((ns::SOAP_ENVELOPE, "Fault"))
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

impl Responder for UpnpError {
    type Body = BoxBody;

    fn respond_to(self, req: &HttpRequest) -> HttpResponse<Self::Body> {
        let mut body = Vec::<u8>::new();

        let status_code = self.status_code();

        if let Err(e) = XmlWriter::write_document(&Envelope(self), &mut body, Some(req.full_url()))
        {
            error!(error=%e, "Failed to serialize XML document");
            return HttpResponseBuilder::new(StatusCode::INTERNAL_SERVER_ERROR).finish();
        }

        HttpResponseBuilder::new(status_code)
            .insert_header(header::ContentType(mime::TEXT_XML))
            .insert_header(header::ContentLength(body.len()))
            .body(body)
    }
}

pub(crate) type SoapResult<T> = Result<T, UpnpError>;
