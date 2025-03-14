use std::{
    collections::HashMap,
    env::consts,
    fmt,
    io::{self, ErrorKind},
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    pin::{Pin, pin},
    str::{FromStr, from_utf8},
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};

use anyhow::bail;
use bytes::{Buf, BytesMut};
use futures::{Stream, StreamExt};
use getifaddrs::{InterfaceFlags, getifaddrs};
use http::{HeaderMap, HeaderName, HeaderValue};
use pin_project::pin_project;
use socket_pktinfo::PktInfoUdpSocket;
use socket2::{Domain, Protocol, Socket, Type};
use tokio::{
    net::UdpSocket,
    sync::{Notify, futures::Notified},
    time,
};
use tracing::{debug, error, info, instrument, trace, warn};
use uuid::Uuid;

use crate::{TaskHandle, ns, rt};

pub(crate) const SSDP_IPV4: Ipv4Addr = Ipv4Addr::new(239, 255, 255, 250);
pub(crate) const SSDP_IPV6: Ipv6Addr = Ipv6Addr::new(0xFF02, 0, 0, 0, 0, 0, 0, 0xC);
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
        location: Option<String>,
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
    buf.extend_from_slice(bytes);
    buf.extend_from_slice(&[13, 10]);
}

fn decode_line(src: &[u8], pos: usize) -> anyhow::Result<Option<(&str, usize)>> {
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

    Ok(Some((from_utf8(&src[pos..end])?.trim(), end + 2)))
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
                if let Some(location) = location {
                    push_line(buffer, format!("LOCATION: {location}"));
                }
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

        let location = get_header::<String>(&headers, "location");

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

    #[instrument(skip_all)]
    fn decode(buffer: &mut BytesMut) -> anyhow::Result<Option<Self>> {
        // Terribly inefficient but we assume the entire packet is available in one go.
        let mut headers: HeaderMap<HeaderValue> = HeaderMap::new();

        let Some((head_line, mut pos)) = decode_line(buffer, 0)? else {
            return Ok(None);
        };

        while pos < buffer.len() {
            let Some((line, next_pos)) = decode_line(buffer, pos)? else {
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

        buffer.advance(pos);

        Ok(result)
    }
}

enum Interface {
    V4 {
        address: Ipv4Addr,
        index: u32,
        multicast: bool,
    },
    V6 {
        address: Ipv6Addr,
        index: u32,
        multicast: bool,
    },
}

impl Interface {
    fn is_ipv4(&self) -> bool {
        matches!(self, Interface::V4 { .. })
    }

    fn index(&self) -> u32 {
        match self {
            Interface::V4 { index, .. } => *index,
            Interface::V6 { index, .. } => *index,
        }
    }

    fn multicast(&self) -> bool {
        match self {
            Interface::V4 { multicast, .. } => *multicast,
            Interface::V6 { multicast, .. } => *multicast,
        }
    }

    fn address(&self) -> IpAddr {
        match self {
            Interface::V4 { address, .. } => IpAddr::V4(*address),
            Interface::V6 { address, .. } => IpAddr::V6(*address),
        }
    }

    fn build_unicast_socket(&self) -> Result<UdpSocket, io::Error> {
        let raw_socket = if self.is_ipv4() {
            let raw_socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
            raw_socket.set_nonblocking(true)?;
            raw_socket.bind(&SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)).into())?;
            raw_socket
        } else {
            let raw_socket = Socket::new(Domain::IPV6, Type::DGRAM, Some(Protocol::UDP))?;
            raw_socket.set_only_v6(true)?;
            raw_socket.set_nonblocking(true)?;
            raw_socket.bind(&SocketAddr::from((Ipv6Addr::UNSPECIFIED, 0)).into())?;
            raw_socket
        };

        UdpSocket::from_std(raw_socket.into())
    }

    fn build_announce_socket(&self) -> Result<(UdpSocket, SocketAddr), io::Error> {
        let raw_socket = Socket::new(
            if self.is_ipv4() {
                Domain::IPV4
            } else {
                Domain::IPV6
            },
            Type::DGRAM,
            Some(Protocol::UDP),
        )?;

        match self {
            Interface::V4 { address, .. } => {
                raw_socket.set_multicast_if_v4(address)?;
            }
            Interface::V6 { index, .. } => {
                raw_socket.set_only_v6(true)?;
                raw_socket.set_multicast_if_v6(*index)?
            }
        }

        raw_socket.set_nonblocking(true)?;
        raw_socket.bind(&SocketAddr::from((self.address(), 0)).into())?;

        Ok((
            UdpSocket::from_std(raw_socket.into())?,
            if self.is_ipv4() {
                (SSDP_IPV4, SSDP_PORT).into()
            } else {
                (SSDP_IPV6, SSDP_PORT).into()
            },
        ))
    }

    fn from(iface: getifaddrs::Interface) -> Option<Self> {
        let multicast = iface.flags.contains(InterfaceFlags::MULTICAST);

        match (iface.address, iface.index) {
            (IpAddr::V4(address), Some(index)) => Some(Interface::V4 {
                address,
                index,
                multicast,
            }),
            (IpAddr::V6(address), Some(index)) => Some(Interface::V6 {
                address,
                index,
                multicast,
            }),
            _ => None,
        }
    }
}

