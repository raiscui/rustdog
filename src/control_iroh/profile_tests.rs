use super::*;
use iroh::SecretKey;
use std::{fs, path::PathBuf};

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("rdog-iroh-profile-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path).expect("create profile scratch dir");
        Self(path)
    }

    fn write(&self, name: &str, body: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, body).expect("write profile");
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn endpoint_id(byte: u8) -> EndpointId {
    SecretKey::from_bytes(&[byte; 32]).public()
}

fn daemon_profile(
    routable_field: &str,
    bind_field: &str,
    directory: &str,
    relay_url: &str,
) -> String {
    format!(
        r#"{{
  "key_file": "/tmp/daemon.key",
  "directory": {{"id": "{}", "addr": "{directory}"}},
  "relay": {{"url": "{relay_url}", "qad_port": 9002, "root_cert": "/tmp/relay.pem"}},
  "name": "lab/daemon",
  "allow": ["{}"]{routable_field}{bind_field}
}}"#,
        endpoint_id(2),
        endpoint_id(3)
    )
}

fn client_profile(
    routable_field: &str,
    bind_field: &str,
    directory: &str,
    relay_url: &str,
) -> String {
    client_profile_with_path(routable_field, bind_field, directory, relay_url, "direct")
}

fn client_profile_with_path(
    routable_field: &str,
    bind_field: &str,
    directory: &str,
    relay_url: &str,
    path: &str,
) -> String {
    format!(
        r#"{{
  "key_file": "/tmp/client.key",
  "directory": {{"id": "{}", "addr": "{directory}"}},
  "relay": {{"url": "{relay_url}", "qad_port": 9002, "root_cert": "/tmp/relay.pem"}},
  "path": "{path}"{routable_field}{bind_field}
}}"#,
        endpoint_id(2)
    )
}

fn rendezvous_profile(
    routable_field: &str,
    relay_http: &str,
    relay_https: &str,
    qad: &str,
) -> String {
    format!(
        r#"{{
  "key_file": "/tmp/rendezvous.key",
  "bind": "10.31.0.1:7000",
  "relay_http": "{relay_http}",
  "relay_https": "{relay_https}",
  "qad": "{qad}",
  "relay_cert": "/tmp/relay.pem",
  "relay_key": "/tmp/relay.key",
  "registrations": {{"lab/daemon": "{}"}},
  "lookups": ["{}"]{routable_field}
}}"#,
        endpoint_id(3),
        endpoint_id(4)
    )
}

#[test]
fn routable_profile_opens_directory_relay_and_bind() {
    let scratch = Scratch::new();
    let daemon = scratch.write(
        "daemon.json",
        &daemon_profile(
            r#", "routable": true"#,
            r#", "bind": "10.21.0.2:0""#,
            "10.21.0.1:7000",
            "https://10.21.0.1:7001",
        ),
    );
    let parsed: DaemonConfig = profile(&daemon).expect("routable daemon profile parses");
    assert!(parsed.routable);
    assert_eq!(parsed.bind, Some("10.21.0.2:0".parse().unwrap()));
    let dial = parsed
        .directory
        .dial(parsed.routable)
        .expect("the directory dial target resolves");
    assert_eq!(
        dial.addr.ip_addrs().count(),
        1,
        "a directory with an address is dialled by it"
    );
    assert!(dial.relay.is_none());
    assert!(relay_allowed(&parsed.relay.url, parsed.relay.qad_port, parsed.routable).is_ok());

    let client = scratch.write(
        "client.json",
        &client_profile(
            r#", "routable": true"#,
            r#", "bind": "10.22.0.2:0""#,
            "10.21.0.1:7000",
            "https://10.21.0.1:7001",
        ),
    );
    let parsed: ClientConfig = profile(&client).expect("routable client profile parses");
    assert!(matches!(parsed.path, Route::Direct));
    assert_eq!(parsed.bind, Some("10.22.0.2:0".parse().unwrap()));
    let dial = parsed
        .directory
        .dial(parsed.routable)
        .expect("the directory dial target resolves");
    assert_eq!(dial.addr.ip_addrs().count(), 1);

    let rendezvous = scratch.write(
        "rendezvous.json",
        &rendezvous_profile(
            r#", "routable": true"#,
            "10.21.0.1:7100",
            "10.21.0.1:7101",
            "10.21.0.1:7102",
        ),
    );
    let parsed: RendezvousConfig =
        profile(&rendezvous).expect("routable rendezvous profile parses");
    assert!(parsed.routable);
    for addr in [
        parsed.bind,
        parsed.relay_http,
        parsed.relay_https,
        parsed.qad,
    ] {
        assert!(
            network(addr, parsed.routable).is_ok(),
            "{addr} must be routable"
        );
    }
    println!("routable profiles parse with routable directory, relay and bind addresses");
}

