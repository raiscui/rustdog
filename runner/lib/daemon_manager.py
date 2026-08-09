"""Managed local-default daemon lifecycle for macOS ops eval matrix.

启动 current target/debug/rdog daemon + rdog_macos.toml, 跟 host 上其他 daemon 隔离.
runner 跑完 5x8 matrix 后 kill daemon, 恢复环境.

ponytail: minimal wrapper, 不做 health check (rdog daemon 自己 log ready line).
"""

from __future__ import annotations

import os
import signal
import subprocess
import sys
import tempfile
import time
from pathlib import Path


def start_local_default_daemon(repo_root: Path, config_name: str = "rdog_macos.toml") -> tuple[int, Path]:
    """Start a managed local-default daemon. Returns (pid, log_path)."""
    binary = repo_root / "target" / "debug" / "rdog"
    config = repo_root / config_name
    if not binary.exists():
        raise FileNotFoundError(f"rdog binary not found: {binary} (run cargo build first)")
    if not config.exists():
        raise FileNotFoundError(f"config not found: {config}")

    tmp_dir = Path(tempfile.mkdtemp(prefix="rdog-eval-macos-ops-", dir=tempfile.gettempdir()))
    log_path = tmp_dir / "daemon.log"

    env = os.environ.copy()
    # Prepend target/debug to PATH so any Pi-spawned `rdog` resolves to current binary.
    env["PATH"] = f"{binary.parent}{os.pathsep}{env.get('PATH', '')}"

    proc = subprocess.Popen(
        [str(binary), "daemon", "-c", str(config)],
        cwd=repo_root,
        stdout=log_path.open("wb"),
        stderr=subprocess.STDOUT,
        env=env,
        start_new_session=True,
    )

    # Wait for ready signal (zenoh router ready log line).
    deadline = time.time() + 15.0
    while time.time() < deadline:
        if proc.poll() is not None:
            raise RuntimeError(
                f"daemon exited early (rc={proc.returncode}); see {log_path}\n"
                f"{log_path.read_text(errors='replace')}"
            )
        if log_path.exists() and "zenoh router daemon ready:" in log_path.read_text(errors="replace"):
            return proc.pid, log_path
        time.sleep(0.2)

    proc.kill()
    raise RuntimeError(f"daemon did not become ready within 15s; see {log_path}")


def stop_daemon(pid: int) -> None:
    """Stop the managed daemon. SIGTERM first, SIGKILL after 3s fallback."""
    try:
        os.killpg(os.getpgid(pid), signal.SIGTERM)
    except ProcessLookupError:
        return
    deadline = time.time() + 3.0
    while time.time() < deadline:
        try:
            os.kill(pid, 0)
        except ProcessLookupError:
            return
        time.sleep(0.1)
    try:
        os.killpg(os.getpgid(pid), signal.SIGKILL)
    except ProcessLookupError:
        pass


if __name__ == "__main__":
    repo = Path(sys.argv[1]) if len(sys.argv) > 1 else Path.cwd()
    pid, log = start_local_default_daemon(repo)
    print(f"daemon pid={pid}, log={log}", file=sys.stderr)
    try:
        time.sleep(2)
    finally:
        stop_daemon(pid)
        print("daemon stopped", file=sys.stderr)