fn get_interfaces() -> Vec<Interface> {
    match getifaddrs() {
        Ok(ifaces) => ifaces.filter_map(Interface::from).collect(),
        Err(e) => {
            warn!(error=%e, "Failed to enumerate interfaces");
            Vec::new()
        }
    }
}

async fn send_to(socket: &UdpSocket, address: SocketAddr, mut data: &[u8]) -> anyhow::Result<()> {
    while !data.is_empty() {
        let len = socket.send_to(data, address).await?;
        data = &data[len..];
    }

    Ok(())
}

#[pin_project]
struct NotifyStream<'a> {
    #[pin]
    notifier: Notified<'a>,
}

impl<'a> NotifyStream<'a> {
    fn new(notifier: &'a Arc<Notify>) -> Self {
        Self {
            notifier: notifier.notified(),
        }
    }
}

impl Stream for NotifyStream<'_> {
    type Item = bool;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.project().notifier.poll(cx) {
            Poll::Pending => Poll::Ready(Some(false)),
            Poll::Ready(()) => Poll::Ready(Some(true)),
        }
    }
}

#[derive(Clone)]
struct SsdpTask {
    uuid: Uuid,
    server: String,
    http_port: u16,
}

impl SsdpTask {
    fn new(uuid: Uuid, server_version: &str, http_port: u16) -> Self {
        Self {
            uuid,
            server: format!("{}/0.0 UPnP/1.1 {}", consts::OS, server_version),
            http_port,
        }
    }

    async fn send_messages(
        socket: &UdpSocket,
        local_interface: &Interface,
        target: SocketAddr,
        messages: Vec<SsdpMessage>,
    ) {
        let mut buffer = BytesMut::new();
        for message in messages {
            trace!(
                ?message,
                local_address = %local_interface.address(),
                remote_address = %target,
                "Sending SSDP message"
            );

            message.encode(&mut buffer);
            let packet = buffer.split();

            if let Err(e) = send_to(socket, target, &packet).await {
                error!(error=%e, local_address = %local_interface.address(), remote_address=%target, "Failed to send unicast message");
            }
        }
    }

    fn notify_message(
        &self,
        interface: &Interface,
        usn: &str,
        notification_type: &str,
    ) -> SsdpMessage {
        let host = if interface.is_ipv4() {
            SSDP_IPV4.to_string()
        } else {
            SSDP_IPV6.to_string()
        };

        SsdpMessage::Notify {
            host,
            notification_type: notification_type.to_owned(),
            unique_service_name: usn.to_owned(),
            availability: "ssdp:alive".to_string(),
            location: Some(format!(
                "http://{}:{}/upnp/device.xml",
                interface.address(),
                self.http_port
            )),
            server: self.server.clone(),
        }
    }

