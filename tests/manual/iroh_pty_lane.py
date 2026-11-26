"""RAI-305 acceptance harness: rdog control NAMESPACE/NAME --pty over iroh.

Drives the formal CLI against a real daemon and embedded relay. Every case
asserts on observable state: client exit codes, PTY output, daemon log lines and
process existence. Run under an isolated HOME/TMPDIR; the caller owns the
network namespace.

    python3 tests/manual/iroh_pty_lane.py <rdog-binary> <scratch-dir> [--routable A B]
    python3 tests/manual/iroh_pty_lane.py <rdog-binary> <scratch-dir> --advertise
"""

import errno
import json
import pty
import select as select_import
import os
import re
import signal
import socket
import subprocess
import sys
import tempfile
import time
from pathlib import Path

binary = str(Path(sys.argv[1]).resolve(strict=True))
scratch = Path(sys.argv[2]).resolve(strict=True)
os.umask(0o077)

args = [item for item in sys.argv[3:]]
routable = None
if args and args[0] == "--routable":
    routable = {"directory": args[1], "daemon": args[2], "client": args[3]}


def identity(home, label):
    seed = os.urandom(32)
    private = home / (label + ".key")
    private.write_bytes(seed)
    private_prefix = bytes.fromhex("302e020100300506032b657004220420")
    public_prefix = bytes.fromhex("302a300506032b6570032100")
    result = subprocess.run(
        ["openssl", "pkey", "-inform", "DER", "-pubout", "-outform", "DER"],
        input=private_prefix + seed, capture_output=True, check=True, timeout=5,
    ).stdout
    assert result.startswith(public_prefix) and len(result) == len(public_prefix) + 32
    return str(private), result.removeprefix(public_prefix).hex()


def ready(process, logfile, marker, budget=20):
    until = time.monotonic() + budget
    while time.monotonic() < until:
        text = logfile.read_text(errors="replace")
        if process.poll() is not None:
            raise RuntimeError(f"{marker}: process exited {process.returncode}: {text[-2000:]}")
        if marker in text:
            return text
        time.sleep(0.04)
    raise RuntimeError(f"{marker}: timed out: {logfile.read_text(errors='replace')[-2000:]}")


def pid_exists(pid):
    return Path(f"/proc/{pid}").exists()


