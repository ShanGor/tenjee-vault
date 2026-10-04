use crate::error::{VaultError, VaultResult};
use if_addrs::{IfAddr, Interface};
use std::net::{IpAddr, SocketAddr, SocketAddrV6};

pub fn private(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => (ip.is_private() || ip.is_link_local()) && !ip.is_loopback() && !ip.is_unspecified() && !ip.is_multicast() && !ip.is_broadcast(),
        IpAddr::V6(ip) => (ip.is_unique_local() || ip.is_unicast_link_local()) && !ip.is_unspecified() && !ip.is_multicast() && !ip.is_loopback(),
    }
}

pub fn interfaces() -> VaultResult<Vec<Interface>> {
    let interfaces = if_addrs::get_if_addrs()?.into_iter().filter(|interface| {
        let name = interface.name.to_lowercase();
        !interface.is_loopback() && !interface.is_p2p && private(interface.ip()) &&
        !["tun", "tap", "wg", "ppp", "tailscale", "utun", "docker", "veth", "virbr"].iter().any(|prefix| name.starts_with(prefix))
    }).collect::<Vec<_>>();
    if interfaces.is_empty() { return Err(VaultError::Validation("No eligible local network. Connect to the same Wi-Fi/router and check VPN settings.".into())); }
    Ok(interfaces)
}

pub fn same_link(interface: &Interface, peer: SocketAddr) -> bool {
    if !private(peer.ip()) || peer.port() == 0 { return false; }
    match (&interface.addr,peer) {
        (IfAddr::V4(local),SocketAddr::V4(remote)) => {
            let mask = u32::from(local.netmask);
            u32::from(local.ip) & mask == u32::from(*remote.ip()) & mask
        }
        (IfAddr::V6(local),SocketAddr::V6(remote)) => {
            if local.ip.is_unicast_link_local() && remote.scope_id() != interface.index.unwrap_or(0) { return false; }
            let mask = u128::from(local.netmask);
            u128::from(local.ip) & mask == u128::from(*remote.ip()) & mask
        }
        _ => false,
    }
}

pub fn socket(interface: &Interface, port: u16) -> SocketAddr {
    match interface.ip() {
        IpAddr::V4(ip) => SocketAddr::new(IpAddr::V4(ip),port),
        IpAddr::V6(ip) => SocketAddr::V6(SocketAddrV6::new(ip,port,0,if ip.is_unicast_link_local() { interface.index.unwrap_or(0) } else { 0 })),
    }
}

pub fn parse_address(input: &str) -> VaultResult<SocketAddr> {
    let address: SocketAddr = input.parse().map_err(|_| VaultError::Validation("Enter a numeric local IP:port (IPv6: [address%interface-index]:port). Hostnames and URLs are not supported.".into()))?;
    if !interfaces()?.iter().any(|interface| same_link(interface,address)) {
        return Err(VaultError::Validation("Peer address must be on an eligible directly connected private network; Internet and VPN routes are rejected.".into()));
    }
    Ok(address)
}
