import errno
import json
import os
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


def ready(process, logfile, marker):
    until = time.monotonic() + 12
    while time.monotonic() < until:
        text = logfile.read_text(errors="replace")
        if process.poll() is not None:
            raise RuntimeError(f"{marker}: process exited {process.returncode}: {text[-1800:]}")
        if marker in text:
            return
        time.sleep(0.04)
    raise RuntimeError(f"{marker}: timed out: {text[-1800:]}")


def run(binary_args, **kwargs):
    return subprocess.run([binary, *binary_args], **kwargs)


with tempfile.TemporaryDirectory(prefix="iroh-cli-", dir=scratch) as dirname:
    home = Path(dirname)
    env = dict(PATH="/usr/bin:/bin", HOME=dirname, TMPDIR=dirname, LANG="C.UTF-8")
    rendezvous_key, rendezvous_id = identity(home, "rendezvous")
    daemon_key, daemon_id = identity(home, "daemon")
    allowed_key, allowed_id = identity(home, "allowed")
    denied_key, denied_id = identity(home, "denied")
    cert, key = home / "relay.pem", home / "relay-key.pem"
    subprocess.run(
        ["openssl", "req", "-x509", "-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:P-256",
         "-nodes", "-subj", "/CN=localhost", "-addext", "subjectAltName=IP:127.0.0.1",
         "-addext", "basicConstraints=critical,CA:FALSE",
         "-days", "1", "-keyout", key, "-out", cert],
        env=env, capture_output=True, check=True, timeout=5,
    )
    sockets = [socket.socket(socket.AF_INET, kind) for kind in
               [socket.SOCK_DGRAM, socket.SOCK_STREAM, socket.SOCK_STREAM, socket.SOCK_DGRAM]]
    for sock in sockets:
        sock.bind(("127.0.0.1", 0))
    directory, http, https, qad = [sock.getsockname()[1] for sock in sockets]
    for sock in sockets:
        sock.close()

    target = "lab/readonly"
    addr = lambda port: f"127.0.0.1:{port}"
    directory_config = dict(id=rendezvous_id, addr=addr(directory))
    relay = dict(url=f"https://{addr(https)}", qad_port=qad, root_cert=str(cert))

    def config(label, **values):
        result = str(home / f"{label}.json")
        Path(result).write_text(json.dumps(values))
        return result

    rendezvous = config("rendezvous", key_file=rendezvous_key, bind=addr(directory),
                        relay_http=addr(http), relay_https=addr(https), qad=addr(qad),
                        relay_cert=str(cert), relay_key=str(key),
                        registrations={target: daemon_id}, lookups=[allowed_id, denied_id])
    daemon = config("daemon", key_file=daemon_key, directory=directory_config, relay=relay,
                    name=target, allow=[allowed_id])
    processes = []

    def start(name, flags, marker):
        logfile = home / f"{name}.log"
        with logfile.open("w") as output:
            child = subprocess.Popen([binary, *flags], cwd=home, env=env, stdin=subprocess.DEVNULL,
                                     stdout=output, stderr=subprocess.STDOUT, start_new_session=True)
        processes.append(child)
        ready(child, logfile, marker)
        print(f"{name}: ready", flush=True)
        return logfile

    def ping(path, key_path, passes):
        client = config("client", key_file=key_path, directory=directory_config,
                        relay=relay, path=path)
        command = ["control", "--transport", "iroh", "--iroh-config", client, target, "@ping#42"]
        result = run(command, cwd=home, env=env, capture_output=True, text=True, timeout=15)
        combined = result.stdout + result.stderr
        if passes:
            responses = [json.loads(line.removeprefix("@response "))
                         for line in result.stdout.splitlines() if line.startswith("@response ")]
            assert result.returncode == 0, combined
            assert len(responses) == 1 and responses[0]["id"] == 42, combined
            assert responses[0]["value"]["pong"] is True, combined
        else:
            assert result.returncode != 0 and "iroh_target_denied" in combined, combined
            assert "@response" not in result.stdout and '"pong":true' not in result.stdout, combined
        assert f"iroh_path={path}" in combined, combined
        outcome = "allowed id=42 pong=true" if passes else "iroh_target_denied; no pong"
        print(f"client iroh_path={path}: {outcome}; exit={result.returncode}")

    try:
        start("rendezvous", ["rendezvous", "-c", rendezvous],
              f"iroh rendezvous ready relay_https={addr(https)} qad={addr(qad)}")
        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as probe:
            try:
                probe.bind(("127.0.0.1", qad))
            except OSError as error:
                assert error.errno == errno.EADDRINUSE, error
            else:
                raise AssertionError("rendezvous did not bind its QAD UDP port")
        print("rendezvous: embedded HTTPS relay ready; QAD UDP bind confirmed")
        daemon_log = start("daemon", ["daemon", "--transport", "iroh", "-c", daemon],
                           "iroh daemon registered")
        for path in ("direct", "relay"):
            ping(path, allowed_key, True)
            ping(path, denied_key, False)
            text = daemon_log.read_text()
            assert text.count(f"iroh admission deny path={path} handler_entries=0") == 1, text
            assert text.count(f"iroh ping dispatch path={path} id=42") == 1, text
            print(f"daemon: iroh ping dispatch path={path} id=42")
            print(f"daemon: iroh admission deny path={path} handler_entries=0")
        client = config("client", key_file=allowed_key, directory=directory_config,
                        relay=relay, path="direct")
        for name, line, reason in (
            ("lab/not_registered", "@ping#42", "iroh_target_not_registered"),
            (target, "@capabilities", "iroh_readonly_ping_required"),
        ):
            result = run(["control", "--transport", "iroh", "--iroh-config", client, name, line],
                         cwd=home, env=env, capture_output=True, text=True, timeout=15)
            assert result.returncode != 0 and reason in result.stderr and not result.stdout, result
            print(f"client: {reason}; exit={result.returncode}")
        ping("direct", allowed_key, True)
        assert all(child.poll() is None for child in processes)
        print("renegotiated direct session reached the original running daemon")
    finally:
        for child in reversed(processes):
            if child.poll() is None:
                os.killpg(child.pid, signal.SIGTERM)
                try:
                    child.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    os.killpg(child.pid, signal.SIGKILL)
                    child.wait(timeout=5)
            assert not Path(f"/proc/{child.pid}").exists()
        print("cleanup: owned subprocesses reaped")
    for port in (http, https):
        with socket.socket() as probe:
            assert probe.connect_ex(("127.0.0.1", port)) == errno.ECONNREFUSED
    for port in (directory, qad):
        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as probe:
            probe.bind(("127.0.0.1", port))
    print("cleanup: relay TCP and directory/QAD UDP ports released")
assert not home.exists()
print("cleanup: temporary HOME and identities removed")
