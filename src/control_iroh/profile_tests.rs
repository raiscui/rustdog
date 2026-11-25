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

fn daemon_profile(routable_field: &str, bind_field: &str, directory: &str, relay_url: &str) -> String {
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

fn client_profile(routable_field: &str, bind_field: &str, directory: &str, relay_url: &str) -> String {
    format!(
        r#"{{
  "key_file": "/tmp/client.key",
  "directory": {{"id": "{}", "addr": "{directory}"}},
  "relay": {{"url": "{relay_url}", "qad_port": 9002, "root_cert": "/tmp/relay.pem"}},
  "path": "direct"{routable_field}{bind_field}
}}"#,
        endpoint_id(2)
    )
}

fn rendezvous_profile(routable_field: &str, relay_http: &str, relay_https: &str, qad: &str) -> String {
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
    assert!(network(parsed.directory.addr, parsed.routable).is_ok());
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
    assert!(network(parsed.directory.addr, parsed.routable).is_ok());

    let rendezvous = scratch.write(
        "rendezvous.json",
        &rendezvous_profile(
            r#", "routable": true"#,
            "10.21.0.1:7100",
            "10.21.0.1:7101",
            "10.21.0.1:7102",
        ),
    );
    let parsed: RendezvousConfig = profile(&rendezvous).expect("routable rendezvous profile parses");
    assert!(parsed.routable);
    for addr in [parsed.bind, parsed.relay_http, parsed.relay_https, parsed.qad] {
        assert!(network(addr, parsed.routable).is_ok(), "{addr} must be routable");
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
            network(refused.parse().unwrap(), true).unwrap_err().to_string(),
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
            "iroh routable profile requires an explicit HTTPS relay and QAD port"
        );
    }
    println!("relay policy follows the profile without weakening the default");
}
