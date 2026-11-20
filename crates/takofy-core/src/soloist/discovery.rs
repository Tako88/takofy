//! Reading the address Soloist wrote beside its data directory.
//!
//! The port is the only field trusted. A host that is not loopback is ignored,
//! so a tampered `ws.addr` can never send this client off the machine.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

/// The endpoint named by `ws.addr` and `ws.port`.
///
/// The port must be a decimal after trimming. The host is accepted only when it
/// parses as a loopback address; otherwise loopback is used.
pub fn endpoint(addr_text: &str, port_text: &str) -> Option<SocketAddr> {
    let port: u16 = port_text.trim().parse().ok()?;
    let host = host_of(addr_text)
        .filter(|ip| ip.is_loopback())
        .unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST));
    Some(SocketAddr::new(host, port))
}

fn host_of(addr_text: &str) -> Option<IpAddr> {
    let text = addr_text.trim();
    if let Ok(socket) = text.parse::<SocketAddr>() {
        return Some(socket.ip());
    }
    if let Ok(ip) = text.parse::<IpAddr>() {
        return Some(ip);
    }
    let (host, _) = text.rsplit_once(':')?;
    let host = host.trim_start_matches('[').trim_end_matches(']');
    host.parse::<IpAddr>().ok()
}

#[cfg(test)]
mod tests;
