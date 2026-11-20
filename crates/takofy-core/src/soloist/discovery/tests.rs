use super::endpoint;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

fn loopback(port: u16) -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)
}

#[test]
fn a_bare_ip_and_a_port_make_the_endpoint() {
    assert_eq!(endpoint("127.0.0.1", "54321"), Some(loopback(54321)));
    assert_eq!(endpoint("127.0.0.1\n", " 54321 \n"), Some(loopback(54321)));
}

#[test]
fn an_address_that_already_carries_the_port_is_read_too() {
    assert_eq!(endpoint("127.0.0.1:4444", "4444"), Some(loopback(4444)));
    assert_eq!(
        endpoint("[::1]:4444", "4444"),
        Some(SocketAddr::new("::1".parse().expect("literal"), 4444))
    );
}

#[test]
fn a_garbage_host_falls_back_to_loopback() {
    assert_eq!(endpoint("nonsense", "9000"), Some(loopback(9000)));
    assert_eq!(endpoint("", "9000"), Some(loopback(9000)));
}

#[test]
fn a_non_loopback_host_is_never_connected_to() {
    assert_eq!(endpoint("10.1.2.3:80", "9000"), Some(loopback(9000)));
    assert_eq!(endpoint("example.invalid", "9000"), Some(loopback(9000)));
}

#[test]
fn no_usable_port_is_no_endpoint() {
    assert_eq!(endpoint("127.0.0.1", ""), None);
    assert_eq!(endpoint("127.0.0.1", "not-a-port"), None);
    assert_eq!(endpoint("127.0.0.1", "70000"), None);
}
