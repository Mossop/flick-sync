use std::{
    env::consts,
    fmt,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    str::{FromStr, from_utf8},
    time::Duration,
};

use anyhow::bail;
use bytes::{Buf, BufMut, BytesMut};
use futures::{FutureExt, select, sink::SinkExt, stream::StreamExt};
use http::{HeaderMap, HeaderName, HeaderValue};
use socket2::{Domain, Protocol, Socket, Type};
use tokio::{net::UdpSocket, time::sleep};
use tokio_util::{
    codec::{Decoder, Encoder},
    udp::UdpFramed,
};
use tracing::{debug, error, instrument, trace, warn};
use uuid::Uuid;

use crate::ns;

const SSDP_IPV4: Ipv4Addr = Ipv4Addr::new(239, 255, 255, 250);
const SSDP_IPV6: Ipv6Addr = Ipv6Addr::new(0xFF02, 0, 0, 0, 0, 0, 0, 0xC);
const SSDP_PORT: u16 = 1900;

#[derive(Debug, Clone)]
enum SsdpMessage {
    MSearch {
        host: String,
        search_target: String,
        max_wait: Option<u64>,
        user_agent: Option<String>,
    },
    Notify {
        host: String,
        notification_type: String,
        unique_service_name: String,
        availability: String,
        location: String,
        server: String,
    },
    SearchResponse {
        location: String,
        server: String,
        search_target: String,
        unique_service_name: String,
    },
}

fn get_header<V>(headers: &HeaderMap<HeaderValue>, header: &str) -> Option<V>
where
    V: FromStr,
    V::Err: fmt::Display,
{
    let value = match headers.get(header)?.to_str() {
        Ok(st) => st,
        Err(e) => {
            warn!(header, error=%e, "Failed to parse header value");
            return None;
        }
    };

    match V::from_str(value.trim_matches('"')) {
        Ok(v) => Some(v),
        Err(e) => {
            warn!(header, error=%e, "Failed to parse header value");
            None
        }
    }
}

fn push_line<A: AsRef<[u8]>>(buf: &mut BytesMut, line: A) {
    let bytes = line.as_ref();
    buf.reserve(bytes.len() + 2);
    buf.put_slice(bytes);
    buf.put_slice(&[13_u8, 10_u8]);
}

impl SsdpMessage {
    fn is_uuid(&self, uuid: Uuid) -> bool {
        match self {
            SsdpMessage::Notify {
                unique_service_name,
                ..
            } => {
                let own_uuid = format!("uuid:{}", uuid.as_hyphenated());
                unique_service_name.starts_with(&own_uuid)
            }
            _ => false,
        }
    }

    fn encode(&self, buffer: &mut BytesMut) {
        match self {
            SsdpMessage::MSearch {
                host,
                search_target,
                max_wait,
                user_agent,
            } => {
                push_line(buffer, "M-SEARCH * HTTP/1.1");
                push_line(buffer, format!("HOST: {host}"));
                push_line(buffer, "MAN: \"ssdp:discover\"");
                push_line(buffer, format!("ST: {}", search_target));

                if let Some(mx) = max_wait {
                    push_line(buffer, format!("MX: {mx}"));
                }

                if let Some(ua) = user_agent {
                    push_line(buffer, format!("USER-AGENT: {ua}"));
                }
            }
            SsdpMessage::Notify {
                host,
                notification_type,
                unique_service_name,
                availability,
                location,
                server,
            } => {
                push_line(buffer, "NOTIFY * HTTP/1.1");
                push_line(buffer, "CACHE_CONTROL: max-age = 120");
                push_line(buffer, format!("HOST: {host}"));
                push_line(buffer, format!("NT: {notification_type}"));
                push_line(buffer, format!("USN: {unique_service_name}"));
                push_line(buffer, format!("NTS: {availability}"));
                push_line(buffer, format!("LOCATION: {location}"));
                push_line(buffer, format!("SERVER: {server}"));
            }
            SsdpMessage::SearchResponse {
                location,
                server,
                search_target,
                unique_service_name,
            } => {
                push_line(buffer, "HTTP/1.1 200 OK");
                push_line(buffer, "CACHE-CONTROL: max-age = 120");
                push_line(buffer, "EXT:");
                push_line(buffer, format!("LOCATION: {location}"));
                push_line(buffer, format!("SERVER: {server}"));
                push_line(buffer, format!("USN: {unique_service_name}"));
                push_line(buffer, format!("ST: {}", search_target));
            }
        }

        push_line(buffer, []);
    }

