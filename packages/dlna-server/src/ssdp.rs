use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use tokio::net::UdpSocket;
use tokio_util::udp::UdpFramed;

const SSDP_IPV4: Ipv4Addr = Ipv4Addr::new(239, 255, 255, 250);
const SSDP_IPV6: Ipv6Addr = Ipv6Addr::new(0xFF02, 0, 0, 0, 0, 0, 0, 0xC);

#[derive(Default)]
struct SsdpCodec {}

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

        let framed = UdpFramed::new(socket, SsdpCodec::default());

        Ok(Self {
            http_port,
            socket: framed,
        })
    }

    pub(crate) async fn run(self) {}
}