def advertise_lane() -> None:
    """A relay address that only the NAT device holds still has to reach peers.

    Emulates 1:1 NAT with three network stacks: the relay binds a private
    address, a NAT stack owns the public one and forwards, and the peers only
    know the public address. The relay URL handed to peers is what tells them
    where to dial, so the positive case proves registration and a ping survive
    when the advertised address is not the bound one; the negative cases prove
    the two ways it can be wrong fail visibly instead of half-working.
    """
    target = "lab/pty"
    relay_ip = os.environ["RAI305_ADV_RELAY_IP"]
    public_ip = os.environ["RAI305_ADV_PUBLIC_IP"]
    peer_ip = os.environ["RAI305_ADV_PEER_IP"]
    directory_port, http_port, https_port, qad_port = [
        int(port) for port in os.environ["RAI305_ADV_PORTS"].split(",")]
    relay_ns = os.environ.get("RAI305_ADV_RELAY_NS")
    relay_prefix = ["nsenter", "-t", relay_ns, "-n", "--"] if relay_ns else []

    def case(label):
        print(f"case: {label}", flush=True)

    with tempfile.TemporaryDirectory(prefix="iroh-advertise-", dir=scratch) as dirname:
        home = Path(dirname)
        env = dict(PATH="/usr/bin:/bin", HOME=dirname, TMPDIR=dirname, LANG="C.UTF-8",
                   RUST_LOG=os.environ.get("RAI305_LOG", "info"))
        rendezvous_key, rendezvous_id = identity(home, "rendezvous")
        daemon_key, daemon_id = identity(home, "daemon")
        owner_key, owner_id = identity(home, "owner")
        cert, key = home / "relay.pem", home / "relay-key.pem"
        subprocess.run(
            ["openssl", "req", "-x509", "-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:P-256",
             "-nodes", "-subj", "/CN=localhost",
             "-addext", f"subjectAltName=IP:{public_ip}",
             "-addext", "basicConstraints=critical,CA:FALSE",
             "-days", "1", "-keyout", key, "-out", cert],
            env=env, capture_output=True, check=True, timeout=5,
        )
        policy_file = home / "policy.json"
        policy_file.write_text(json.dumps(dict(allow=[owner_id], revoked=[], terminate=[])))
        started = []

        def profiles(label, advertise):
            """Peers only ever know the public address; the relay binds private."""
            relay = dict(url=f"https://{public_ip}:{https_port}", qad_port=qad_port,
                         root_cert=str(cert))
            directory = dict(id=rendezvous_id, addr=f"{public_ip}:{directory_port}")
            rendezvous = home / f"{label}-rendezvous.json"
            values = dict(
                key_file=rendezvous_key, bind=f"{relay_ip}:{directory_port}",
                relay_http=f"{relay_ip}:{http_port}", relay_https=f"{relay_ip}:{https_port}",
                qad=f"{relay_ip}:{qad_port}", relay_cert=str(cert), relay_key=str(key),
                registrations={target: daemon_id}, lookups=[owner_id], routable=True)
            if advertise is not None:
                values["advertise"] = advertise
            rendezvous.write_text(json.dumps(values))
            daemon = home / f"{label}-daemon.json"
            daemon.write_text(json.dumps(dict(
                key_file=daemon_key, directory=directory, relay=relay, name=target,
                allow=[owner_id], policy_file=str(policy_file), routable=True,
                bind=f"{peer_ip}:0")))
            client = home / f"{label}-client.json"
            client.write_text(json.dumps(dict(
                key_file=owner_key, directory=directory, relay=relay,
                path="auto", routable=True, bind=f"{peer_ip}:0")))
            return rendezvous, daemon, client

        def spawn(argv, logfile, marker, budget=60, netns=False, may_exit=False):
            """The relay lives on the far side of the NAT stack, so it runs there.

            `may_exit` is for a case whose whole point is that the process ends:
            the marker is still required, but the exit is the expected outcome
            rather than a startup failure.
            """
            log = open(logfile, "wb")
            child = subprocess.Popen((relay_prefix if netns else []) + argv, cwd=home, env=env,
                                     stdin=subprocess.DEVNULL,
                                     stdout=log, stderr=log, start_new_session=True)
            started.append(child)
            if not may_exit:
                return child, ready(child, logfile, marker, budget=budget)
            until = time.monotonic() + budget
            while time.monotonic() < until:
                text = logfile.read_text(errors="replace")
                if marker in text:
                    return child, text
                if child.poll() is not None:
                    raise RuntimeError(f"{marker}: process exited {child.returncode}: "
                                       f"{text[-2000:]}")
                time.sleep(0.05)
            raise RuntimeError(f"{marker}: timed out: "
                               f"{logfile.read_text(errors='replace')[-2000:]}")

        def reap(child):
            if child.poll() is None:
                os.killpg(child.pid, signal.SIGTERM)
                try:
                    child.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    os.killpg(child.pid, signal.SIGKILL)
                    child.wait(timeout=5)

        try:
            case("a relay behind 1:1 NAT serves peers on its advertised address")
            rendezvous, daemon, client = profiles("nat", public_ip)
            directory_process, directory_log = spawn(
                [binary, "rendezvous", "-c", str(rendezvous)],
                home / "nat-rendezvous.log",
                f"advertised={public_ip}:{https_port}", netns=True)
            assert f"relay_https={relay_ip}:{https_port}" in directory_log, directory_log
            daemon_process, _ = spawn(
                [binary, "daemon", "--transport", "iroh", "-c", str(daemon)],
                home / "nat-daemon.log", "iroh daemon registered")
            text = (home / "nat-daemon.log").read_text(errors="replace")
            assert f"relay https://{public_ip}:{https_port}/" in text, text[-2000:]
            assert f"relay https://{relay_ip}:{https_port}/" not in text, \
                "the daemon took the bound address instead of the advertised one"
            result = subprocess.run(
                [binary, "control", "--transport", "iroh", "--iroh-config", str(client),
                 target, "@ping#7"], cwd=home, env=env, capture_output=True, text=True,
                timeout=90, stdin=subprocess.DEVNULL)
            combined = result.stdout + result.stderr
            assert result.returncode == 0, combined
            response = json.loads(result.stdout.strip().removeprefix("@response "))
            assert response["id"] == 7 and response["value"]["pong"] is True, result.stdout
            print(f"advertise: bound {relay_ip}:{https_port}, advertised "
                  f"{public_ip}:{https_port}; registration and ping passed", flush=True)
            reap(daemon_process)
            reap(directory_process)

            case("without an advertised address the peers cannot be registered")
            rendezvous, daemon, _ = profiles("bound", None)
            directory_process, directory_log = spawn(
                [binary, "rendezvous", "-c", str(rendezvous)],
                home / "bound-rendezvous.log", "iroh rendezvous ready", netns=True)
            assert f"advertised={relay_ip}:{https_port}" in directory_log, directory_log
            blind, _ = spawn(
                [binary, "daemon", "--transport", "iroh", "-c", str(daemon)],
                home / "bound-daemon.log", "iroh_registration_denied", may_exit=True)
            deadline = time.monotonic() + 30
            while time.monotonic() < deadline and blind.poll() is None:
                time.sleep(0.05)
            code = blind.poll()
            assert code is not None and code != 0, f"daemon exit={code} after a denial"
            text = (home / "bound-daemon.log").read_text(errors="replace")
            assert "iroh daemon registered" not in text, "the bound address was accepted"
            assert "iroh_registration_denied" in text, f"no reason given: {text[-1500:]}"
            print(f"advertise: a relay that only names its bound address refuses the "
                  f"registration (exit={code})", flush=True)
            reap(blind)
            reap(directory_process)

            case("an advertised address nothing holds is refused, not half-accepted")
            rendezvous, daemon, _ = profiles("dead", f"{public_ip.rsplit('.', 1)[0]}.250")
            directory_process, directory_log = spawn(
                [binary, "rendezvous", "-c", str(rendezvous)],
                home / "dead-rendezvous.log", "advertised=", netns=True)
            dead = subprocess.Popen(
                [binary, "daemon", "--transport", "iroh", "-c", str(daemon)],
                cwd=home, env=env, stdin=subprocess.DEVNULL,
                stdout=open(home / "dead-daemon.log", "wb"),
                stderr=subprocess.STDOUT, start_new_session=True)
            started.append(dead)
            deadline = time.monotonic() + 90
            while time.monotonic() < deadline and dead.poll() is None:
                time.sleep(0.1)
            assert dead.poll() is not None, "the daemon kept running against a dead relay"
            code = dead.poll()
            text = (home / "dead-daemon.log").read_text(errors="replace")
            assert code != 0, f"daemon exit={code} {text[-1500:]}"
            assert "iroh daemon registered" not in text, "a relay nobody serves registered it"
            print(f"advertise: advertised {public_ip.rsplit('.', 1)[0]}.250 is unreachable; "
                  f"daemon exit={code}, never registered", flush=True)
            reap(directory_process)
            print("advertise: all three directions hold", flush=True)
        finally:
            for child in reversed(started):
                reap(child)
            assert not any(child.poll() is None for child in started), "leftovers survived"