    #[instrument(skip(headers))]
    fn parse_m_search(headers: HeaderMap<HeaderValue>) -> Option<Self> {
        let Some(host) = get_header::<String>(&headers, "host") else {
            warn!("Missing host header");
            return None;
        };

        let Some(search_target) = get_header::<String>(&headers, "st") else {
            warn!("Missing st header");
            return None;
        };

        let max_wait = get_header::<u64>(&headers, "mx");

        let user_agent = get_header::<String>(&headers, "user-agent");

        Some(Self::MSearch {
            host,
            search_target,
            max_wait,
            user_agent,
        })
    }

    #[instrument(skip(headers))]
    fn parse_notify(headers: HeaderMap<HeaderValue>) -> Option<Self> {
        let Some(host) = get_header::<String>(&headers, "host") else {
            warn!("Missing host header");
            return None;
        };

        let Some(notification_type) = get_header::<String>(&headers, "nt") else {
            warn!("Missing nt header");
            return None;
        };

        let Some(availability) = get_header::<String>(&headers, "nts") else {
            warn!("Missing nts header");
            return None;
        };

        let Some(location) = get_header::<String>(&headers, "location") else {
            warn!("Missing location header");
            return None;
        };

        let Some(unique_service_name) = get_header::<String>(&headers, "usn") else {
            warn!("Missing usn header");
            return None;
        };

        let server = get_header::<String>(&headers, "server").unwrap_or_default();

        Some(Self::Notify {
            host,
            notification_type,
            availability,
            location,
            unique_service_name,
            server,
        })
    }

    fn parse(method: &str, headers: HeaderMap<HeaderValue>) -> Option<Self> {
        match method.to_lowercase().as_str() {
            "m-search" => Self::parse_m_search(headers),
            "notify" => Self::parse_notify(headers),
            _ => {
                warn!(method, "Unknown packet method");
                None
            }
        }
    }
}

fn decode_line(src: &BytesMut, pos: usize) -> anyhow::Result<Option<(&str, usize)>> {
    // This can't be a complete line without at least a CRLF.
    if src.len() < 2 {
        return Ok(None);
    }

    let mut end = pos;

    while end < src.len() && src[end] != 13 {
        end += 1;
    }

    if end >= src.len() - 1 || src[end + 1] != 10 {
        // Not yet enough data to decode.
        return Ok(None);
    }

    Ok(Some((from_utf8(&src[pos..end])?, end + 2)))
}

fn parse_method(line: &str) -> anyhow::Result<&str> {
    if !line.ends_with("HTTP/1.1") {
        bail!("Invalid protocol");
    }

    let line = line[..line.len() - 9].trim();

    let Some(separator) = line.find(' ') else {
        bail!("Missing method separator");
    };

    let method = line[0..separator].trim();
    Ok(method)
}

fn parse_header(line: &str) -> anyhow::Result<(HeaderName, HeaderValue)> {
    let Some(separator) = line.find(':') else {
        bail!("Missing separator");
    };

    let name = HeaderName::try_from(line[0..separator].trim())?;
    let value = HeaderValue::try_from(line[separator + 1..].trim())?;

    Ok((name, value))
}

struct SsdpCodec;

impl Encoder<SsdpMessage> for SsdpCodec {
    type Error = anyhow::Error;

    fn encode(&mut self, item: SsdpMessage, dst: &mut BytesMut) -> Result<(), Self::Error> {
        item.encode(dst);
        Ok(())
    }
}

impl Decoder for SsdpCodec {
    type Item = SsdpMessage;

    type Error = anyhow::Error;

    #[instrument(skip_all)]
    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<SsdpMessage>, Self::Error> {
        // Terribly inefficient but we assume the entire packet is available in one go.
        let mut headers: HeaderMap<HeaderValue> = HeaderMap::new();

        let Some((head_line, mut pos)) = decode_line(src, 0)? else {
            return Ok(None);
        };

        while pos < src.len() {
            let Some((line, next_pos)) = decode_line(src, pos)? else {
                return Ok(None);
            };
            pos = next_pos;

            // This is the end of the headers.
            if line.is_empty() {
                break;
            }

            match parse_header(line) {
                Ok((name, value)) => {
                    headers.insert(name, value);
                }
                Err(e) => {
                    warn!(header = line, error=%e, "Unparseable header found in packet");
                }
            }
        }

        // We have now decoded all the data from the headers.

        let result = match parse_method(head_line) {
            Ok(method) => SsdpMessage::parse(method, headers),
            Err(e) => {
                warn!(
                    line=head_line,
                    error = %e,
                    "Failed to parse packet method"
                );

                None
            }
        };

        src.advance(pos);

        Ok(result)
    }
}

pub(crate) trait Interface: Send + Sync {
    fn address(&self) -> IpAddr;
    fn build_recv(&self) -> anyhow::Result<UdpSocket>;
    fn build_unicast(&self) -> anyhow::Result<UdpSocket>;
    fn build_multicast(&self) -> anyhow::Result<(UdpSocket, SocketAddr)>;
}

