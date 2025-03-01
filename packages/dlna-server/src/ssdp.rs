use std::{
    fmt,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    str::{FromStr, from_utf8},
};

use anyhow::bail;
use bytes::{Buf, BytesMut};
use futures::stream::StreamExt;
use http::{HeaderMap, HeaderName, HeaderValue};
use tokio::net::UdpSocket;
use tokio_util::{
    codec::{Decoder, Encoder},
    udp::UdpFramed,
};
use tracing::{debug, error, instrument, trace, warn};

const SSDP_IPV4: Ipv4Addr = Ipv4Addr::new(239, 255, 255, 250);
const SSDP_IPV6: Ipv6Addr = Ipv6Addr::new(0xFF02, 0, 0, 0, 0, 0, 0, 0xC);

#[derive(Debug, Clone)]
enum SsdpMessage {
    MSearch {
        path: String,
        host: String,
        method: String,
        search_targets: Vec<String>,
        max_wait: Option<u64>,
        user_agent: Option<String>,
    },
    Notify {
        path: String,
        host: String,
        notification_type: String,
        unique_service_name: String,
        availability: String,
        location: String,
        server: Option<String>,
    },
    ByeBye {
        path: String,
        host: String,
        notification_type: String,
        unique_service_name: String,
        availability: String,
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

impl SsdpMessage {
    #[instrument(skip(headers))]
    fn parse_m_search(path: &str, headers: HeaderMap<HeaderValue>) -> Option<Self> {
        let Some(host) = get_header::<String>(&headers, "host") else {
            warn!("Missing host header");
            return None;
        };

        let Some(method) = get_header::<String>(&headers, "man") else {
            warn!("Missing man header");
            return None;
        };

        let Some(search_target_list) = get_header::<String>(&headers, "st") else {
            warn!("Missing st header");
            return None;
        };

        let max_wait = get_header::<u64>(&headers, "mx");

        let user_agent = get_header::<String>(&headers, "user-agent");

        Some(Self::MSearch {
            path: path.to_owned(),
            host,
            method,
            search_targets: search_target_list
                .split(',')
                .map(|st| st.to_owned())
                .collect(),
            max_wait,
            user_agent,
        })
    }

    #[instrument(skip(headers))]
    fn parse_notify(path: &str, headers: HeaderMap<HeaderValue>) -> Option<Self> {
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

        let server = get_header::<String>(&headers, "server");

        Some(Self::Notify {
            path: path.to_owned(),
            host,
            notification_type,
            availability,
            location,
            unique_service_name,
            server,
        })
    }

    #[instrument(skip(headers))]
    fn parse_bye_bye(path: &str, headers: HeaderMap<HeaderValue>) -> Option<Self> {
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

        let Some(unique_service_name) = get_header::<String>(&headers, "usn") else {
            warn!("Missing usn header");
            return None;
        };

        Some(Self::ByeBye {
            path: path.to_owned(),
            host,
            notification_type,
            availability,
            unique_service_name,
        })
    }

    fn parse(method: &str, path: &str, headers: HeaderMap<HeaderValue>) -> Option<Self> {
        match method.to_lowercase().as_str() {
            "m-search" => Self::parse_m_search(path, headers),
            "notify" => Self::parse_notify(path, headers),
            "bye-bye" => Self::parse_bye_bye(path, headers),
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

fn parse_method(line: &str) -> anyhow::Result<(&str, &str)> {
    if !line.ends_with(" HTTP/1.1") {
        bail!("Invalid protocol");
    }

    let line = line[..line.len() - 9].trim();

    let Some(separator) = line.find(' ') else {
        bail!("Missing method separator");
    };

    let method = line[0..separator].trim();
    let path = line[separator + 1..].trim();
    Ok((method, path))
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
        todo!()
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
            Ok((method, path)) => SsdpMessage::parse(method, path, headers).map(|message| {
                trace!(?message, "Received SSDP message");
                message
            }),
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

pub(crate) struct SsdpTask {
    pub(crate) http_port: u16,
    socket: UdpFramed<SsdpCodec>,
}

impl SsdpTask {
    pub(crate) async fn new(
        ipaddr: IpAddr,
        ssdp_port: u16,
        http_port: u16,
    ) -> anyhow::Result<Self> {
        let socket = UdpSocket::bind((ipaddr, ssdp_port)).await?;

        match ipaddr {
            IpAddr::V4(ipv4_addr) => socket.join_multicast_v4(SSDP_IPV4, ipv4_addr)?,
            IpAddr::V6(_) => socket.join_multicast_v6(&SSDP_IPV6, 0)?,
        }

        let framed = UdpFramed::new(socket, SsdpCodec);

        Ok(Self {
            http_port,
            socket: framed,
        })
    }

    pub(crate) async fn run(mut self) {
        loop {
            let (message, socket) = match self.socket.next().await {
                Some(Ok(r)) => r,
                Some(Err(e)) => {
                    error!(error=%e, "Failed to decode SSDP packet");
                    return;
                }
                None => {
                    debug!("Socket closed unexpectedly");
                    return;
                }
            };
        }
    }
}