#[test]
fn default_profile_keeps_loopback_only_network_policy() {
    let scratch = Scratch::new();
    let daemon = scratch.write(
        "daemon.json",
        &daemon_profile("", "", "127.0.0.1:7000", "https://127.0.0.1:7001"),
    );
    let parsed: DaemonConfig = profile(&daemon).expect("default daemon profile parses");
    assert!(!parsed.routable);
    assert_eq!(parsed.bind, None);

    assert!(network("127.0.0.1:7000".parse().unwrap(), false).is_ok());
    let refused = network("10.21.0.1:7000".parse().unwrap(), false).unwrap_err();
    assert_eq!(refused.to_string(), "P1 iroh requires IPv4 loopback");
    assert_eq!(
        relay_allowed(&"https://10.21.0.1:7001".parse().unwrap(), 9002, false)
            .unwrap_err()
            .to_string(),
        "P1 iroh requires an explicit HTTPS loopback relay and QAD port"
    );
    println!("default profile still refuses routable addresses with the P1 message");
}

#[test]
fn routable_profile_refuses_unspecified_addresses() {
    assert!(network("10.21.0.1:7000".parse().unwrap(), true).is_ok());
    assert!(network("[fd00::1]:7000".parse().unwrap(), true).is_ok());
    for refused in ["0.0.0.0:7000", "[::]:7000", "255.255.255.255:7000"] {
        assert_eq!(
            network(refused.parse().unwrap(), true)
                .unwrap_err()
                .to_string(),
            "iroh routable profile requires an explicit address",
            "{refused} must not be advertised"
        );
    }
    println!("routable policy accepts routable and refuses unspecified addresses");
}

#[test]
fn relay_policy_follows_the_profile() {
    let routable_relay: RelayUrl = "https://10.21.0.1:7001".parse().unwrap();
    let plaintext: RelayUrl = "http://10.21.0.1:7001".parse().unwrap();
    assert!(relay_allowed(&"https://127.0.0.1:7001".parse().unwrap(), 7002, false).is_ok());
    for (url, qad) in [(&plaintext, 7002u16), (&routable_relay, 0)] {
        assert_eq!(
            relay_allowed(url, qad, true).unwrap_err().to_string(),
            "iroh routable profile requires an HTTPS relay host and a non-zero QAD port"
        );
    }
    println!("relay policy follows the profile without weakening the default");
}

#[test]
fn auto_path_is_available_for_interactive_fallback() {
    let scratch = Scratch::new();
    let client = scratch.write(
        "client.json",
        &client_profile_with_path("", "", "127.0.0.1:7000", "https://127.0.0.1:7001", "auto"),
    );
    let parsed: ClientConfig = profile(&client).expect("auto path profile parses");
    assert!(
        matches!(parsed.path, Route::Auto),
        "an interactive session must be able to fall back to the relay"
    );
    println!("auto path profile parses for interactive relay fallback");
}