    async fn announce(&self, interface: &Interface) {
        let (socket, target) = match interface.build_announce_socket() {
            Ok(s) => s,
            Err(e) => {
                match e.kind() {
                    ErrorKind::AddrNotAvailable | ErrorKind::InvalidInput => {}
                    _ => {
                        warn!(error=%e, kind=?e.kind(), local_address=%interface.address(), "Failed to connect SSDP announcement socket");
                    }
                }

                return;
            }
        };

        let usn_base = format!("uuid:{}", self.uuid.as_hyphenated());

        let messages = vec![
            self.notify_message(interface, &usn_base, &usn_base),
            self.notify_message(
                interface,
                &format!("{}::{}", usn_base, ns::UPNP_ROOT),
                ns::UPNP_ROOT,
            ),
            self.notify_message(
                interface,
                &format!("{}::{}", usn_base, ns::UPNP_MEDIASERVER),
                ns::UPNP_MEDIASERVER,
            ),
            self.notify_message(
                interface,
                &format!("{}::{}", usn_base, ns::UPNP_CONTENTDIRECTORY),
                ns::UPNP_CONTENTDIRECTORY,
            ),
        ];

        debug!(local_address = %interface.address(), count = messages.len(), "Sending SSDP announcements");

        Self::send_messages(&socket, interface, target, messages).await;
    }

    async fn announce_task(self) {
        loop {
            for interface in get_interfaces() {
                if !interface.multicast() {
                    continue;
                }

                self.announce(&interface).await
            }

            time::sleep(Duration::from_secs(60)).await;
        }
    }

    fn response_message(
        &self,
        local_interface: &Interface,
        usn: &str,
        search_target: &str,
    ) -> SsdpMessage {
        SsdpMessage::SearchResponse {
            location: format!(
                "http://{}:{}/upnp/device.xml",
                local_interface.address(),
                self.http_port
            ),
            server: self.server.clone(),
            search_target: search_target.to_owned(),
            unique_service_name: usn.to_owned(),
        }
    }

    async fn send_search_response(
        &self,
        local_interface: &Interface,
        search_target: &str,
        address: SocketAddr,
    ) {
        let usn_base = format!("uuid:{}", self.uuid.as_hyphenated());
        let mut messages = Vec::new();

        match search_target {
            ns::UPNP_ROOT => {
                messages.push(self.response_message(local_interface, &usn_base, &usn_base));
                messages.push(self.response_message(
                    local_interface,
                    &format!("{}::{}", usn_base, ns::UPNP_ROOT),
                    ns::UPNP_ROOT,
                ));
                messages.push(self.response_message(
                    local_interface,
                    &format!("{}::{}", usn_base, ns::UPNP_MEDIASERVER),
                    ns::UPNP_MEDIASERVER,
                ));
                messages.push(self.response_message(
                    local_interface,
                    &format!("{}::{}", usn_base, ns::UPNP_CONTENTDIRECTORY),
                    ns::UPNP_CONTENTDIRECTORY,
                ));
            }
            ns::UPNP_MEDIASERVER | ns::UPNP_CONTENTDIRECTORY => {
                messages.push(self.response_message(
                    local_interface,
                    &format!("{}::{}", usn_base, search_target),
                    search_target,
                ));
            }
            _ => {}
        }

        if search_target.to_lowercase() == usn_base {
            messages.push(self.response_message(local_interface, &usn_base, &usn_base));
        }

        if messages.is_empty() {
            return;
        }

        debug!(local_address = %local_interface.address(), remote_address = %address, count = messages.len(), "Sending notification responses");

        let socket = match local_interface.build_unicast_socket() {
            Ok(s) => s,
            Err(e) => {
                error!(error=%e, local_address=%local_interface.address(), "Failed to build unicast socket");
                return;
            }
        };

        Self::send_messages(&socket, local_interface, address, messages).await;
    }

    fn build_recv_socket(is_ipv4: bool) -> Result<PktInfoUdpSocket, io::Error> {
        let socket = PktInfoUdpSocket::new(if is_ipv4 { Domain::IPV4 } else { Domain::IPV6 })?;
        socket.set_nonblocking(true)?;
        socket.set_reuse_address(true)?;
        socket.set_reuse_port(true)?;

        let interfaces = get_interfaces();

        if is_ipv4 {
            socket.bind(&SocketAddr::from((Ipv4Addr::UNSPECIFIED, SSDP_PORT)).into())?;

            for interface in interfaces {
                if let IpAddr::V4(ipaddr) = interface.address() {
                    socket.join_multicast_v4(&SSDP_IPV4, &ipaddr)?;
                }
            }

            socket.set_multicast_loop_v4(false)?;
        } else {
            socket.bind(&SocketAddr::from((Ipv6Addr::UNSPECIFIED, SSDP_PORT)).into())?;

            for interface in interfaces {
                if !interface.is_ipv4() {
                    socket.join_multicast_v6(&SSDP_IPV6, interface.index())?;
                }
            }

            socket.set_multicast_loop_v6(false)?;
        }

        Ok(socket)
    }

