"""RAI-426 acceptance harness: look up a daemon through a relay-carried directory.

The directory is bound on the router's loopback, so no peer can dial it by IP.
Three cases run against one topology:

  1. the profile shape this lane has always had (an IP for the directory) fails,
     because there is no route to the directory's address at all;
  2. `directory.via_relay` finds the daemon and the ping answers;
  3. the same lookup with a relay-only data path reports `iroh_path=relay`.

    python3 tests/manual/iroh_directory_relay.py <rdog-binary> <scratch-dir> RELAY_IP DAEMON_IP CLIENT_IP
"""

import json
import os
import socket
import subprocess
import sys
import tempfile
import time
from pathlib import Path

binary = str(Path(sys.argv[1]).resolve(strict=True))
scratch = Path(sys.argv[2]).resolve(strict=True)
relay_ip, daemon_ip, client_ip = sys.argv[3], sys.argv[4], sys.argv[5]
os.umask(0o077)

RELAY_NS = os.environ["RAI426_RELAY_NS"]
DAEMON_NS = os.environ["RAI426_DAEMON_NS"]

PORT_PROBE = """
import json, socket, sys
from contextlib import ExitStack

with ExitStack() as stack:
    ports = []
    for kind in (socket.SOCK_DGRAM, socket.SOCK_STREAM, socket.SOCK_STREAM, socket.SOCK_DGRAM):
        sock = stack.enter_context(socket.socket(socket.AF_INET, kind))
        sock.bind(("127.0.0.1", 0))
        ports.append(sock.getsockname()[1])
    json.dump(ports, sys.stdout)
"""


def identity(home, label):
    seed = os.urandom(32)
    private = home / (label + ".key")
    private.write_bytes(seed)
    prefix = bytes.fromhex("302e020100300506032b657004220420")
    public_prefix = bytes.fromhex("302a300506032b6570032100")
    result = subprocess.run(
        ["openssl", "pkey", "-inform", "DER", "-pubout", "-outform", "DER"],
        input=prefix + seed, capture_output=True, check=True, timeout=5,
    ).stdout
    assert result.startswith(public_prefix) and len(result) == len(public_prefix) + 32
    return str(private), result.removeprefix(public_prefix).hex()


def ready(process, logfile, marker, budget=30):
    until = time.monotonic() + budget
    while time.monotonic() < until:
        text = logfile.read_text(errors="replace")
        if process.poll() is not None:
            raise RuntimeError(f"{marker}: process exited {process.returncode}: {text[-2000:]}")
        if marker in text:
            return text
        time.sleep(0.04)
    raise RuntimeError(f"{marker}: timed out: {logfile.read_text(errors='replace')[-2000:]}")


def free_ports():
    probe = subprocess.run(["nsenter", "-t", RELAY_NS, "-n", "python3", "-c", PORT_PROBE],
                           capture_output=True, text=True, check=True, timeout=10)
    return json.loads(probe.stdout)


def case(label):
    print(f"case: {label}", flush=True)


