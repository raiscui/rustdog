//! What the directory accepts as a registration, and why it refuses the rest.
//!
//! One name has one owner and one relay; the IP candidates are whatever the
//! daemon currently binds, including every interface a wildcard socket covers.

use super::*;
use iroh::SecretKey;
use std::collections::HashMap;

fn endpoint_id(byte: u8) -> EndpointId {
    SecretKey::from_bytes(&[byte; 32]).public()
}

fn relay_url() -> RelayUrl {
    "https://10.21.0.1:7001".parse().expect("relay url")
}

fn registry(owner: EndpointId) -> Registry {
    Registry {
        owners: HashMap::from([("lab/daemon".into(), owner)]),
        readers: vec![endpoint_id(9)],
        leases: HashMap::new(),
        relay: relay_url(),
        routable: true,
    }
}

fn register(registry: &mut Registry, peer: EndpointId, addrs: Vec<TransportAddr>) -> DirectoryReply {
    registry.request(
        peer,
        DirectoryRequest::Register {
            name: "lab/daemon".into(),
            addr: EndpointAddr::from_parts(peer, addrs),
        },
    )
}

fn ip(addr: &str) -> TransportAddr {
    TransportAddr::Ip(addr.parse().expect("socket address"))
}

#[test]
fn a_registration_carries_every_interface_the_daemon_binds() {
    let owner = endpoint_id(2);
    let mut registry = registry(owner);
    let reply = register(
        &mut registry,
        owner,
        vec![
            ip("10.42.0.2:7000"),
            ip("10.43.0.2:7000"),
            TransportAddr::Relay(relay_url()),
        ],
    );
    assert!(matches!(reply, DirectoryReply::Registered));
    let lookup = registry.request(
        endpoint_id(9),
        DirectoryRequest::Lookup { name: "lab/daemon".into() },
    );
    let DirectoryReply::Found { addr } = lookup else {
        panic!("a registered daemon has to resolve");
    };
    let ips = addr.ip_addrs().count();
    assert_eq!(ips, 2, "every advertised interface survives the round trip");
    assert_eq!(addr.relay_urls().count(), 1);
}

#[test]
fn a_routable_registration_never_carries_loopback() {
    let owner = endpoint_id(2);
    let mut loopback = registry(owner);
    let denied = register(
        &mut loopback,
        owner,
        vec![
            ip("10.42.0.2:7000"),
            ip("127.0.0.1:7000"),
            TransportAddr::Relay(relay_url()),
        ],
    );
    assert!(
        matches!(denied, DirectoryReply::Rejected { ref reason } if reason == "iroh_registration_denied"),
        "a peer dialling 127.0.0.1 only reaches itself"
    );

    let mut loopback_profile = registry(owner);
    loopback_profile.routable = false;
    let accepted = register(
        &mut loopback_profile,
        owner,
        vec![ip("127.0.0.1:7000"), TransportAddr::Relay(relay_url())],
    );
    assert!(
        matches!(accepted, DirectoryReply::Registered),
        "a loopback profile still registers its loopback socket"
    );
}

#[test]
fn a_registration_needs_an_address_a_matching_relay_and_its_owner() {
    let owner = endpoint_id(2);

    let mut no_ip = registry(owner);
    let denied = register(&mut no_ip, owner, vec![TransportAddr::Relay(relay_url())]);
    assert!(matches!(denied, DirectoryReply::Rejected { ref reason } if reason == "iroh_registration_denied"));

    let mut wildcard = registry(owner);
    let denied = register(
        &mut wildcard,
        owner,
        vec![ip("0.0.0.0:7000"), TransportAddr::Relay(relay_url())],
    );
    assert!(
        matches!(denied, DirectoryReply::Rejected { ref reason } if reason == "iroh_registration_denied"),
        "an unspecified address is not a candidate"
    );

    let mut wrong_relay = registry(owner);
    let denied = register(
        &mut wrong_relay,
        owner,
        vec![
            ip("10.42.0.2:7000"),
            TransportAddr::Relay("https://10.21.0.9:7001".parse().expect("relay url")),
        ],
    );
    assert!(
        matches!(denied, DirectoryReply::Rejected { ref reason } if reason == "iroh_registration_denied"),
        "a relay that is not the configured one cannot carry the name"
    );

    let stranger = endpoint_id(4);
    let mut not_owner = registry(owner);
    let denied = register(
        &mut not_owner,
        stranger,
        vec![ip("10.42.0.9:7000"), TransportAddr::Relay(relay_url())],
    );
    assert!(
        matches!(denied, DirectoryReply::Rejected { ref reason } if reason == "iroh_registration_denied"),
        "a name has exactly one owner"
    );
    let still_owner = register(
        &mut not_owner,
        owner,
        vec![ip("10.42.0.2:7000"), TransportAddr::Relay(relay_url())],
    );
    assert!(matches!(still_owner, DirectoryReply::Registered));
}
