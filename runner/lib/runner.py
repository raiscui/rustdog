"""eval-macos-ops main runner.

ponytail: 1 file, no abstractions for "future extensions". If you need to
generalize this for cross-model eval, copy this file.

drive model x case matrix:
1. start managed local-default daemon (target/debug/rdog)
2. for each (model, case):
   - invoke Pi (via `pi --provider X --model Y --api-key K --extension mano_cua_rdog.mjs`)
   - parse Pi session JSONL for bash commands + tool results
   - feed each bash command into interaction_ledger
   - maxCaseAttempts retry on classification=recovery / outcome=recoverable
3. stop daemon
4. emit summary.json + manifest
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

# When invoked as `python3 -m runner.lib.runner`, sys.path[0] is repo_root,
# so we need to explicitly add runner/lib/ for sibling-imports to work.
_LIB_DIR = Path(__file__).resolve().parent
if str(_LIB_DIR) not in sys.path:
    sys.path.insert(0, str(_LIB_DIR))

# local import: runner/lib/ is now on sys.path
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
from dataclasses import asdict, dataclass
from pathlib import Path

# local import: runner/lib/ is added to sys.path by eval-macos-ops.sh
from interaction_ledger import BashDecision, InteractionLedger


SCHEMA = "rdog.macos-ops.interaction-ledger.v1"
RUNNER_VERSION = "0.1.0"


@dataclass
class CaseRunResult:
    model_id: str
    case_id: str
    attempts: int
    success: bool
    final_state: str
    ledger_summary: dict
    error: str = ""


def _sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    h.update(path.read_bytes())
    return h.hexdigest()


def _resolve_binary(repo_root: Path, config: dict) -> tuple[Path, str]:
    """Resolve current rdog binary + compute sha256 for provenance gate."""
    bin_path = repo_root / config["rdogBinary"]["path"]
    if not bin_path.exists():
        raise FileNotFoundError(f"current rdog binary missing: {bin_path}")
    return bin_path, _sha256_file(bin_path)


def _build_case_prompt(case: dict) -> str:
    """Compose the task prompt fed to Pi for one case attempt.

    Adds explicit @computer-act protocol requirement + scoring rule so models
    do not fall back to direct verbs (@open-app / @ax-press etc.), which bypass
    the @computer-act envelope and therefore skip outcome 三态.

    The requirement block is placed BEFORE the task so the model sees it first.
    5x8 baseline (commit 5c7b9a6) showed deepseek + minimax-cn 全 0/8 despite
    a system_prompt-level IMPORTANT hint — they consistently chose direct verbs.
    Putting the requirement in front of the task is the next iteration.
    """
    task = case['task']
    verify_desc = case['verify'].get('description', case.get('expectedResult', ''))
    expected = case.get('expectedResult', '')
    verify_method = case['verify'].get('method', 'unspecified')

    # ponytail: hard-constraint language ("不计分" / "硬约束") is the only lever
    # that flipped any model off direct verbs in 5x8 round 1. keep terse.
    requirement = """## Protocol requirement (硬约束 — 必须遵守)
所有 GUI 动作必须包在 `@computer-act#N:{...}` envelope 内 (schema: rdog.computer-act.v1).
直接调用 `@open-app` / `@ax-press` / `@key` / `@click` / `@ax-set-value` / `@type-text` 等 direct verb **不计分** (视为未完成).
模板: `rdog control @computer-act#1:'{schema:"rdog.computer-act.v1",action:"open_app",args:{app_name:"Calculator"},verify:"best_effort"}'`
action 可选: open_app / ax_press / ax_set_value / ax_action / ax_focus / type_text / key / click.
verify 选项: "none" / "best_effort" (默认) / "always" (强制 verify).
"""

    verify_block = f"""## Verify standard (完成后必做, 不通过算 fail)