with tempfile.TemporaryDirectory(prefix="iroh-directory-", dir=scratch) as dirname:
    home = Path(dirname)
    env = dict(PATH="/usr/bin:/bin", HOME=dirname, TMPDIR=dirname, LANG="C.UTF-8")
    if os.environ.get("RAI426_LOG"):
        env["RUST_LOG"] = os.environ["RAI426_LOG"]

    rendezvous_key, rendezvous_id = identity(home, "rendezvous")
    daemon_key, daemon_id = identity(home, "daemon")
    owner_key, owner_id = identity(home, "owner")
    cert, key = home / "relay.pem", home / "relay-key.pem"
    subprocess.run(
        ["openssl", "req", "-x509", "-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:P-256",
         "-nodes", "-subj", "/CN=localhost", "-addext", f"subjectAltName=IP:{relay_ip}",
         "-addext", "basicConstraints=critical,CA:FALSE",
         "-days", "1", "-keyout", key, "-out", cert],
        env=env, capture_output=True, check=True, timeout=5,
    )
    directory, http, https, qad = free_ports()
    # The directory endpoint binds the router's loopback: reachable by the relay
    # process itself and by nobody else, which is what "no inbound address" means.
    directory_addr = f"127.0.0.1:{directory}"
    relay_url = f"https://{relay_ip}:{https}"
    target = "lab/nat"

    def config(label, **values):
        path = home / f"{label}.json"
        path.write_text(json.dumps(values))
        print(f"profile: {label} {json.dumps(values)}", flush=True)
        return str(path)

    relay = dict(url=relay_url, qad_port=qad, root_cert=str(cert))
    directory_relay = dict(url=relay_url, root_cert=str(cert))
    rendezvous = config(
        "rendezvous", key_file=rendezvous_key, bind=directory_addr,
        relay_http=f"{relay_ip}:{http}", relay_https=f"{relay_ip}:{https}",
        qad=f"{relay_ip}:{qad}", relay_cert=str(cert), relay_key=str(key),
        registrations={target: daemon_id}, lookups=[owner_id], routable=True,
        directory_relay=directory_relay,
    )

    def daemon_profile(label, directory_block):
        return config(label, key_file=daemon_key, directory=directory_block, relay=relay,
                      name=target, allow=[owner_id], routable=True,
                      bind=f"{daemon_ip}:0")

    def client_profile(label, directory_block, path="auto"):
        return config(label, key_file=owner_key, directory=directory_block, relay=relay,
                      path=path, routable=True, bind=f"{client_ip}:0")

    processes = []

    def spawn(argv, logfile, marker, ns=None, may_exit=False):
        command = ["nsenter", "-t", ns, "-n", "--"] + argv if ns else argv
        with logfile.open("w") as output:
            child = subprocess.Popen(command, cwd=home, env=env, stdin=subprocess.DEVNULL,
                                     stdout=output, stderr=subprocess.STDOUT, start_new_session=True)
        processes.append(child)
        if not may_exit:
            ready(child, logfile, marker)
        return child

    def control(profile, budget=90):
        started = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
        command = [binary, "control", "--transport", "iroh", "--iroh-config", profile,
                   target, "@ping"]
        print(f"receipt: {started} rdog control --transport iroh --iroh-config "
              f"{Path(profile).name} {target} '@ping'", flush=True)
        result = subprocess.run(command, cwd=home, env=env, capture_output=True, text=True,
                                timeout=budget, stdin=subprocess.DEVNULL)
        combined = result.stdout + result.stderr
        paths = sorted({token for token in combined.split() if token.startswith("iroh_path=")})
        hop = sorted({token for token in combined.split()
                      if token.startswith("iroh_directory_path=")})
        print(f"receipt: {Path(profile).name} exit={result.returncode} "
              f"paths={' '.join(paths) or 'none'} directory_hop={' '.join(hop) or 'none'}",
              flush=True)
        result.directory_hop = hop
        return result, paths

    try:
        # Case 1: the profile shape without the switch has no way to the directory.
        case("an address nothing listens on still fails without the switch")
        plain_daemon = daemon_profile("daemon-plain",
                                      dict(id=rendezvous_id, addr=f"{relay_ip}:{directory}"))
        plain_client = client_profile("client-plain",
                                      dict(id=rendezvous_id, addr=f"{relay_ip}:{directory}"))
        rendezvous_process = spawn([binary, "rendezvous", "-c", rendezvous],
                                   home / "rendezvous.log",
                                   f"iroh rendezvous ready relay_https={relay_ip}:{https}",
                                   ns=RELAY_NS)
        dead = spawn([binary, "daemon", "--transport", "iroh", "-c", plain_daemon],
                     home / "daemon-plain.log", "", ns=DAEMON_NS, may_exit=True)
        deadline = time.monotonic() + 60
        while dead.poll() is None and time.monotonic() < deadline:
            time.sleep(0.05)
        text = (home / "daemon-plain.log").read_text(errors="replace")
        assert dead.poll() is not None, f"the daemon kept waiting for a directory it cannot reach: {text[-1500:]}"
        assert dead.returncode != 0, f"a directory nobody serves registered the daemon: {text[-1500:]}"
        assert "iroh daemon registered" not in text, "registration was reported without a directory"
        missed, _ = control(plain_client)
        assert missed.returncode != 0, "a lookup reached a directory that is not on that address"
        print(f"case 1: daemon exit={dead.returncode}, client exit={missed.returncode}", flush=True)

        # Case 2: the switch, both ends behind NAT with no inbound address.
        case("a relay-carried directory finds the daemon and the ping answers")
        nat_daemon = daemon_profile("daemon-nat",
                                    dict(id=rendezvous_id, via_relay=True, relay=directory_relay))
        nat_client = client_profile("client-nat",
                                    dict(id=rendezvous_id, via_relay=True, relay=directory_relay))
        for profile in (nat_daemon, nat_client):
            block = json.loads(Path(profile).read_text())["directory"]
            assert "addr" not in block, f"{profile} still names a directory address"
        daemon_process = spawn([binary, "daemon", "--transport", "iroh", "-c", nat_daemon],
                               home / "daemon-nat.log", "iroh daemon registered", ns=DAEMON_NS)
        rendezvous_log = (home / "rendezvous.log").read_text(errors="replace")
        assert f"directory_relay={relay_url}" in rendezvous_log, \
            f"the directory never joined its relay: {rendezvous_log[-1500:]}"
        answered, answered_paths = control(nat_client)
        assert answered.returncode == 0, f"the relay-carried lookup failed: {answered.stderr[-1500:]}"
        assert '"pong":true' in answered.stdout.replace(" ", ""), answered.stdout
        assert answered_paths, f"the client reported no path: {answered.stderr[-1500:]}"
        assert "iroh_directory_path=relay" in answered.directory_hop, \
            f"the relay-carried lookup reported no directory path: {answered.stderr[-1500:]}"
        print(f"case 2: answered over {' '.join(answered_paths)}, directory hop reported "
              f"{' '.join(answered.directory_hop)}", flush=True)

        # Case 3: the same lookup with the data path forced onto the relay.
        case("a relay-only data path reports iroh_path=relay")
        relay_client = client_profile("client-relay",
                                      dict(id=rendezvous_id, via_relay=True,
                                           relay=directory_relay), path="relay")
        forced, paths = control(relay_client)
        assert forced.returncode == 0, f"the relay-only session failed: {forced.stderr[-1500:]}"
        assert paths, f"the client reported no path: {forced.stderr[-1500:]}"
        assert "iroh_path=relay" in paths, f"a relay-only profile did not use the relay: {paths}"

        # Case 4: no reachable address on either side — the directory and the data
        # path both have to ride the relay, and the path readout says so.
        case("both ends behind NAT: the directory and the data path both ride the relay")
        blackholed = subprocess.run(["ip", "route", "replace", "blackhole", f"{daemon_ip}/32"],
                                    capture_output=True, text=True)
        assert blackholed.returncode == 0, blackholed.stderr
        nat_auto = client_profile("client-fallback",
                                  dict(id=rendezvous_id, via_relay=True, relay=directory_relay),
                                  path="auto")
        fell_back, fallback_paths = control(nat_auto)
        assert fell_back.returncode == 0, f"the fallback session failed: {fell_back.stderr[-1500:]}"
        assert "iroh_path=direct unavailable" in fell_back.stderr, \
            f"the direct path was not tried before the relay: {fell_back.stderr[-1500:]}"
        assert "iroh_path=relay" in fallback_paths, \
            f"a client with no direct path did not use the relay: {fallback_paths}"
        refused = next((line for line in fell_back.stderr.splitlines()
                        if "iroh_path=direct unavailable" in line), "")
        print(f"case 4: {refused.strip()[:220]}", flush=True)
        print(f"case 4: answered over {' '.join(fallback_paths)} with the direct path blackholed",
              flush=True)

        print(f"receipt: directory_relay={relay_url} directory_bind={directory_addr}", flush=True)
        print(f"receipt: rendezvous_log={home / 'rendezvous.log'}", flush=True)
        print(f"receipt: daemon_log={home / 'daemon-nat.log'}", flush=True)
        print("cleanup: relay-carried directory cases reaped", flush=True)
    finally:
        for child in reversed(processes):
            if child.poll() is None:
                child.terminate()
        for child in reversed(processes):
            try:
                child.wait(timeout=10)
            except subprocess.TimeoutExpired:
                child.kill()
                child.wait(timeout=10)
        assert not any(child.poll() is None for child in processes), "leftovers survived"