pub(crate) struct Ipv6Interface {
    address: Ipv6Addr,
    interface: u32,
}

impl Interface for Ipv6Interface {
    fn address(&self) -> IpAddr {
        IpAddr::V6(self.address)
    }

    fn build_recv(&self) -> anyhow::Result<UdpSocket> {
        let raw_socket = Socket::new(Domain::IPV6, Type::DGRAM, Some(Protocol::UDP))?;
        raw_socket.set_reuse_address(true)?;
        raw_socket.set_nonblocking(true)?;
        raw_socket.bind(&SocketAddr::from((SSDP_IPV6, SSDP_PORT)).into())?;
        raw_socket.join_multicast_v6(&SSDP_IPV6, self.interface)?;
        raw_socket.set_multicast_loop_v6(false)?;

        Ok(UdpSocket::from_std(raw_socket.into())?)
    }

    fn build_unicast(&self) -> anyhow::Result<UdpSocket> {
        let raw_socket = Socket::new(Domain::IPV6, Type::DGRAM, Some(Protocol::UDP))?;
        raw_socket.set_nonblocking(true)?;
        raw_socket.bind(&SocketAddr::from((self.address, 0)).into())?;

        Ok(UdpSocket::from_std(raw_socket.into())?)
    }

    fn build_multicast(&self) -> anyhow::Result<(UdpSocket, SocketAddr)> {
        let raw_socket = Socket::new(Domain::IPV6, Type::DGRAM, Some(Protocol::UDP))?;
        raw_socket.set_nonblocking(true)?;
        raw_socket.bind(&SocketAddr::from((self.address, 0)).into())?;
        raw_socket.set_multicast_if_v6(self.interface)?;

        Ok((
            UdpSocket::from_std(raw_socket.into())?,
            (SSDP_IPV6, SSDP_PORT).into(),
        ))
    }
}

pub(crate) struct Ipv4Interface {
    address: Ipv4Addr,
}

impl Interface for Ipv4Interface {
    fn address(&self) -> IpAddr {
        IpAddr::V4(self.address)
    }

    fn build_recv(&self) -> anyhow::Result<UdpSocket> {
        let raw_socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
        raw_socket.set_reuse_address(true)?;
        raw_socket.set_nonblocking(true)?;
        raw_socket.bind(&SocketAddr::from((SSDP_IPV4, SSDP_PORT)).into())?;
        raw_socket.join_multicast_v4(&SSDP_IPV4, &self.address)?;
        raw_socket.set_multicast_loop_v4(false)?;

        Ok(UdpSocket::from_std(raw_socket.into())?)
    }

    fn build_unicast(&self) -> anyhow::Result<UdpSocket> {
        let raw_socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
        raw_socket.set_nonblocking(true)?;
        raw_socket.bind(&SocketAddr::from((self.address, 0)).into())?;

        Ok(UdpSocket::from_std(raw_socket.into())?)
    }

    fn build_multicast(&self) -> anyhow::Result<(UdpSocket, SocketAddr)> {
        let raw_socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
        raw_socket.set_nonblocking(true)?;
        raw_socket.bind(&SocketAddr::from((self.address, 0)).into())?;
        raw_socket.set_multicast_if_v4(&self.address)?;

        Ok((
            UdpSocket::from_std(raw_socket.into())?,
            (SSDP_IPV4, SSDP_PORT).into(),
        ))
    }
}

impl From<getifaddrs::Interface> for Box<dyn Interface> {
    fn from(value: getifaddrs::Interface) -> Self {
        match value.address {
            IpAddr::V4(ipv4) => Box::new(Ipv4Interface { address: ipv4 }),
            IpAddr::V6(ipv6) => Box::new(Ipv6Interface {
                address: ipv6,
                interface: value.index.unwrap(),
            }),
        }
    }
}

pub(crate) struct SsdpTask {
    uuid: Uuid,
    interface: Box<dyn Interface + 'static>,
    server: String,
    http_port: u16,
}

impl SsdpTask {
    pub(crate) async fn new(
        uuid: Uuid,
        interface: Box<dyn Interface + 'static>,
        server_version: &str,
        http_port: u16,
    ) -> Self {
        Self {
            uuid,
            interface,
            server: format!("{}/0.0 UPnP/1.1 {}", consts::OS, server_version),
            http_port,
        }
    }

    async fn send_messages<I>(&self, socket: UdpSocket, address: SocketAddr, messages: I)
    where
        I: IntoIterator<Item = SsdpMessage>,
    {
        let mut framed = UdpFramed::new(socket, SsdpCodec);

        for message in messages {
            trace!(
                ?message,
                local_address = %self.interface.address(),
                remote_address = %address,
                "Sending SSDP message"
            );

            if let Err(e) = framed.send((message, address)).await {
                error!(error=%e, "Failed to build unicast socket");
            }
        }
    }