verify 方法: {verify_method}
verify 标准: {verify_desc}
expected result: {expected}"""

    return f"{requirement}\n## Task\n{task}\n\n{verify_block}"


def _invoke_pi_rpc(*, provider_key: str, model: str, api_key: str,
                    extension_path: Path, system_prompt: str, prompt: str,
                    session_dir: Path, max_tool_iterations: int = 30,
                    tools: str = "bash", timeout_s: int = 90) -> tuple[int, list]:
    """Invoke Pi in RPC mode (headless, no TTY required).

    RPC schema (from src/rpc.rs in pi_agent_rust):
    - Request: {"type":"prompt", "id":"...", "message":"..."}
    - Response: {"type":"response","command":"prompt","success":true/false,"id":"..."}
    - Events: AgentEvent tagged enum serialized line-by-line to stdout
      (agent_start, turn_start, message_start/update/end, tool_execution_start/end, agent_end)

    ponytail: pass prompt as JSON `message` field, NOT via argv — argv triggers
    Pi argparse to mis-parse any '- ' / '--' token. RPC mode also doesn't accept
    @file references (per src/rpc.rs: "@file arguments are not supported in rpc mode").

    Returns (exit_code, parsed_events). Events include tool_execution_start/end
    for ledger extraction.

    Force models to use @computer-act so outcome 三态 fields are exercised.
    Without this hint, models prefer direct verbs (@ping, @window-find, @ax-find)
    which bypass @computer-act envelope and therefore outcome field.
    """
    # ponytail: case prompt now carries the explicit @computer-act requirement
    # (see _build_case_prompt). The system_prompt-level hint is removed because
    # 5x8 baseline (commit 5c7b9a6) showed models ignore it. Keep system_prompt
    # as-is so we do not regress unrelated profile-driven behavior.
    _force_computer_act_hint = system_prompt
    cmd = [
        str(shutil.which("pi") or "/Users/cuiluming/.cargo/bin/pi"),
        "--mode", "rpc",
        "--provider", provider_key,
        "--model", model,
        "--api-key", api_key,
        "--extension", str(extension_path),
        "--append-system-prompt", " " + _force_computer_act_hint if _force_computer_act_hint.startswith("-") else _force_computer_act_hint,
        "--no-session",
        "--max-tool-iterations", str(max_tool_iterations),
        "--tools", tools,
    ]
    request_line = json.dumps({"type": "prompt", "id": "eval-1", "message": prompt}) + "\n"
    try:
        proc = subprocess.run(
            cmd, input=request_line.encode(), capture_output=True,
            timeout=timeout_s,
        )
    except subprocess.TimeoutExpired:
        return 124, [{
            "type": "agent_end",
            "error": f"Pi call timed out after {timeout_s}s (max-tool-iterations={max_tool_iterations})",
            "messages": [],
        }]
    events = []
    for line in proc.stdout.decode(errors="replace").splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            events.append(json.loads(line))
        except json.JSONDecodeError:
            continue
    # Persist events + raw streams for post-mortem.
    (session_dir / "rpc_events.jsonl").write_text("\n".join(
        json.dumps(e) for e in events
    ))
    (session_dir / "rpc_stdout.txt").write_bytes(proc.stdout)
    (session_dir / "rpc_stderr.txt").write_bytes(proc.stderr)
    if proc.returncode != 0:
        import sys as _sys
        print(f"[{provider_key}] pi exit={proc.returncode}; stderr={proc.stderr.decode(errors='replace')[:300]}", file=_sys.stderr)
    return proc.returncode, events


def _parse_pi_session(session_dir: Path) -> Iterator[tuple[str, dict]]:
    """Yield (bash_command, tool_result) from Pi session JSONL files.
    Yields only decisions where tool_name is 'bash'."""
    jsonl_files = sorted(session_dir.glob("*.jsonl"))
    for path in jsonl_files:
        for line in path.read_text(errors="replace").splitlines():
            line = line.strip()
            if not line:
                continue
            try:
                entry = json.loads(line)
            except json.JSONDecodeError:
                continue
            # pi JSONL v2: entries carry type / role / content. Adapt per shape.
            if entry.get("type") == "tool_call" and entry.get("tool_name") == "bash":
                cmd = entry.get("input", {}).get("command", "")
                yield cmd, {"_stage": "tool_call", "_path": str(path)}
            elif entry.get("type") == "tool_result" and entry.get("tool_name") == "bash":
                yield "", {"_stage": "tool_result", "_path": str(path),
                            "_result": entry.get("content")}


def _resolve_model_meta(provider_key: str) -> tuple[str, str]:
    """Read ~/.pi/agent/models.json and return (provider_key, model_id) for Pi.

    ponytail: model id MUST be a provider-accepted id (e.g. deepseek-v4-flash).
    Passing the provider key directly (e.g. 'deepseek') fails with HTTP 400.
    Pick the first model entry under providers[X]['models'].
    """
    path = Path.home() / ".pi" / "agent" / "models.json"
    cfg = json.loads(path.read_text())
    provider = cfg["providers"][provider_key]
    models = provider.get("models", [])
    if models and isinstance(models[0], dict) and "id" in models[0]:
        return provider_key, models[0]["id"]
    return provider_key, provider.get("model", provider_key)


def _resolve_api_key(provider_cfg: dict) -> str:
    """Resolve apiKey field. supports 'env:VAR_NAME' env-var references.

    ponytail: defensive \r strip protects against CRLF pollution in .envrc
    (file edited on Windows leaves \r which direnv passes through verbatim).
    """
    raw = provider_cfg.get("apiKey", "")
    if raw.startswith("env:"):
        import os
        return os.environ.get(raw[4:], "").strip("\r")
    return raw.strip("\r")


def _run_one_case(*, repo_root: Path, config: dict, model_cfg: dict, case_id: str,
                  bin_path: Path, skill_dir: Path | None) -> CaseRunResult:
    case = json.loads((repo_root / "runner" / "cases" / f"{case_id}.json").read_text())
    provider_key, model_id = _resolve_model_meta(model_cfg["provider_key"])
    api_key = _resolve_api_key(
        json.loads((Path.home() / ".pi" / "agent" / "models.json").read_text())[
            "providers"][provider_key]
    )

    profile = json.loads((Path.home() / ".pi" / "agent" / "models.json").read_text())[
        "toolUseProfiles"][model_cfg["profile"]]
    system_prompt = profile.get("appendSystemPrompt", "")

    extension_path = Path(
        "/Users/cuiluming/local_doc/l_dev/my/rust/fast-infer/pi_extensions/"
        "mano_cua_rdog.mjs"
    )
    if not extension_path.exists():
        raise FileNotFoundError(f"Pi extension missing: {extension_path}")

    ledger = InteractionLedger(model_id=model_id, case_id=case_id)
    max_attempts = model_cfg.get("maxCaseAttempts", 3)
    final_success = False
    final_state = "not_attempted"
    error = ""

    for attempt in range(1, max_attempts + 1):
        ledger._attempt = attempt
        session_dir = Path(tempfile.mkdtemp(prefix=f"pi-{case_id}-", dir=tempfile.gettempdir()))
        rc, events = _invoke_pi_rpc(
            provider_key=provider_key, model=model_id, api_key=api_key,
            extension_path=extension_path, system_prompt=system_prompt,
            prompt=_build_case_prompt(case),
            session_dir=session_dir,
            max_tool_iterations=8,
        )

        # Walk events to extract tool calls + tool results, feed into ledger.
        # Pi RPC emits tool_execution_start {toolName, args} then tool_execution_end
        # {toolName, result, isError}. toolName is "bash" (after extension translation),
        # args.command is the actual `rdog control mac.lab ...` invocation.
        last_tool_start: dict | None = None
        for evt in events:
            etype = evt.get("type")
            if etype == "tool_execution_start":
                tool_name = evt.get("toolName", "")
                tool_args = evt.get("args", {})
                if tool_name == "bash":
                    cmd = tool_args.get("command", "")
                    if cmd:
                        last_tool_start = {"command": cmd, "ts": evt}
                        ledger.record(cmd, artifact_path=str(session_dir / "rpc_events.jsonl"))
            elif etype == "tool_execution_end":
                if last_tool_start is not None:
                    cmd = last_tool_start["command"]
                    last_tool_start = None
                    res = evt.get("result", "")
                    if isinstance(res, list):
                        text = "".join(
                            b.get("text", "") for b in res if isinstance(b, dict)
                        )
                    else:
                        text = str(res)
                    is_err = evt.get("isError", False)
                    ledger.record(
                        "", rdog_response=text[:500],
                        rdog_error=("tool_error" if is_err else ""),
                    )

        # crude success heuristic: at least one action classified + no recovery tail
        summary = ledger.summary()
        has_actions = summary["by_class"].get("action", 0) >= 1
        recoveries = summary["by_class"].get("recovery", 0)
        if has_actions and recoveries == 0 and rc == 0:
            final_success = True
            final_state = "success"
            break
        elif attempt == max_attempts:
            final_state = "max_attempts_exceeded"
            error = f"rc={rc}, recoveries={recoveries}"
            break
        else:
            final_state = "retry_pending"

    return CaseRunResult(
        model_id=model_id,
        case_id=case_id,
        attempts=attempt,
        success=final_success,
        final_state=final_state,
        ledger_summary=ledger.summary(),
        error=error,
    )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--config", default="runner/config.json")
    parser.add_argument("--mode", choices=["dry", "live"], default="dry",
                        help="dry: skip Pi invocation, emit skeleton ledger. "
                             "live: actually invoke Pi for each (model, case).")
    parser.add_argument("--models", default=None, help="comma-separated model ids")
    parser.add_argument("--cases", default=None, help="comma-separated case ids")
    parser.add_argument("--output", default=None, help="output directory (default: tmp)")
    args = parser.parse_args()

    repo_root = Path(__file__).resolve().parents[2]
    config = json.loads((repo_root / args.config).read_text())
    bin_path, bin_sha256 = _resolve_binary(repo_root, config)

    models = config["models"]
    cases = config["cases"]
    if args.models and args.models != "all":
        models = [m for m in models if m["id"] in args.models.split(",")]
    if args.cases and args.cases != "all":
        cases = [c for c in cases if c in args.cases.split(",")]

    output_dir = Path(args.output) if args.output else Path(tempfile.mkdtemp(prefix="rdog-eval-"))
    output_dir.mkdir(parents=True, exist_ok=True)

    manifest = {
        "schema": SCHEMA,
        "runner_version": RUNNER_VERSION,
        "rustdog_commit": subprocess.check_output(
            ["git", "rev-parse", "HEAD"], cwd=repo_root
        ).decode().strip(),
        "rdogBinary_path": str(bin_path),
        "rdogBinary_sha256": bin_sha256,
        "skill_sha256": _sha256_file(repo_root / ".codex" / "skills" / "rdog-control" / "SKILL.md"),
        "models": [m["id"] for m in models],
        "cases": cases,
        "immutableBaseline": config["immutableBaseline"],
    }
    (output_dir / "manifest.json").write_text(json.dumps(manifest, indent=2))

    if args.mode == "dry":
        print(f"DRY RUN: would run {len(models)} models x {len(cases)} cases = {len(models) * len(cases)} attempts")
        print(f"output dir: {output_dir}")
        print(f"manifest: {output_dir / 'manifest.json'}")
        return 0

    # live mode: start daemon, run matrix, stop daemon
    from daemon_manager import start_local_default_daemon, stop_daemon
    daemon_pid, daemon_log = start_local_default_daemon(repo_root)
    print(f"daemon pid={daemon_pid} log={daemon_log}", file=sys.stderr)
    try:
        all_results: list[CaseRunResult] = []
        for model in models:
            for case in cases:
                case_dir = output_dir / model["id"] / case
                case_dir.mkdir(parents=True, exist_ok=True)
                try:
                    result = _run_one_case(
                        repo_root=repo_root, config=config,
                        model_cfg=model, case_id=case,
                        bin_path=bin_path, skill_dir=None,
                    )
                except Exception as e:
                    result = CaseRunResult(
                        model_id=model["id"], case_id=case, attempts=0,
                        success=False, final_state="runner_error",
                        ledger_summary={"error": str(e)}, error=str(e),
                    )
                (case_dir / "result.json").write_text(
                    json.dumps(asdict(result), indent=2)
                )
                all_results.append(result)
                print(f"  {model['id']:30s} {case:35s} -> success={result.success} attempts={result.attempts}", file=sys.stderr)

        # suite summary
        suite = {
            "schema": SCHEMA,
            "manifest": manifest,
            "results": [asdict(r) for r in all_results],
            "totals": {
                "models": len(models),
                "cases": len(cases),
                "successful": sum(1 for r in all_results if r.success),
                "total_attempts": sum(r.attempts for r in all_results),
                "agent_decisions": sum(r.ledger_summary.get("agent_decisions", 0) for r in all_results),
                "rdog_requests": sum(r.ledger_summary.get("rdog_requests", 0) for r in all_results),
            },
        }
        (output_dir / "suite-result.json").write_text(json.dumps(suite, indent=2))
        print(f"\nsuite summary written: {output_dir / 'suite-result.json'}", file=sys.stderr)
        return 0 if all(r.success for r in all_results) else 1
    finally:
        stop_daemon(daemon_pid)
        print(f"daemon stopped (pid={daemon_pid})", file=sys.stderr)


if __name__ == "__main__":
    sys.exit(main())