#[test]
fn frame_timeout_profile_sets_the_write_budget() {
    let scratch = Scratch::new();
    let daemon = scratch.write(
        "daemon.json",
        &daemon_profile(
            r#", "routable": true"#,
            r#", "bind": "10.21.0.2:0", "frame_timeout_ms": 9000"#,
            "10.21.0.1:7000",
            "https://10.21.0.1:7001",
        ),
    );
    let parsed: DaemonConfig =
        profile(&daemon).expect("a daemon profile with a frame budget parses");
    parsed
        .check_frame_budget()
        .expect("a supported budget is accepted");
    assert_eq!(parsed.frame_timeout(), Duration::from_millis(9000));

    let client = scratch.write(
        "client.json",
        &client_profile(
            r#", "routable": true"#,
            r#", "bind": "10.22.0.2:0", "frame_timeout_ms": 4500"#,
            "10.21.0.1:7000",
            "https://10.21.0.1:7001",
        ),
    );
    let parsed: ClientConfig =
        profile(&client).expect("a client profile with a frame budget parses");
    parsed
        .check_frame_budget()
        .expect("a supported budget is accepted");
    assert_eq!(parsed.frame_timeout(), Duration::from_millis(4500));
}

#[test]
fn frame_timeout_profile_refuses_a_budget_below_the_floor() {
    let scratch = Scratch::new();
    let quick = scratch.write(
        "quick.json",
        &daemon_profile(
            r#", "routable": true"#,
            r#", "bind": "10.21.0.2:0", "frame_timeout_ms": 50"#,
            "10.21.0.1:7000",
            "https://10.21.0.1:7001",
        ),
    );
    let parsed: DaemonConfig = profile(&quick).expect("the profile itself still parses");
    let err = parsed
        .check_frame_budget()
        .expect_err("a sub-second frame budget must be refused");
    assert!(
        err.to_string().contains("frame_timeout_ms"),
        "the refusal has to name the field: {err}"
    );
}

#[test]
fn a_routable_profile_may_bind_the_wildcard_socket() {
    let wildcard: SocketAddr = "0.0.0.0:0".parse().unwrap();
    assert_eq!(
        bind_addr(None, true).expect("an omitted bind is the wildcard"),
        wildcard,
        "an omitted bind lets the socket follow the machine's current addresses"
    );
    assert_eq!(
        bind_addr(Some(wildcard), true).expect("an explicit wildcard is accepted"),
        wildcard
    );
    assert_eq!(
        bind_addr(Some("10.22.0.2:0".parse().unwrap()), true)
            .expect("an explicit address stays valid"),
        "10.22.0.2:0".parse().unwrap()
    );
}

#[test]
fn a_bind_never_widens_what_is_refused() {
    assert_eq!(
        bind_addr(None, false).expect("a loopback profile keeps its default"),
        "127.0.0.1:0".parse().unwrap()
    );
    let ported: SocketAddr = "0.0.0.0:5000".parse().unwrap();
    let err = bind_addr(Some(ported), true)
        .expect_err("a wildcard with a fixed port must be refused, not silently dropped");
    assert!(err.to_string().contains("explicit address"), "{err}");
    let foreign: SocketAddr = "10.22.0.2:0".parse().unwrap();
    let err = bind_addr(Some(foreign), false)
        .expect_err("a loopback profile still refuses a routable bind");
    assert!(err.to_string().contains("loopback"), "{err}");
}

#[test]
fn a_published_address_stays_explicit() {
    let wildcard: SocketAddr = "0.0.0.0:0".parse().unwrap();
    assert_eq!(
        bindable(wildcard, true).expect("a routable bind may be the wildcard"),
        wildcard
    );
    let err = network(wildcard, true)
        .expect_err("a registration candidate cannot be the wildcard itself");
    assert!(err.to_string().contains("explicit address"), "{err}");
}

fn relay_carried_client(directory: &str, routable: bool) -> String {
    format!(
        r#"{{
  "key_file": "/tmp/client.key",
  "directory": {directory},
  "relay": {{"url": "https://{}", "qad_port": 9002, "root_cert": "/tmp/relay.pem"}},
  "path": "auto"{}
}}"#,
        if routable {
            "10.21.0.1:7001"
        } else {
            "127.0.0.1:7001"
        },
        if routable {
            r#", "routable": true"#
        } else {
            ""
        }
    )
}