if "--advertise" in args:
    advertise_lane()
    print("cleanup: advertised relay lane reaped")
    raise SystemExit(0)

with tempfile.TemporaryDirectory(prefix="iroh-pty-", dir=scratch) as dirname:
    home = Path(dirname)
    env = dict(PATH="/usr/bin:/bin", HOME=dirname, TMPDIR=dirname, LANG="C.UTF-8")
    if os.environ.get("RAI305_LOG"):
        env["RUST_LOG"] = os.environ["RAI305_LOG"]
    rendezvous_key, rendezvous_id = identity(home, "rendezvous")
    daemon_key, daemon_id = identity(home, "daemon")
    owner_key, owner_id = identity(home, "owner")
    stranger_key, stranger_id = identity(home, "stranger")
    cert, key = home / "relay.pem", home / "relay-key.pem"
    bind = routable["directory"] if routable else "127.0.0.1"
    advertise = routable["directory"] if routable else "127.0.0.1"
    subprocess.run(
        ["openssl", "req", "-x509", "-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:P-256",
         "-nodes", "-subj", "/CN=localhost", "-addext", f"subjectAltName=IP:{advertise}",
         "-addext", "basicConstraints=critical,CA:FALSE",
         "-days", "1", "-keyout", key, "-out", cert],
        env=env, capture_output=True, check=True, timeout=5,
    )
    sockets = [socket.socket(socket.AF_INET, kind) for kind in
               [socket.SOCK_DGRAM, socket.SOCK_STREAM, socket.SOCK_STREAM, socket.SOCK_DGRAM]]
    # The relay runs on another host in the routable matrix, so the port probe
    # here only needs ports that are free where the relay will bind.
    probe = "0.0.0.0" if routable else bind
    for sock in sockets:
        sock.bind((probe, 0))
    directory, http, https, qad = [sock.getsockname()[1] for sock in sockets]
    for sock in sockets:
        sock.close()

    target = "lab/pty"
    addr = lambda ip, port: f"{ip}:{port}"
    directory_addr = addr(bind, directory)
    relay = dict(url=f"https://{addr(advertise, https)}", qad_port=qad, root_cert=str(cert))
    from_lan = bool(routable)

    def config(label, **values):
        result = str(home / f"{label}.json")
        Path(result).write_text(json.dumps(values))
        return result

    policy = home / "policy.json"

    def write_policy(allow, revoked=(), terminate=()):
        policy.write_text(json.dumps(dict(allow=list(allow), revoked=list(revoked),
                                          terminate=list(terminate))))

    write_policy([owner_id])
    rendezvous = config("rendezvous", key_file=rendezvous_key, bind=directory_addr,
                        relay_http=addr(bind, http), relay_https=addr(advertise, https),
                        qad=addr(advertise, qad), relay_cert=str(cert), relay_key=str(key),
                        registrations={target: daemon_id}, lookups=[owner_id, stranger_id],
                        **({"routable": True} if from_lan else {}))
    frame_budget = os.environ.get("RAI305_FRAME_MS")
    budget = {"frame_timeout_ms": int(frame_budget)} if frame_budget else {}
    # RAI305_WILDCARD=1 leaves the daemon's bind out, so it binds the wildcard
    # socket and registers every interface the network stack has.
    wildcard_bind = from_lan and os.environ.get("RAI305_WILDCARD") == "1"
    daemon_scope = ({} if not from_lan else
                    {"routable": True} if wildcard_bind else
                    {"routable": True, "bind": addr(routable["daemon"], 0)})
    daemon = config("daemon", key_file=daemon_key, directory=dict(id=rendezvous_id,
                                                                 addr=directory_addr),
                    relay=relay, name=target, allow=[owner_id, stranger_id],
                    policy_file=str(policy), **budget, **daemon_scope)
    if wildcard_bind:
        print("daemon: wildcard bind (profile names no address)", flush=True)

    def client_profile(label, key_path, path="direct", lease_ttl_ms=None):
        values = dict(key_file=key_path, directory=dict(id=rendezvous_id, addr=directory_addr),
                      relay=relay, path=path, **budget)
        if from_lan:
            values.update(routable=True, bind=addr(routable["client"], 0))
        if lease_ttl_ms is not None:
            values["lease_ttl_ms"] = lease_ttl_ms
        return config(label, **values)

    processes = []

    host_ns = {"daemon": os.environ.get("RAI305_DAEMON_NS"),
               "rendezvous": os.environ.get("RAI305_RELAY_NS")}

    def start(name, flags, marker):
        logfile = home / f"{name}.log"
        command = [binary, *flags]
        if host_ns.get(name):
            command = ["nsenter", "-t", host_ns[name], "-n", *command]
        with logfile.open("w") as output:
            child = subprocess.Popen(command, cwd=home, env=env, stdin=subprocess.DEVNULL,
                                     stdout=output, stderr=subprocess.STDOUT, start_new_session=True)
        processes.append(child)
        ready(child, logfile, marker)
        print(f"{name}: ready", flush=True)
        return logfile

    def policy_cli(action, *extra, profile_path=None):
        return subprocess.run([binary, "policy", "--config", profile_path or daemon, action,
                               *extra], cwd=home, env=env, capture_output=True, text=True,
                              timeout=30)

    def control(profile, *extra, timeout=30, stdin=subprocess.DEVNULL):
        return subprocess.run([binary, "control", "--transport", "iroh", "--iroh-config", profile,
                               target, *extra], cwd=home, env=env, capture_output=True, text=True,
                              timeout=timeout, stdin=stdin)

    fork_pids = []

    def pty_client(owner_key_path, program, ttl=None):
        """Start a --pty client on a real local TTY; returns (pid, master fd)."""
        profile_path = client_profile(f"pty-{len(fork_pids)}", owner_key_path, lease_ttl_ms=ttl)
        pid, master = pty.fork()
        if pid == 0:
            os.execve(binary, [binary, "control", "--transport", "iroh", "--iroh-config",
                               profile_path, target, "--pty", "--", "python3", "-c", program],
                      env)
        fork_pids.append(pid)
        return pid, master

    def pty_attach(owner_key_path, session_id):
        """Attach a real local TTY to an existing session; returns (pid, master fd)."""
        profile_path = client_profile(f"attach-{len(fork_pids)}", owner_key_path)
        pid, master = pty.fork()
        if pid == 0:
            os.execve(binary, [binary, "control", "--transport", "iroh", "--iroh-config",
                               profile_path, target, "--pty-attach", session_id], env)
        fork_pids.append(pid)
        return pid, master

    def read_until(master, needle, budget=20.0):
        deadline = time.monotonic() + budget
        text = ""
        while time.monotonic() < deadline:
            if needle in text:
                return text
            ready, _, _ = select_import.select([master], [], [], 0.05)
            if ready:
                try:
                    text += os.read(master, 4096).decode(errors="replace")
                except OSError:
                    break
        return text

    def pty_wait(pid, budget=30.0):
        deadline = time.monotonic() + budget
        while time.monotonic() < deadline:
            try:
                done, status = os.waitpid(pid, os.WNOHANG)
            except ChildProcessError:
                return 0
            if done == pid:
                return os.waitstatus_to_exitcode(status)
            time.sleep(0.05)
        raise AssertionError(f"pty client {pid} did not exit")

    def pty_alive(pid):
        try:
            done, _ = os.waitpid(pid, os.WNOHANG)
        except ChildProcessError:
            return False
        return done != pid

    def case(label):
        print(f"case: {label}", flush=True)

    try:
        rendezvous_log = start("rendezvous", ["rendezvous", "-c", rendezvous],
                              f"iroh rendezvous ready relay_https={addr(advertise, https)}")
        daemon_log = start("daemon", ["daemon", "--transport", "iroh", "-c", daemon],
                           "iroh daemon registered pty=admitted")
        print("daemon: admitted PTY lane enabled", flush=True)

        case("default ping still answers over the admitted lane")
        owner = client_profile("owner", owner_key)
        result = control(owner, "@ping#7")
        dialled = result.stdout + result.stderr
        assert result.returncode == 0, result.stdout + result.stderr
        response = json.loads(result.stdout.strip().removeprefix("@response "))
        assert response["id"] == 7 and response["value"]["pong"] is True, result.stdout
        print(f"ping: id=7 pong=true exit=0", flush=True)

        if from_lan:
            case("a routable registration advertises no loopback candidate")
            registered = [line for line in rendezvous_log.read_text(errors="replace").splitlines()
                          if "iroh directory registered" in line]
            assert registered, "the daemon never registered"
            assert "127.0.0.1" not in registered[-1], registered[-1]
            print(f"registration: {registered[-1].split('info')[-1].strip()}", flush=True)

            case("a client with no bind follows the address the machine has")
            named = client_profile("wildcard", owner_key)
            wildcard = json.loads(Path(named).read_text())
            wildcard.pop("bind", None)
            wildcard_path = home / "wildcard-client.json"
            wildcard_path.write_text(json.dumps(wildcard))
            result = control(wildcard_path, "@ping#9")
            combined = result.stdout + result.stderr
            assert result.returncode == 0, combined
            response = json.loads(result.stdout.strip().removeprefix("@response "))
            assert response["id"] == 9 and response["value"]["pong"] is True, result.stdout
            print("bind: omitted bind reached the daemon from a non-loopback interface", flush=True)

            case("a bind the machine does not have is still refused")
            absent = json.loads(Path(named).read_text())
            absent["bind"] = "10.99.99.99:0"
            absent_path = home / "absent-client.json"
            absent_path.write_text(json.dumps(absent))
            result = control(absent_path, "@ping#10")
            combined = result.stdout + result.stderr
            assert result.returncode != 0, f"a nonexistent bind address was accepted: {combined}"
            assert "Failed to bind sockets" in combined or "bind" in combined, combined
            print(f"bind: an address the machine lacks still fails (exit={result.returncode})",
                  flush=True)

        case("a stranger is refused before any process exists")
        result = control(client_profile("stranger", stranger_key), "--pty", "--", "/bin/sh", "-c",
                         "sleep 30")
        combined = result.stdout + result.stderr
        assert result.returncode != 0 and "iroh_target_denied" in combined, combined
        assert "iroh pty opened" not in daemon_log.read_text(), "stranger opened a PTY"
        print(f"stranger: refused ({result.returncode}); no pty opened", flush=True)

        case("attached PTY carries stdin and reports the exit code")
        program = ("import sys\nprint('ready', flush=True)\nfor line in sys.stdin:\n"
                   "    print('echo:' + line.strip(), flush=True)\n")
        client, master = pty_client(owner_key, program)
        text = read_until(master, "ready")
        assert "ready" in text, repr(text)
        os.write(master, b"hello rai-305\n")
        text += read_until(master, "echo:hello rai-305")
        assert "echo:hello rai-305" in text, repr(text)
        opened = re.search(r"iroh pty opened session=(\S+) lease=(\S+)", daemon_log.read_text())
        assert opened, daemon_log.read_text()
        session_id, lease_id = opened.group(1), opened.group(2)
        os.write(master, b"\x04")
        code = pty_wait(client)
        os.close(master)
        assert code == 0, (code, repr(text))
        print(f"pty: session={session_id} lease={lease_id} stdin echoed; exit=0", flush=True)

        case("a session outlives its own lease renewal")
        renewed = pty_client(owner_key, "import time\nprint('ready', flush=True)\n"
                            "time.sleep(6)\nprint('alive', flush=True)\n", ttl=2000)
        renewed_pid, renewed_master = renewed
        renewed_text = read_until(renewed_master, "ready")
        if "ready" not in renewed_text:
            raised = pty_wait(renewed_pid, budget=10)
            raise AssertionError(
                f"renewal session never became ready: exit={raised} {renewed_text!r}")
        renewed_code = pty_wait(renewed_pid, budget=40)
        try:
            renewed_text += os.read(renewed_master, 65536).decode(errors="replace")
        except OSError:
            pass
        os.close(renewed_master)
        assert renewed_code == 0, f"session did not survive its renewal: {renewed_code} {renewed_text!r}"
        assert '"op":"renew"' not in renewed_text and "\"op\": \"renew\"" not in renewed_text, \
            f"renewal frame was typed at the remote program: {renewed_text!r}"
        assert "iroh lease renewed" in daemon_log.read_text(), \
            "the daemon never accepted a renewal"
        print("renewal: a session outlived its lease deadline", flush=True)

        case("a detached session can be attached again from another connection")
        loner, loner_master = pty_client(owner_key, "import sys\nprint('ready', flush=True)\n"
                                        "for line in sys.stdin:\n"
                                        "    print('echo:' + line.strip(), flush=True)\n",
                                        ttl=2000)
        assert "ready" in read_until(loner_master, "ready"), "attach target never became ready"
        attachable = re.findall(r"iroh pty opened session=(\S+)", daemon_log.read_text())[-1]
        closed = control(owner, "--pty-detach", attachable)
        assert closed.returncode == 0, closed.stdout + closed.stderr
        pty_wait(loner)
        os.close(loner_master)
        assert f"iroh pty reattached session={attachable}" not in daemon_log.read_text()
        attacher, attacher_master = pty_attach(owner_key, attachable)
        until = time.monotonic() + 10
        while time.monotonic() < until:
            if f"iroh pty reattached session={attachable}" in daemon_log.read_text():
                break
            time.sleep(0.05)
        else:
            raise AssertionError("the detached session was never attached again")
        os.write(attacher_master, b"again\n")
        text = read_until(attacher_master, "echo:again")
        assert "echo:again" in text, repr(text)
        print("attach: holding past the detached lease's deadline", flush=True)
        time.sleep(3.0)
        os.write(attacher_master, b"after-deadline\n")
        text += read_until(attacher_master, "echo:after-deadline")
        assert "echo:after-deadline" in text, (
            f"the reattached session died with the lease that detached it: {text!r}")
        result = control(owner, "--pty-close", attachable)
        assert result.returncode == 0, result.stdout + result.stderr
        pty_wait(attacher)
        os.close(attacher_master)
        print(f"attach: session={attachable} attached again and closed from a third connection",
              flush=True)

        case("a revoked session takes its grandchildren with it")
        doomed, doomed_master = pty_client(owner_key, "import subprocess, time\n"
                                          "kid = subprocess.Popen(['sleep', '61'])\n"
                                          "print('grandchild', kid.pid, flush=True)\n"
                                          "time.sleep(120)\n")
        doomed_text = read_until(doomed_master, "grandchild")
        assert "grandchild" in doomed_text, f"grandchild never started: {doomed_text!r}"
        grandchild = int(re.search(r"grandchild (\d+)", doomed_text).group(1))
        doomed_session = re.findall(r"iroh pty opened session=(\S+)", daemon_log.read_text())[-1]
        write_policy([owner_id], revoked=[owner_id], terminate=[owner_id])
        until = time.monotonic() + 20
        while time.monotonic() < until:
            if f"lease reclaim session={doomed_session} reason=lease_revoked" in \
                    daemon_log.read_text():
                break
            time.sleep(0.05)
        else:
            raise AssertionError("the revoked session was never reclaimed")
        pty_wait(doomed)
        os.close(doomed_master)
        until = time.monotonic() + 10
        while time.monotonic() < until and pid_exists(grandchild):
            time.sleep(0.05)
        assert not pid_exists(grandchild), \
            f"the grandchild {grandchild} survived revocation"
        write_policy([owner_id])
        print(f"revocation: grandchild {grandchild} died with the session", flush=True)

        case("a second connection of the same subject closes its own session")
        holder, holder_master = pty_client(owner_key, "import time\nprint('ready', flush=True)\n"
                                                   "time.sleep(60)\n")
        assert "ready" in read_until(holder_master, "ready"), "holder never became ready"
        sessions = re.findall(r"iroh pty opened session=(\S+)", daemon_log.read_text())
        sleeping = sessions[-1]
        result = control(owner, "--pty-close", sleeping)
        assert result.returncode == 0, result.stdout + result.stderr
        holder_code = pty_wait(holder)
        os.close(holder_master)
        print(f"close: same subject closed session={sleeping}; holder exit={holder_code}",
              flush=True)

        case("a foreign subject cannot close the session of another subject")
        loner, loner_master = pty_client(owner_key, "import time\nprint('ready', flush=True)\n"
                                                "time.sleep(60)\n")
        assert "ready" in read_until(loner_master, "ready"), "loner never became ready"
        target_session = re.findall(r"iroh pty opened session=(\S+)", daemon_log.read_text())[-1]
        write_policy([owner_id, stranger_id])
        result = control(client_profile("stranger", stranger_key), "--pty-close", target_session)
        assert result.returncode != 0 and "iroh_subject_denied" in result.stdout + result.stderr, \
            result.stdout + result.stderr
        assert pty_alive(loner), "foreign subject terminated the session"
        os.write(loner_master, b"still-here\n")
        assert pty_alive(loner), "session stopped after the foreign close attempt"
        print(f"close: foreign subject refused; session={target_session} still live", flush=True)
        write_policy([owner_id])
        result = control(owner, "--pty-close", target_session)
        assert result.returncode == 0, result.stdout + result.stderr
        pty_wait(loner)
        os.close(loner_master)
        print(f"close: owner closed session={target_session}", flush=True)

        case("a detached session is reclaimed when its lease runs out")
        detacher, detach_master = pty_client(owner_key, "import time\nprint('ready', flush=True)\n"
                                                    "time.sleep(120)\n", ttl=1500)
        assert "ready" in read_until(detach_master, "ready"), "detacher never became ready"
        detached = re.findall(r"iroh pty opened session=(\S+)", daemon_log.read_text())[-1]
        result = control(client_profile("owner-short", owner_key, lease_ttl_ms=1500),
                         "--pty-detach", detached)
        assert result.returncode == 0, result.stdout + result.stderr
        pty_wait(detacher)
        os.close(detach_master)
        assert "lease_expired" not in daemon_log.read_text(), "reclaimed before the deadline"
        until = time.monotonic() + 15
        while time.monotonic() < until:
            if f"lease reclaim session={detached} reason=lease_expired" in \
                    daemon_log.read_text():
                break
            time.sleep(0.05)
        else:
            raise AssertionError("detached session was not reclaimed by its lease deadline")
        print(f"expiry: detached session={detached} reclaimed without a later request", flush=True)

        case("the operator lists the admitted parties with their fingerprint")
        listed = policy_cli("list")
        assert listed.returncode == 0, listed.stdout + listed.stderr
        assert owner_id in listed.stdout and "fingerprint=" in listed.stdout, listed.stdout
        print(f"policy: list -> {listed.stdout.strip().splitlines()[0]}", flush=True)

        case("graceful revocation refuses new sessions and keeps the live one")
        write_policy([owner_id])
        keeper, keeper_master = pty_client(owner_key, "import time\nprint('ready', flush=True)\n"
                                                  "time.sleep(30)\n")
        assert "ready" in read_until(keeper_master, "ready"), "keeper never became ready"
        revoked_with_cli = policy_cli("revoke", "--subject", owner_id, "--note", "rai-305 closeout")
        assert revoked_with_cli.returncode == 0, revoked_with_cli.stdout + revoked_with_cli.stderr
        listed = policy_cli("list")
        assert "revoked" in listed.stdout and "rai-305 closeout" in listed.stdout, listed.stdout
        print(f"policy: revoke -> {revoked_with_cli.stdout.strip()}", flush=True)
        result = control(owner, "@ping#9")
        combined = result.stdout + result.stderr
        assert result.returncode != 0 and "iroh_target_denied" in combined, combined
        assert pty_alive(keeper), "graceful revocation terminated the live session"
        print("revocation: graceful refused a new session; live session kept", flush=True)

        case("emergency revocation terminates the live session")
        emergency = policy_cli("emergency", "--subject", owner_id)
        assert emergency.returncode == 0, emergency.stdout + emergency.stderr
        print(f"policy: emergency -> {emergency.stdout.strip()}", flush=True)
        until = time.monotonic() + 15
        while time.monotonic() < until and pty_alive(keeper):
            time.sleep(0.05)
        assert not pty_alive(keeper), "emergency marker did not terminate the session"
        pty_wait(keeper)
        os.close(keeper_master)
        assert "reason=lease_revoked" in daemon_log.read_text(), daemon_log.read_text()
        print("revocation: emergency terminated the live session", flush=True)

        case("a restarted daemon still refuses the revoked party")
        first_daemon_log = daemon_log
        daemon_process = next(process for process in processes if "daemon" in process.args)
        processes.remove(daemon_process)
        daemon_process.terminate()
        daemon_process.wait(timeout=20)
        daemon_log = start("daemon", ["daemon", "--transport", "iroh", "-c", daemon],
                           "iroh daemon registered pty=admitted")
        after_restart = control(owner, "--pty", "--", "/bin/sh", "-c", "sleep 5")
        combined = after_restart.stdout + after_restart.stderr
        assert after_restart.returncode != 0 and "iroh_target_denied" in combined, combined
        print("policy: a restarted daemon still refuses the revoked party", flush=True)

        write_policy([owner_id])
        assert "path=direct" in first_daemon_log.read_text(), \
            "the first daemon never recorded a direct session"
        if from_lan:
            if os.environ.get("RAI305_STALL"):
                outage = int(os.environ["RAI305_STALL"])
                burst = int(os.environ.get("RAI305_BURST", "0"))
                case(f"a {outage}s stall either keeps the session or ends it for real")
                program = "import sys\nprint('ready', flush=True)\n"
                if burst:
                    # A frozen reader only stalls the peer once the window is full.
                    program += (f"for line in sys.stdin:\n"
                                f"    sys.stdout.write('x' * {burst * 1000000})\n"
                                f"    sys.stdout.flush()\n"
                                f"    print('burst-done', flush=True)\n")
                else:
                    program += ("for line in sys.stdin:\n"
                                "    print('echo:' + line.strip(), flush=True)\n")
                echo, echo_master = pty_client(owner_key, program)
                assert "ready" in read_until(echo_master, "ready"), "stall session never became ready"
                stalled = re.findall(r"iroh pty opened session=(\S+)", daemon_log.read_text())[-1]
                if burst:
                    os.kill(echo, signal.SIGSTOP)
                else:
                    subprocess.run(["ip", "link", "set", "ca", "down"], check=True)
                try:
                    os.write(echo_master, b"during-stall\n")
                    time.sleep(outage)
                finally:
                    if burst:
                        os.kill(echo, signal.SIGCONT)
                    else:
                        subprocess.run(["ip", "link", "set", "ca", "up"], check=True)
                needle = "burst-done" if burst else "echo:during-stall"
                text = read_until(echo_master, needle, budget=40)
                frame_broken = "iroh_frame_broken" in daemon_log.read_text()
                control(owner, "--pty-close", stalled)
                try:
                    code = pty_wait(echo, budget=25)
                except AssertionError:
                    os.write(echo_master, b"\x04")
                    code = pty_wait(echo, budget=25)
                os.close(echo_master)
                survived = needle in text and code == 0
                print(f"stall: session={stalled} outage={outage}s burst={burst}MB "
                      f"survived={survived} frame_broken={frame_broken} exit={code}", flush=True)
                if frame_broken:
                    reason_line = next(
                        (line for line in daemon_log.read_text().splitlines()
                         if "iroh_frame_broken" in line), "")
                    print(f"stall: daemon recorded {reason_line.strip()[:160]}", flush=True)
                assert survived or frame_broken, \
                    f"the session ended without a frame reason or did not survive: {code}"
            case("a relay-only profile also reaches the daemon")
            relay_only = client_profile("relay-only", owner_key, path="relay")
            started = time.monotonic()
            relayed = control(relay_only, "@ping#10")
            print(f"relay: dedicated relay profile took {time.monotonic() - started:.1f}s",
                  flush=True)
            combined = relayed.stdout + relayed.stderr
            assert relayed.returncode == 0, combined
            assert "iroh_path=relay" in combined, combined
            print("relay: a relay-only profile reached the daemon", flush=True)

            case("an interactive session falls back to the relay and records that path")
            direct_only = client_profile("fallback-direct", owner_key, path="direct")
            blocked = subprocess.run(["ip", "route", "replace", "blackhole",
                                      f"{routable['daemon']}/32"], capture_output=True)
            assert blocked.returncode == 0, blocked.stderr
            try:
                started = time.monotonic()
                blocked_relay = control(relay_only, "@ping#15", timeout=120)
                print(f"fallback: relay-only with the direct path blocked took "
                      f"{time.monotonic() - started:.1f}s exit={blocked_relay.returncode}",
                      flush=True)
                refused = control(direct_only, "@ping#11")
                assert refused.returncode != 0, \
                    "a direct-only profile must fail while the direct path is blocked"
                auto = client_profile("fallback-auto", owner_key, path="auto")
                started = time.monotonic()
                result = control(auto, "@ping#12", timeout=120)
                print(f"fallback: auto finished in {time.monotonic() - started:.1f}s", flush=True)
                combined = result.stdout + result.stderr
                assert result.returncode == 0, combined
                assert "iroh_path=relay" in combined, combined
                assert "iroh_path=direct unavailable" in combined, combined
                print("fallback: direct blocked -> relay carried the session; iroh_path=relay",
                      flush=True)
            finally:
                subprocess.run(["ip", "route", "del", f"{routable['daemon']}/32"],
                               capture_output=True)
            assert "path=direct" in daemon_log.read_text(), "the daemon never recorded the path"
        assert "iroh_path=direct" in dialled, "the client never reported the path it used"
        alive = [pid for pid in fork_pids if pty_alive(pid)]
        assert not alive, f"forks still alive: {alive}"
        print("harness: all cases passed", flush=True)
    except BaseException:
        if (home / "pty-client.log").exists():
            print("--- pty-client.log ---", flush=True)
            print((home / "pty-client.log").read_text(errors="replace")[-2000:], flush=True)
        for name in ("daemon", "rendezvous"):
            logfile = home / f"{name}.log"
            if logfile.exists():
                print(f"--- {name}.log ---", flush=True)
                print(logfile.read_text(errors="replace")[-4000:], flush=True)
        raise
    finally:
        for pid in fork_pids:
            try:
                os.kill(pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
            try:
                pty_wait(pid, budget=5)
            except (AssertionError, ChildProcessError):
                os.kill(pid, signal.SIGKILL)
        for child in reversed(processes):
            if child.poll() is None:
                os.killpg(child.pid, signal.SIGTERM)
                try:
                    child.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    os.killpg(child.pid, signal.SIGKILL)
                    child.wait(timeout=5)
        print("cleanup: owned subprocesses reaped", flush=True)
assert not home.exists()
print("cleanup: temporary HOME and identities removed")
