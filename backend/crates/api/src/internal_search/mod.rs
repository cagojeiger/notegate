//! Private HTTP boundary between the public API process and search execution.

mod auth;
mod client;
mod contract;
mod server;

pub(crate) use client::{SearchClient, SearchClientError};
pub(crate) use server::{SearchServerState, routes};

pub(crate) const FIND_PATH: &str = "/internal/v1/search/find";
pub(crate) const GREP_PATH: &str = "/internal/v1/search/grep";

pub(crate) fn loopback_base_url(bind_addr: std::net::SocketAddr) -> String {
    let ip = match bind_addr.ip() {
        std::net::IpAddr::V4(ip) if ip.is_unspecified() => std::net::Ipv4Addr::LOCALHOST.into(),
        std::net::IpAddr::V6(ip) if ip.is_unspecified() => std::net::Ipv6Addr::LOCALHOST.into(),
        ip => ip,
    };
    format!("http://{}", std::net::SocketAddr::new(ip, bind_addr.port()))
}

#[cfg(test)]
mod tests;
