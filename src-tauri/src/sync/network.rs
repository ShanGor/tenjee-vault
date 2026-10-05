use crate::error::{VaultError, VaultResult};
use if_addrs::{IfAddr, Interface};
use serde::Serialize;
use socket2::{Domain, Protocol, Socket, Type};
use std::collections::BTreeMap;
use std::net::{IpAddr, SocketAddr, SocketAddrV6, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};

fn invalid(message: &str) -> VaultError {
    VaultError::Validation(message.into())
}

pub fn private(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            (ip.is_private() || ip.is_link_local())
                && !ip.is_loopback()
                && !ip.is_unspecified()
                && !ip.is_multicast()
                && !ip.is_broadcast()
        }
        IpAddr::V6(ip) => {
            (ip.is_unique_local() || ip.is_unicast_link_local())
                && !ip.is_unspecified()
                && !ip.is_multicast()
                && !ip.is_loopback()
        }
    }
}

fn shared(ip: IpAddr) -> bool {
    matches!(ip, IpAddr::V4(ip) if u32::from(ip) & 0xffc0_0000 == 0x6440_0000) // 100.64.0.0/10, including Tailscale IPv4.
}

pub fn vpn(interface: &Interface) -> bool {
    let name = interface.name.to_lowercase();
    interface.is_p2p
        || shared(interface.ip())
        || ["tun", "tap", "wg", "ppp", "utun"]
            .iter()
            .any(|prefix| name.starts_with(prefix))
        || ["tailscale", "wireguard", "openvpn", "zerotier", "vpn"]
            .iter()
            .any(|part| name.contains(part))
}

fn eligible(interface: &Interface) -> bool {
    let name = interface.name.to_lowercase();
    !interface.is_loopback()
        && (private(interface.ip()) || shared(interface.ip()))
        && !["docker", "veth", "virbr"]
            .iter()
            .any(|prefix| name.starts_with(prefix))
}

#[derive(Serialize)]
pub struct NetworkOption {
    pub name: String,
    pub vpn: bool,
    pub addresses: Vec<String>,
}

pub fn options() -> VaultResult<Vec<NetworkOption>> {
    let mut options = BTreeMap::<String, NetworkOption>::new();
    for interface in if_addrs::get_if_addrs()?.into_iter().filter(eligible) {
        let option = options
            .entry(interface.name.clone())
            .or_insert_with(|| NetworkOption {
                name: interface.name.clone(),
                vpn: false,
                addresses: Vec::new(),
            });
        option.vpn |= vpn(&interface);
        option.addresses.push(interface.ip().to_string());
    }
    Ok(options.into_values().collect())
}

/// Default mode retains same-link LAN restrictions. An explicitly selected interface
/// permits routed private/overlay peers, with sockets bound to that interface's IP.
pub struct Network {
    interfaces: Vec<Interface>,
    routed: bool,
}

impl Network {
    pub fn select(name: Option<&str>) -> VaultResult<Self> {
        Self::from_interfaces(if_addrs::get_if_addrs()?, name)
    }

    fn from_interfaces(interfaces: Vec<Interface>, name: Option<&str>) -> VaultResult<Self> {
        let mut interfaces: Vec<_> = interfaces.into_iter().filter(eligible).collect();
        // Classify the entire adapter, including its IPv6 addresses.
        let vpn_names: Vec<_> = interfaces
            .iter()
            .filter(|i| vpn(i))
            .map(|i| i.name.clone())
            .collect();
        interfaces.retain(|i| match name {
            Some(name) => i.name == name,
            None => private(i.ip()) && !vpn_names.contains(&i.name),
        });
        if interfaces.is_empty() {
            return Err(invalid("No eligible network. Connect to Wi-Fi or your VPN, then refresh the network list and select an interface."));
        }
        Ok(Self {
            interfaces,
            routed: name.is_some(),
        })
    }

    pub fn discovery_interfaces(&self) -> Vec<&Interface> {
        let vpn_names: Vec<_> = self
            .interfaces
            .iter()
            .filter(|i| vpn(i))
            .map(|i| i.name.clone())
            .collect();
        self.interfaces
            .iter()
            .filter(|i| !vpn_names.contains(&i.name))
            .collect()
    }

