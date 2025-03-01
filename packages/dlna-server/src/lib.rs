#![deny(unreachable_pub)]
//! A basic implementation of a DLNA media server

use core::net::Ipv4Addr;
use std::net::IpAddr;

use actix_web::{App, HttpServer, dev::ServerHandle};

use crate::{
    rt::{TaskHandle, spawn},
    ssdp::SsdpTask,
};

/// The default port to use for SSDP communication
pub const SSDP_PORT: u16 = 1900;
/// The default port to use for HTTP communication
pub const HTTP_PORT: u16 = 1980;

mod cds;
#[cfg_attr(feature = "rt-async", path = "rt/async_std.rs")]
#[cfg_attr(feature = "rt-tokio", path = "rt/tokio.rs")]
mod rt;
mod ssdp;

#[cfg(not(any(feature = "rt-tokio", feature = "rt-async")))]
compile_error!("An async runtime must be selected");

#[derive(Default)]
struct TaskHandles {
    handles: Vec<TaskHandle>,
}

impl TaskHandles {
    fn add(&mut self, handle: TaskHandle) {
        self.handles.push(handle);
    }

    async fn shutdown(self) {
        for handle in self.handles {
            handle.shutdown().await;
        }
    }
}

/// A handle to the DLNA server allowing for discovering clients and shutting
/// the server down.
pub struct DlnaServer {
    task_handles: TaskHandles,
    web_handle: ServerHandle,
}

impl DlnaServer {
    /// Builds a default DLNA server listing on all interfaces on IPv4 and
    /// starts it listening using the chosen runtime.
    pub async fn new() -> anyhow::Result<Self> {
        Self::builder()
            .bind(Ipv4Addr::UNSPECIFIED, SSDP_PORT, HTTP_PORT)
            .build()
            .await
    }

    pub fn builder() -> DlnaServerBuilder {
        DlnaServerBuilder::default()
    }

    pub async fn shutdown(self) {
        self.task_handles.shutdown().await;
        self.web_handle.stop(true).await;
    }
}

#[derive(Default)]
/// A builder allowing configuration of the DLNA server.
pub struct DlnaServerBuilder {
    binds: Vec<(IpAddr, u16, u16)>,
}

impl DlnaServerBuilder {
    /// Builds the DLNA server and starts it listening using the chosen runtime.
    pub async fn build(self) -> anyhow::Result<DlnaServer> {
        let mut http_server = HttpServer::new(App::new);

        let mut ssdp: Vec<SsdpTask> = Vec::new();
        for (ipaddr, ssdp_port, http_port) in self.binds {
            ssdp.push(SsdpTask::new(ipaddr, ssdp_port, http_port).await?);

            http_server = http_server.bind((ipaddr, http_port))?;
        }

        let server = http_server.run();
        let web_handle = server.handle();

        spawn(server);

        let mut task_handles = TaskHandles::default();
        for ssdp_task in ssdp {
            task_handles.add(spawn(ssdp_task.run()));
        }

        Ok(DlnaServer {
            task_handles,
            web_handle,
        })
    }

    /// Binds to an IP address. You must give both the SSDP (UDP) port and HTTP (tcp) ports to
    /// use for this address.
    /// This can be called multiple times to bind to different addresses, most commonly to bind
    /// to IPv4 and IPv6 addresses.
    pub fn bind<A: Into<IpAddr>>(mut self, addr: A, ssdp_port: u16, http_port: u16) -> Self {
        self.binds.push((addr.into(), ssdp_port, http_port));
        self
    }
}