    async fn receive_task(self, is_ipv4: bool, restart_notifier: Arc<Notify>) {
        loop {
            let socket = match Self::build_recv_socket(is_ipv4) {
                Ok(s) => s,
                Err(e) => {
                    match e.kind() {
                        ErrorKind::AddrNotAvailable | ErrorKind::InvalidInput => {
                            // IP version not supported.
                            return;
                        }
                        _ => {
                            error!(error=%e, kind=?e.kind(), is_ipv4, "Failed to build multicast receiver socket");
                            time::sleep(Duration::from_secs(10)).await;

                            continue;
                        }
                    }
                }
            };

            let mut buffers: HashMap<SocketAddr, BytesMut> = HashMap::new();
            let mut receive_buffer = [0_u8; 4096];

            let mut notified = pin!(NotifyStream::new(&restart_notifier));

            loop {
                if let Some(true) = notified.next().await {
                    break;
                }

                let (len, info) = match socket.recv(&mut receive_buffer) {
                    Ok(p) => p,
                    Err(e) => {
                        if matches!(
                            e.kind(),
                            ErrorKind::WouldBlock
                                | ErrorKind::Interrupted
                                | ErrorKind::ResourceBusy
                        ) {
                            time::sleep(Duration::from_millis(100)).await;

                            continue;
                        }

                        warn!(error=%e, kind=?e.kind(), "Socket receive error. Terminating thread.");
                        return;
                    }
                };

                if len == 0 {
                    continue;
                }

                let Some(local_interface) = get_interfaces().into_iter().find(|iface| {
                    iface.is_ipv4() == info.addr_src.is_ipv4()
                        && iface.index() as u64 == info.if_index
                }) else {
                    warn!(remote_address = %info.addr_src, "Received SSDP packet on unknown local interface");
                    continue;
                };

                let buffer = buffers.entry(info.addr_src).or_default();
                buffer.extend_from_slice(&receive_buffer[0..len]);

                while !buffer.is_empty() {
                    match SsdpMessage::decode(buffer) {
                        Ok(Some(message)) => {
                            if message.is_uuid(self.uuid) {
                                continue;
                            }

                            if let SsdpMessage::MSearch { search_target, .. } = &message {
                                debug!(?message, local_address = %local_interface.address(), remote_address = %info.addr_src, "Received SSDP message");

                                self.send_search_response(
                                    &local_interface,
                                    search_target,
                                    info.addr_src,
                                )
                                .await;
                            } else {
                                trace!(?message, local_address = %local_interface.address(), remote_address = %info.addr_src, "Received SSDP message");
                            }
                        }
                        Ok(None) => {
                            // Not enough data
                            break;
                        }
                        Err(e) => {
                            warn!(error=%e, remote_address = %info.addr_src, "Failed to decode SSDP packet");
                            break;
                        }
                    }
                }
            }

            info!("Restarting SSDP listener.");
            drop(socket);
            time::sleep(Duration::from_secs(1)).await;
        }
    }
}

pub(crate) struct Ssdp {
    announce: TaskHandle,
    ipv4_handle: TaskHandle,
    ipv6_handle: TaskHandle,
    restart_notify: Arc<Notify>,
}

impl Ssdp {
    pub(crate) fn new(uuid: Uuid, server_version: &str, http_port: u16) -> Self {
        let task = SsdpTask::new(uuid, server_version, http_port);
        let restart_notify = Arc::new(Notify::new());

        Self {
            announce: rt::spawn(task.clone().announce_task()),
            ipv4_handle: rt::spawn(task.clone().receive_task(true, restart_notify.clone())),
            ipv6_handle: rt::spawn(task.receive_task(false, restart_notify.clone())),
            restart_notify,
        }
    }

    pub(crate) fn restart(&self) {
        self.restart_notify.notify_waiters();
    }

    pub(crate) async fn shutdown(self) {
        self.announce.shutdown().await;
        self.ipv4_handle.shutdown().await;
        self.ipv6_handle.shutdown().await;
    }
}