    fn allows_on(&self, interface: &Interface, peer: SocketAddr) -> bool {
        if peer.port() == 0
            || !(private(peer.ip()) || (self.routed && shared(peer.ip())))
            || interface.ip().is_ipv4() != peer.ip().is_ipv4()
        {
            return false;
        }
        let link_local = match peer.ip() {
            IpAddr::V4(ip) => ip.is_link_local(),
            IpAddr::V6(ip) => ip.is_unicast_link_local(),
        };
        if link_local {
            return same_link(interface, peer);
        }
        if let SocketAddr::V6(peer) = peer {
            if peer.scope_id() != 0 {
                return false;
            }
        }
        self.routed || same_link(interface, peer)
    }

    pub fn allows(&self, peer: SocketAddr) -> bool {
        self.interfaces.iter().any(|i| self.allows_on(i, peer))
    }

    pub fn allows_incoming(&self, local: SocketAddr, peer: SocketAddr) -> bool {
        self.interfaces
            .iter()
            .any(|i| socket(i, local.port()) == local && self.allows_on(i, peer))
    }

    pub fn listen(&mut self, port: u16) -> VaultResult<Vec<TcpListener>> {
        let mut listeners = Vec::new();
        let mut bound = Vec::new();
        for interface in &self.interfaces {
            let address = socket(interface, port);
            match TcpListener::bind(address) {
                Ok(listener) => { listener.set_nonblocking(true)?; listeners.push(listener); bound.push(interface.clone()); }
                Err(_) if port != 0 => return Err(invalid(&format!("Cannot listen on {address}. The port may be in use; choose another exchange port or leave it blank for automatic allocation."))),
                Err(_) => {},
            }
        }
        if listeners.is_empty() {
            return Err(invalid("Cannot listen on the selected network; check network permission and firewall settings."));
        }
        self.interfaces = bound;
        Ok(listeners)
    }

    fn candidates(&self, addresses: Vec<SocketAddr>) -> VaultResult<Vec<SocketAddr>> {
        let mut candidates = Vec::new();
        for address in addresses {
            if self.allows(address) && !candidates.contains(&address) {
                candidates.push(address);
            }
            if candidates.len() == 16 {
                break;
            }
        }
        if candidates.is_empty() {
            return Err(invalid("The address has no eligible peer IP. Use a local hostname/IP, or select your VPN interface for routed private or Tailscale addresses. Public Internet addresses are not supported."));
        }
        // Alternate families so an unreachable IPv6 route cannot exhaust the budget
        // before trying an IPv4 result (or vice versa).
        let first_v4 = candidates[0].is_ipv4();
        let (mut first, mut second): (Vec<_>, Vec<_>) = candidates
            .into_iter()
            .partition(|a| a.is_ipv4() == first_v4);
        first.reverse();
        second.reverse();
        let mut candidates = Vec::new();
        while !first.is_empty() || !second.is_empty() {
            if let Some(address) = first.pop() {
                candidates.push(address);
            }
            if let Some(address) = second.pop() {
                candidates.push(address);
            }
        }
        Ok(candidates)
    }

    pub fn resolve(
        &self,
        input: &str,
        check: impl Fn() -> VaultResult<()>,
    ) -> VaultResult<Vec<SocketAddr>> {
        check()?;
        let addresses = match endpoint(input)? {
            Endpoint::Address(address) => vec![address],
            Endpoint::Hostname(host, port) => lookup(host, port, &check)?,
        };
        check()?;
        self.candidates(addresses)
    }

    pub fn connect(
        &self,
        addresses: &[SocketAddr],
        check: impl Fn() -> VaultResult<()>,
    ) -> VaultResult<TcpStream> {
        let candidates = addresses.iter().flat_map(|address| {
            self.interfaces
                .iter()
                .filter(move |i| self.allows_on(i, *address))
                .map(move |i| (socket(i, 0), *address))
        });
        connect_candidates(candidates, check)
    }
}