    fn notify_message(
        &self,
        address: SocketAddr,
        usn: &str,
        notification_type: &str,
    ) -> SsdpMessage {
        SsdpMessage::Notify {
            host: address.to_string(),
            notification_type: notification_type.to_owned(),
            unique_service_name: usn.to_owned(),
            availability: "ssdp:alive".to_string(),
            location: format!(
                "http://{}:{}/device.xml",
                self.interface.address(),
                self.http_port
            ),
            server: self.server.clone(),
        }
    }

    async fn send_notify(&self) {
        let (socket, address) = match self.interface.build_multicast() {
            Ok(r) => r,
            Err(e) => {
                error!(error=%e, "Failed to build multiicast socket");
                return;
            }
        };

        let usn_base = format!("uuid:{}", self.uuid.as_hyphenated());

        let messages = vec![
            self.notify_message(address, &usn_base, &usn_base),
            self.notify_message(
                address,
                &format!("{}::{}", usn_base, ns::UPNP_ROOT),
                ns::UPNP_ROOT,
            ),
            self.notify_message(
                address,
                &format!("{}::{}", usn_base, ns::UPNP_MEDIASERVER),
                ns::UPNP_MEDIASERVER,
            ),
            self.notify_message(
                address,
                &format!("{}::{}", usn_base, ns::UPNP_CONTENTDIRECTORY),
                ns::UPNP_CONTENTDIRECTORY,
            ),
        ];

        self.send_messages(socket, address, messages).await;
    }

    fn response_message(&self, usn: &str, search_target: &str) -> SsdpMessage {
        SsdpMessage::SearchResponse {
            location: format!(
                "http://{}:{}/device.xml",
                self.interface.address(),
                self.http_port
            ),
            server: self.server.clone(),
            search_target: search_target.to_owned(),
            unique_service_name: usn.to_owned(),
        }
    }

    async fn send_search_response(&self, search_target: &str, address: SocketAddr) {
        let usn_base = format!("uuid:{}", self.uuid.as_hyphenated());
        let mut messages = Vec::new();

        match search_target {
            ns::UPNP_ROOT => {
                messages.push(self.response_message(&usn_base, &usn_base));
                messages.push(
                    self.response_message(
                        &format!("{}::{}", usn_base, ns::UPNP_ROOT),
                        ns::UPNP_ROOT,
                    ),
                );
                messages.push(self.response_message(
                    &format!("{}::{}", usn_base, ns::UPNP_MEDIASERVER),
                    ns::UPNP_MEDIASERVER,
                ));
                messages.push(self.response_message(
                    &format!("{}::{}", usn_base, ns::UPNP_CONTENTDIRECTORY),
                    ns::UPNP_CONTENTDIRECTORY,
                ));
            }
            ns::UPNP_MEDIASERVER | ns::UPNP_CONTENTDIRECTORY => {
                messages.push(
                    self.response_message(
                        &format!("{}::{}", usn_base, search_target),
                        search_target,
                    ),
                );
            }
            _ => {}
        }

        if search_target.to_lowercase() == usn_base {
            messages.push(self.response_message(&usn_base, &usn_base));
        }

        if messages.is_empty() {
            return;
        }

        let socket = match self.interface.build_unicast() {
            Ok(s) => s,
            Err(e) => {
                error!(error=%e, "Failed to build unicast socket");
                return;
            }
        };

        self.send_messages(socket, address, messages).await;
    }

    async fn announce_loop(&self) {
        loop {
            self.send_notify().await;

            sleep(Duration::from_secs(60)).await;
        }
    }

    async fn recv_loop(&self) -> anyhow::Result<()> {
        let socket = self.interface.build_recv()?;
        let mut framed = UdpFramed::new(socket, SsdpCodec);

        loop {
            let (message, remote_address) = match framed.next().await {
                Some(Ok(r)) => r,
                Some(Err(e)) => {
                    error!(error=%e, "Failed to decode SSDP packet");
                    return Ok(());
                }
                None => {
                    debug!("Socket closed unexpectedly");
                    return Ok(());
                }
            };

            if message.is_uuid(self.uuid) {
                continue;
            }

            trace!(?message, local_address=%self.interface.address(), %remote_address, "Received SSDP message");

            if let SsdpMessage::MSearch { search_target, .. } = message {
                self.send_search_response(&search_target, remote_address)
                    .await;
            }
        }
    }

    pub(crate) async fn run(self) {
        select! {
            result = self.recv_loop().fuse() => {
                if let Err(e) = result {
                    error!(error=%e, "Failed listening to multicast traffic");
                }
            },
            _ = self.announce_loop().fuse() => {},
        }
    }
}