#[test]
fn a_directory_without_an_address_is_dialled_through_its_relay() {
    let scratch = Scratch::new();
    let relay: RelayUrl = "https://127.0.0.1:7100".parse().unwrap();
    let client = scratch.write(
        "client.json",
        &relay_carried_client(
            &format!(
                r#"{{"id": "{}", "via_relay": true, "relay": {{"url": "{relay}", "root_cert": "/tmp/relay.pem"}}}}"#,
                endpoint_id(2)
            ),
            false,
        ),
    );
    let parsed: ClientConfig = profile(&client).expect("a relay-carried directory profile parses");
    parsed
        .check_frame_budget()
        .expect("the profile is otherwise unchanged");
    assert!(
        parsed.directory.addr.is_none(),
        "the profile never names an address"
    );
    let dial = parsed
        .directory
        .dial(parsed.routable)
        .expect("the relay resolves the dial target");
    assert_eq!(
        dial.addr.ip_addrs().count(),
        0,
        "no address is invented for a directory that published none"
    );
    assert_eq!(
        dial.addr.relay_urls().next(),
        Some(&relay),
        "the relay is the only address the lookup is dialled at"
    );
    assert!(
        dial.relay.is_some(),
        "the endpoint needs the relay it dials through"
    );
    println!("directory: a relay-carried lookup dials no IP at all");
}

