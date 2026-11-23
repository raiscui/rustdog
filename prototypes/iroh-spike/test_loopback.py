#!/usr/bin/env python3
import json
import socket
import subprocess
import sys
import tempfile
import time
from pathlib import Path

binary = str(Path(sys.argv[1]).resolve())
with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
    sock.bind(("127.0.0.1", 0))
    port = sock.getsockname()[1]
with tempfile.TemporaryDirectory() as tmp:
    node_log = Path(tmp) / "node.log"
    with node_log.open("w+") as log:
        node = subprocess.Popen([binary, "node", "--bind-addr", f"127.0.0.1:{port}", "--relay", "http://127.0.0.1:9/"], stdout=subprocess.PIPE, stderr=log, text=True)
        try:
            import select
            if not select.select([node.stdout], [], [], 15)[0]:
                raise RuntimeError("node startup timeout")
            info = json.loads(node.stdout.readline())
            target = json.dumps({"id": info["id"], "addrs": [{"Ip": f"127.0.0.1:{port}"}]})
            started = time.monotonic()
            result = subprocess.run([binary, "client", "--relay", "http://127.0.0.1:9/", "--target", target, "--bytes", "1024", "--linger", "3"], capture_output=True, text=True, timeout=20)
            print(result.stdout, result.stderr, sep="\n")
            assert time.monotonic() - started >= 3, "linger ended early"
            assert result.returncode == 0, f"client exit {result.returncode}"
            assert "exchanges=2" in result.stdout, "two exact echoes missing"
            assert "ApplicationClosed" not in result.stderr, "connection closed during linger"
        finally:
            node.terminate()
            try:
                node.wait(timeout=5)
            except subprocess.TimeoutExpired:
                node.kill()
                node.wait(timeout=5)
            log.seek(0)
            print(log.read(), file=sys.stderr)

for size in ("0", str(8 * 1024 * 1024 + 1)):
    result = subprocess.run([binary, "client", "--bytes", size], capture_output=True, text=True, timeout=10)
    assert result.returncode != 0 and "--bytes" in result.stderr, "invalid size accepted"