fn connect_candidates(
    candidates: impl Iterator<Item = (SocketAddr, SocketAddr)>,
    check: impl Fn() -> VaultResult<()>,
) -> VaultResult<TcpStream> {
    let deadline = Instant::now() + Duration::from_secs(10);
    for (local, peer) in candidates {
        check()?;
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(invalid("Connection timed out. Check that the peer is in exchange mode, the port is correct, and firewall/VPN access rules permit device-to-device connections."));
        }
        if let Ok(stream) = connect_socket(local, peer, remaining.min(Duration::from_secs(2))) {
            check()?;
            return Ok(stream);
        }
    }
    Err(invalid("Cannot connect on the selected network. Check the hostname/IP and port, start exchange on the peer, and check firewall, VPN access rules, and client isolation."))
}

fn connect_socket(
    local: SocketAddr,
    peer: SocketAddr,
    timeout: Duration,
) -> std::io::Result<TcpStream> {
    let socket = Socket::new(
        if peer.is_ipv4() {
            Domain::IPV4
        } else {
            Domain::IPV6
        },
        Type::STREAM,
        Some(Protocol::TCP),
    )?;
    socket.bind(&local.into())?;
    socket.connect_timeout(&peer.into(), timeout)?;
    Ok(socket.into())
}

pub fn same_link(interface: &Interface, peer: SocketAddr) -> bool {
    if !private(peer.ip()) || peer.port() == 0 {
        return false;
    }
    match (&interface.addr, peer) {
        (IfAddr::V4(local), SocketAddr::V4(remote)) => {
            let mask = u32::from(local.netmask);
            u32::from(local.ip) & mask == u32::from(*remote.ip()) & mask
        }
        (IfAddr::V6(local), SocketAddr::V6(remote)) => {
            if local.ip.is_unicast_link_local() || remote.ip().is_unicast_link_local() {
                if remote.scope_id() == 0 || remote.scope_id() != interface.index.unwrap_or(0) {
                    return false;
                }
            }
            let mask = u128::from(local.netmask);
            u128::from(local.ip) & mask == u128::from(*remote.ip()) & mask
        }
        _ => false,
    }
}

pub fn socket(interface: &Interface, port: u16) -> SocketAddr {
    match interface.ip() {
        IpAddr::V4(ip) => SocketAddr::new(IpAddr::V4(ip), port),
        IpAddr::V6(ip) => SocketAddr::V6(SocketAddrV6::new(
            ip,
            port,
            0,
            if ip.is_unicast_link_local() {
                interface.index.unwrap_or(0)
            } else {
                0
            },
        )),
    }
}

#[derive(Debug, PartialEq)]
enum Endpoint {
    Address(SocketAddr),
    Hostname(String, u16),
}

fn endpoint(input: &str) -> VaultResult<Endpoint> {
    let input = input.trim();
    if let Ok(address) = input.parse::<SocketAddr>() {
        if address.port() == 0 {
            return Err(invalid("The peer port must be between 1 and 65535."));
        }
        return Ok(Endpoint::Address(address));
    }
    let message = "Enter hostname:port or IP:port (IPv6: [address%interface-index]:port). URLs are not supported.";
    let (host, port) = input.rsplit_once(':').ok_or_else(|| invalid(message))?;
    let port = if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) {
        port.parse::<u16>().ok().filter(|p| *p != 0)
    } else {
        None
    }
    .ok_or_else(|| invalid("The peer port must be between 1 and 65535."))?;
    let name = host.strip_suffix('.').unwrap_or(host);
    if name.is_empty()
        || name.len() > 253
        || name.bytes().all(|b| b.is_ascii_digit() || b == b'.')
        || !name.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label.starts_with(|c: char| c.is_ascii_alphanumeric())
                && label.ends_with(|c: char| c.is_ascii_alphanumeric())
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
    {
        return Err(invalid(message));
    }
    Ok(Endpoint::Hostname(host.into(), port))
}

// OS DNS cannot be interrupted portably. Bound waiting and permit only one OS
// lookup at a time; a late answer has no authority to open a socket after Stop.
static RESOLVING: AtomicBool = AtomicBool::new(false);
struct ResolveGuard;
impl Drop for ResolveGuard {
    fn drop(&mut self) {
        RESOLVING.store(false, Ordering::SeqCst);
    }
}