#[test]
fn the_directory_switch_never_invents_a_dial_target() {
    let scratch = Scratch::new();
    let cases = [
        (
            "no switch, no address",
            format!(r#"{{"id": "{}"}}"#, endpoint_id(2)),
            "requires `addr`",
        ),
        (
            "switch without a relay",
            format!(r#"{{"id": "{}", "via_relay": true}}"#, endpoint_id(2)),
            "`via_relay` requires a `relay`",
        ),
        (
            "both a dial target and the switch",
            format!(
                r#"{{"id": "{}", "addr": "127.0.0.1:7100", "via_relay": true, "relay": {{"url": "https://127.0.0.1:7100", "root_cert": "/tmp/directory.pem"}}}}"#,
                endpoint_id(2)
            ),
            "names both `addr` and `via_relay`",
        ),
        (
            "relay without the switch",
            format!(
                r#"{{"id": "{}", "relay": {{"url": "https://127.0.0.1:7100", "root_cert": "/tmp/directory.pem"}}}}"#,
                endpoint_id(2)
            ),
            "only read with `via_relay`",
        ),
    ];
    for (label, directory, expected) in cases {
        let path = scratch.write("client.json", &relay_carried_client(&directory, false));
        let parsed: ClientConfig = profile(&path).expect("the profile itself still parses");
        let err = parsed
            .directory
            .dial(parsed.routable)
            .err()
            .expect("an ambiguous directory profile must not pick a target");
        assert!(err.to_string().contains(expected), "{label}: {err}");
    }
    let anchorless = scratch.write(
        "anchorless.json",
        &relay_carried_client(
            &format!(
                r#"{{"id": "{}", "via_relay": true, "relay": {{"url": "https://127.0.0.1:7100"}}}}"#,
                endpoint_id(2)
            ),
            false,
        ),
    );
    assert!(
        profile::<ClientConfig>(&anchorless).is_err(),
        "a relay-carried directory without a named CA chain must not parse at all"
    );
    println!("directory: every half-configured profile fails closed");
}

#[test]
fn only_a_named_directory_anchor_widens_the_roots() {
    let scratch = Scratch::new();
    let client = scratch.write(
        "client.json",
        &relay_carried_client(
            &format!(
                r#"{{"id": "{}", "via_relay": true, "relay": {{"url": "https://127.0.0.1:7100", "root_cert": "/tmp/directory.pem"}}}}"#,
                endpoint_id(2)
            ),
            false,
        ),
    );
    let parsed: ClientConfig = profile(&client).expect("a relay-carried directory profile parses");
    let lane = Relay {
        url: "https://127.0.0.1:7001".parse().unwrap(),
        qad_port: 9002,
        root_cert: PathBuf::from("/tmp/relay.pem"),
    };
    let dial = parsed
        .directory
        .dial(parsed.routable)
        .expect("the switch resolves");
    let directory = dial.relay.expect("the switch carries a relay");

    assert_eq!(
        relay_roots(None, &directory),
        RelayRoots::Pinned(PathBuf::from("/tmp/directory.pem")),
        "a directory endpoint trusts exactly the chain the profile named"
    );
    assert_eq!(
        relay_roots(Some(&lane), &directory),
        RelayRoots::LaneAndDirectory(
            PathBuf::from("/tmp/relay.pem"),
            PathBuf::from("/tmp/directory.pem")
        ),
        "a shared endpoint pins the lane's chain and the named directory anchor together"
    );
    println!(
        "directory: both root sets are explicit chains — {} and nothing else",
        "no public CA is trusted on either"
    );
    println!("directory: the named anchor decides the root set, nothing else does");
}

#[test]
fn a_directory_relay_url_follows_the_profile_without_weakening_it() {
    let loopback: RelayUrl = "https://127.0.0.1:7100".parse().unwrap();
    let routable: RelayUrl = "https://10.21.0.1:7100".parse().unwrap();
    let plaintext: RelayUrl = "http://127.0.0.1:7100".parse().unwrap();
    assert!(directory_url_ok(&loopback, false).is_ok());
    assert!(directory_url_ok(&routable, true).is_ok());
    let err = directory_url_ok(&routable, false)
        .expect_err("a loopback profile keeps its relay on loopback");
    assert!(err.to_string().contains("loopback"), "{err}");
    let err = directory_url_ok(&plaintext, true).expect_err("a directory relay is never plaintext");
    assert!(err.to_string().contains("HTTPS"), "{err}");
    println!("directory: the relay URL follows the same profile gate as the lane relay");
}

#[test]
fn the_directory_hop_fits_inside_the_lease_it_renews() {
    assert!(
        DIRECTORY_CONNECT < LEASE - RENEW,
        "a directory call that outlives the lease it renews denies every peer while it is in flight"
    );
    assert!(
        DIRECTORY_CONNECT >= IO,
        "the hop still needs a usable budget"
    );
    println!("directory: the hop budget stays inside the lease it refreshes");
}

#[test]
fn a_renewal_never_outlives_the_lease_it_refreshes() {
    assert_eq!(
        renewal_budget(LEASE),
        DIRECTORY_CONNECT,
        "a healthy renewal keeps the full directory budget"
    );
    let nearly_due = Duration::from_secs(2);
    assert_eq!(
        renewal_budget(nearly_due),
        nearly_due,
        "an attempt near the deadline gets only the time that is left"
    );
    println!("directory: a late renewal cannot run past the lease it refreshes");
}

#[test]
fn a_public_relay_spelled_with_the_default_port_is_accepted_where_it_matters() {
    // A standard public relay is https on 443, and `RelayUrl` normalizes that port
    // away, so the explicit-port rule would refuse the only spelling it has.
    let public: RelayUrl = "https://relay.example.".parse().unwrap();
    assert_eq!(public.port(), None, "the default port is normalized away");
    assert!(
        relay_allowed(&public, 7842, true).is_ok(),
        "a routable profile accepts a public relay with the default port"
    );
    let err = relay_allowed(&public, 7842, false)
        .expect_err("the loopback profile still pins an explicit loopback relay");
    assert!(err.to_string().contains("loopback"), "{err}");
    // The port rule itself has to be covered on its own: a loopback host that only
    // lacks the port would pass if that branch had lost the explicit-port check.
    let loopback_without_port: RelayUrl = "https://127.0.0.1".parse().unwrap();
    assert_eq!(loopback_without_port.port(), None);
    let err = relay_allowed(&loopback_without_port, 7842, false)
        .expect_err("the loopback profile still requires an explicit port");
    assert!(err.to_string().contains("loopback"), "{err}");
    let plaintext: RelayUrl = "http://relay.example.".parse().unwrap();
    let err = relay_allowed(&plaintext, 7842, true).expect_err("plaintext stays refused");
    assert!(err.to_string().contains("HTTPS"), "{err}");
    println!("relay: a public relay is spellable, and both profiles keep their own gate");
}