fn lookup(
    host: String,
    port: u16,
    check: &impl Fn() -> VaultResult<()>,
) -> VaultResult<Vec<SocketAddr>> {
    if RESOLVING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err(invalid("A previous hostname lookup is still finishing. Try again shortly or use the peer IP and port."));
    }
    let guard = ResolveGuard;
    let (sender, receiver) = mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("exchange-dns".into())
        .spawn(move || {
            let result = (host.as_str(), port)
                .to_socket_addrs()
                .map(|addresses| addresses.take(64).collect::<Vec<_>>());
            drop(guard);
            let _ = sender.send(result);
        })?;
    wait_lookup(receiver, check)
}

fn wait_lookup(
    receiver: mpsc::Receiver<std::io::Result<Vec<SocketAddr>>>,
    check: &impl Fn() -> VaultResult<()>,
) -> VaultResult<Vec<SocketAddr>> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        check()?;
        if Instant::now() >= deadline {
            return Err(invalid(
                "Hostname lookup timed out. Check DNS/MagicDNS or use the peer IP and port.",
            ));
        }
        match receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(Ok(addresses)) => return Ok(addresses),
            Ok(Err(_)) | Err(mpsc::RecvTimeoutError::Disconnected) => return Err(invalid("Cannot resolve the hostname. Check its spelling and DNS/MagicDNS settings, or use the peer IP and port.")),
            Err(mpsc::RecvTimeoutError::Timeout) => {},
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use if_addrs::{IfOperStatus, Ifv4Addr, Ifv6Addr};

    fn interface(name: &str, ip: &str, mask: &str, index: u32, p2p: bool) -> Interface {
        let addr = match (
            ip.parse::<IpAddr>().unwrap(),
            mask.parse::<IpAddr>().unwrap(),
        ) {
            (IpAddr::V4(ip), IpAddr::V4(netmask)) => IfAddr::V4(Ifv4Addr {
                ip,
                netmask,
                prefixlen: netmask.to_bits().count_ones() as u8,
                broadcast: None,
            }),
            (IpAddr::V6(ip), IpAddr::V6(netmask)) => IfAddr::V6(Ifv6Addr {
                ip,
                netmask,
                prefixlen: netmask.to_bits().count_ones() as u8,
                broadcast: None,
            }),
            _ => unreachable!(),
        };
        Interface {
            name: name.into(),
            addr,
            index: Some(index),
            oper_status: IfOperStatus::Up,
            is_p2p: p2p,
            #[cfg(windows)]
            adapter_name: name.into(),
        }
    }

    fn fixture() -> Vec<Interface> {
        vec![
            interface("wlan0", "192.168.1.10", "255.255.255.0", 1, false),
            interface("wlan0", "fd11::10", "ffff:ffff:ffff:ffff::", 1, false),
            interface("wlan0", "fe80::10", "ffff:ffff:ffff:ffff::", 1, false),
            interface("tailscale0", "100.90.1.1", "255.255.255.255", 2, true),
            interface(
                "tailscale0",
                "fd7a:115c:a1e0::1",
                "ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff",
                2,
                true,
            ),
            interface("tun0", "10.8.0.2", "255.255.255.255", 3, true),
            interface("docker0", "172.17.0.1", "255.255.0.0", 4, false),
            interface("public0", "8.8.8.8", "255.255.255.0", 5, false),
        ]
    }

    fn address(input: &str) -> SocketAddr {
        input.parse().unwrap()
    }

    #[test]
    fn hostname_ip_and_scoped_ipv6_endpoints() {
        for host in [
            "my-laptop",
            "my-laptop.local",
            "my-laptop.tail1234.ts.net",
            "MY-LAPTOP.",
        ] {
            assert_eq!(
                endpoint(&format!(" {host}:49152 ")).unwrap(),
                Endpoint::Hostname(host.into(), 49152)
            );
        }
        for input in [
            "192.168.1.20:49152",
            "100.100.1.2:65535",
            "[fd7a:115c:a1e0::2]:49152",
            "[fe80::2%1]:49152",
        ] {
            assert_eq!(endpoint(input).unwrap(), Endpoint::Address(address(input)));
        }
    }

    #[test]
    fn malformed_hosts_urls_and_invalid_ports_are_rejected() {
        for input in [
            "https://laptop:49152",
            "laptop:49152/path",
            "user@laptop:49152",
            "laptop",
            "laptop:0",
            "laptop:65536",
            "laptop:+80",
            "laptop: 80",
            "laptop:",
            "192.168.1.20:0",
            "999.1.1.1:80",
            "-laptop:80",
            "laptop-:80",
            "a..b:80",
            "my laptop:80",
            "[fd7a::2]:0",
            "fd7a::2:49152",
        ] {
            assert!(endpoint(input).is_err(), "accepted {input}");
        }
    }

    #[test]
    fn default_lan_does_not_enable_vpn_or_routed_peers() {
        let network = Network::from_interfaces(fixture(), None).unwrap();
        assert!(network.interfaces.iter().all(|i| i.name == "wlan0"));
        assert!(network.allows(address("192.168.1.20:49152")));
        assert!(network.allows(address("[fd11::20]:49152")));
        for peer in ["192.168.2.20:49152", "10.8.0.3:49152", "100.90.1.2:49152"] {
            assert!(!network.allows(address(peer)));
        }
        assert!(Network::from_interfaces(fixture(), Some("missing")).is_err());
        assert!(Network::from_interfaces(fixture(), Some("docker0")).is_err());
        assert!(Network::from_interfaces(fixture(), Some("public0")).is_err());
        let only_vpn: Vec<_> = fixture()
            .into_iter()
            .filter(|i| i.name == "tailscale0")
            .collect();
        assert!(Network::from_interfaces(only_vpn.clone(), None).is_err());
        assert!(Network::from_interfaces(only_vpn, Some("tailscale0")).is_ok());
    }

    #[test]
    fn selected_overlay_accepts_routed_peers_but_never_public_or_special_addresses() {
        let network = Network::from_interfaces(fixture(), Some("tailscale0")).unwrap();
        for peer in [
            "100.64.0.1:49152",
            "100.127.255.254:49152",
            "10.20.30.40:49152",
            "[fd7a:115c:a1e0::2]:49152",
        ] {
            assert!(network.allows(address(peer)), "rejected {peer}");
        }
        for peer in [
            "100.63.255.255:49152",
            "100.128.0.1:49152",
            "8.8.8.8:49152",
            "127.0.0.1:49152",
            "0.0.0.0:49152",
            "224.0.0.1:49152",
            "255.255.255.255:49152",
            "[::1]:49152",
            "[::]:49152",
            "[ff02::1]:49152",
            "[2001:4860:4860::8888]:49152",
            "[::ffff:127.0.0.1]:49152",
            "[::ffff:8.8.8.8]:49152",
            "100.90.1.2:0",
        ] {
            assert!(!network.allows(address(peer)), "accepted {peer}");
        }
        assert!(network.discovery_interfaces().is_empty());
        assert!(network.allows_incoming(address("100.90.1.1:49152"), address("100.90.1.2:40000")));
        assert!(
            !network.allows_incoming(address("192.168.1.10:49152"), address("100.90.1.2:40000"))
        );
        let vpn = Network::from_interfaces(fixture(), Some("tun0")).unwrap();
        assert!(vpn.allows(address("10.99.0.7:49152")));
    }

    #[test]
    fn link_local_ipv6_requires_the_selected_scope_even_in_routed_mode() {
        for selection in [None, Some("wlan0")] {
            let network = Network::from_interfaces(fixture(), selection).unwrap();
            assert!(network.allows(address("[fe80::20%1]:49152")));
            assert!(!network.allows(address("[fe80::20%2]:49152")));
            assert!(!network.allows(address("[fe80::20]:49152")));
            assert!(!network.allows(address("[fd11::20%2]:49152")));
            assert!(network
                .allows_incoming(address("[fe80::10%1]:49152"), address("[fe80::20%1]:40000")));
            assert!(!network
                .allows_incoming(address("[fe80::10%2]:49152"), address("[fe80::20%1]:40000")));
        }
    }

    #[test]
    fn resolved_addresses_are_filtered_deduplicated_and_alternate_families() {
        let network = Network::from_interfaces(fixture(), Some("tailscale0")).unwrap();
        let candidates = network
            .candidates(
                [
                    "8.8.8.8:80",
                    "[fd7a:115c:a1e0::2]:49152",
                    "[fd7a:115c:a1e0::3]:49152",
                    "100.90.1.2:49152",
                    "100.90.1.2:49152",
                ]
                .map(address)
                .to_vec(),
            )
            .unwrap();
        assert_eq!(
            candidates,
            [
                "[fd7a:115c:a1e0::2]:49152",
                "100.90.1.2:49152",
                "[fd7a:115c:a1e0::3]:49152"
            ]
            .map(address)
        );
        assert!(network
            .candidates(vec![address("8.8.8.8:80"), address("127.0.0.1:80")])
            .is_err());
        let many = (1..=40)
            .map(|i| address(&format!("100.90.1.{i}:49152")))
            .collect();
        assert_eq!(network.candidates(many).unwrap().len(), 16);
    }

    #[test]
    fn stopping_during_dns_wait_ignores_even_a_ready_answer() {
        let (sender, receiver) = mpsc::sync_channel(1);
        sender.send(Ok(vec![address("100.90.1.2:49152")])).unwrap();
        assert!(wait_lookup(receiver, &|| Err(invalid("Stopped"))).is_err());
        let (_sender, receiver) = mpsc::sync_channel(1);
        let calls = std::cell::Cell::new(0);
        let started = Instant::now();
        assert!(wait_lookup(receiver, &|| {
            calls.set(calls.get() + 1);
            if calls.get() > 1 {
                Err(invalid("Stopped"))
            } else {
                Ok(())
            }
        })
        .is_err());
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn system_resolver_returns_numeric_candidates_and_policy_checks_them() {
        let candidates = lookup("localhost".into(), 49152, &|| Ok(())).unwrap();
        assert!(!candidates.is_empty());
        assert!(candidates.iter().all(|a| a.port() == 49152));
        let network = Network::from_interfaces(fixture(), Some("tailscale0")).unwrap();
        assert!(network.candidates(candidates).is_err());
        assert_eq!(
            network.resolve("100.90.1.2:49152", || Ok(())).unwrap(),
            vec![address("100.90.1.2:49152")]
        );
        assert!(network
            .resolve("100.90.1.2:49152", || Err(invalid("Stopped")))
            .is_err());
    }

    #[test]
    fn tcp_connection_uses_the_chosen_source_address() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let stream = connect_socket(
            address("127.0.0.2:0"),
            listener.local_addr().unwrap(),
            Duration::from_secs(1),
        )
        .unwrap();
        let (accepted, peer) = listener.accept().unwrap();
        assert_eq!(peer.ip(), "127.0.0.2".parse::<IpAddr>().unwrap());
        assert_eq!(stream.local_addr().unwrap(), peer);
        assert_eq!(accepted.local_addr().unwrap(), stream.peer_addr().unwrap());
    }

    #[test]
    fn connection_falls_back_to_a_later_candidate_and_obeys_stop() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        // A mismatched source family fails immediately, then the valid address connects.
        let candidates = [
            (address("[::1]:0"), listener.local_addr().unwrap()),
            (address("127.0.0.1:0"), listener.local_addr().unwrap()),
        ];
        let stream = connect_candidates(candidates.into_iter(), || Ok(())).unwrap();
        let (accepted, _) = listener.accept().unwrap();
        assert_eq!(accepted.local_addr().unwrap(), stream.peer_addr().unwrap());
        assert!(connect_candidates(candidates.into_iter(), || Err(invalid("Stopped"))).is_err());
        listener.set_nonblocking(true).unwrap();
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }

    #[test]
    fn fixed_port_conflicts_are_reported_and_automatic_ports_are_released() {
        // Loopback exists on every test host; production selection excludes it.
        let local = interface("test", "127.0.0.1", "255.0.0.0", 1, false);
        let mut network = Network {
            interfaces: vec![local],
            routed: false,
        };
        let listeners = network.listen(0).unwrap();
        let bound = listeners[0].local_addr().unwrap();
        assert!(bound.port() > 0);
        assert!(network.listen(bound.port()).is_err());
        drop(listeners);
        let listeners = network.listen(bound.port()).unwrap();
        assert_eq!(listeners[0].local_addr().unwrap(), bound);
        drop(listeners);
        assert!(TcpListener::bind(bound).is_ok());
    }
}
