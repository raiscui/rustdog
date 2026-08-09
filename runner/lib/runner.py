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
import re
import signal
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
import struct
import subprocess
import sys
import tempfile
import time
import unicodedata
import zlib
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
    checks: dict
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

    2026-08-09 bisect 验证: archive 40/40 的组合是 "skill 契约 + 纯 task"
    (外部 runner 把 skill 全文嵌入 system prompt, case prompt 是纯 task)。
    仓库内 runner 的 profile system prompt 只有 4 行基础规则, 所以把
    skill 关键契约放进 case prompt 开头 (等价于外部 runner 的嵌入效果)。

    不保留 v2 的 @computer-act 硬约束: 移植严格验证后计分不看 envelope,
    archive 风格下 deepseek 0-1 decisions/case 高效完成 (bisect Step 1 证据)。
    """
    task = case["task"]
    expected = case.get("expectedResult", "")
    verify_desc = case.get("expected", expected)

    # skill 契约摘要: 来自外部 runner system-prompt-with-skill.md 前言 +
    # 当前 SKILL.md 核心规则 (fresh 验证 / 不编造 / 失败先验证状态)。
    contract = """## Protocol contract (必须遵守)
你是本地 macOS GUI 控制 agent。
通过 bash 工具运行原始 `rdog control` 命令完成请求, 每次调用一个 rdog 进程。
不要使用 shell 管道、链式命令、命令替换、坐标或 raw mouse fallback。
持续在同一 agent loop 内工作, 直到 fresh 结构化 rdog 输出证明真实 GUI 结果;
成功的 action 响应不是最终证据。
遇到权限错误、窗口歧义、stale target 或不可读结果时停止, 先验证状态再行动。
不要编造命令、id、stdout、应用状态或成功。
只有真正读到 GUI 结果后才简短回答。"""

    verify_block = f"""## Verify standard (完成后必须满足)
期望结果: {expected}
验证标准: {verify_desc}"""

    return f"{contract}\n## Task\n{task}\n\n{verify_block}"


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


# ---------------------------------------------------------------------------
# macOS app 状态捕获 + case prepare/verify
# 移植自外部 runner run_macos_ops_eval.py, 让评测判定对齐 archive 40/40
# 的严格验证语义 (fresh window/AX 证据 + expected result 断言)。
# ---------------------------------------------------------------------------

_ANSI_RE = re.compile(r"\x1b\[[0-9;]*m")
_READ_KINDS = {"ax-find", "ax-get", "ax-tree", "observe", "window-find"}


@dataclass
class _ProcessResult:
    """轻量子进程结果 (带 timed_out 标志, 用于超时判定)。"""
    args: list[str]
    returncode: int
    stdout: str
    stderr: str
    timed_out: bool = False


def _run_process(args: list[str], *, cwd: Path, timeout_s: int) -> _ProcessResult:
    """运行进程并在超时时终结整个进程组,避免遗留子进程。"""
    proc = subprocess.Popen(
        list(args), cwd=cwd, text=True,
        stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        start_new_session=True,
    )
    timed_out = False
    try:
        stdout, stderr = proc.communicate(timeout=timeout_s)
    except subprocess.TimeoutExpired:
        timed_out = True
        os.killpg(proc.pid, signal.SIGTERM)
        try:
            stdout, stderr = proc.communicate(timeout=5)
        except subprocess.TimeoutExpired:
            os.killpg(proc.pid, signal.SIGKILL)
            stdout, stderr = proc.communicate()
    return _ProcessResult(
        args=list(args),
        returncode=proc.returncode,
        stdout=stdout,
        stderr=stderr,
        timed_out=timed_out,
    )


def _extract_rdog_responses(text: str) -> list:
    """从完整 stdout 提取所有 @response,不依赖最后一行或 shell 管道。"""
    responses: list = []
    for line in text.splitlines():
        clean = _ANSI_RE.sub("", line)
        if not clean.startswith("@response "):
            continue
        payload = clean[len("@response "):]
        try:
            responses.append(json.loads(payload))
        except json.JSONDecodeError:
            responses.append(payload)
    return responses


def _extract_tool_result_text(result) -> str:
    """从 Pi RPC tool_execution_end 的 result 里提取纯文本。

    RPC 实际格式: {"content": [{"type": "text", "text": "..."}], "details": null}
    部分老版本是裸 list, 两种形态都兼容。
    """
    if isinstance(result, dict):
        content = result.get("content")
        if isinstance(content, list):
            return "".join(
                b.get("text", "") for b in content
                if isinstance(b, dict) and b.get("type") == "text"
            )
        return str(result)
    if isinstance(result, list):
        return "".join(
            b.get("text", "") for b in result if isinstance(b, dict)
        )
    return str(result)


def _run_rdog_frame(bin_path: Path, frame: str, cwd: Path, timeout_s: int = 60):
    """执行一个原始 rdog control frame,返回最后一个 @response。"""
    cwd.mkdir(parents=True, exist_ok=True)
    result = _run_process([str(bin_path), "control", frame], cwd=cwd, timeout_s=timeout_s)
    responses = _extract_rdog_responses(result.stdout)
    if result.timed_out or result.returncode != 0 or not responses:
        raise RuntimeError(f"rdog 失败: {frame[:60]}..., exit={result.returncode}, responses={len(responses)}")
    response = responses[-1]
    if isinstance(response, dict) and (response.get("error") or response.get("code") not in (None, 0)):
        raise RuntimeError(f"rdog 返回错误: {response}")
    return response


def _response_value(response) -> dict:
    """只接受带对象 value 的查询响应。"""
    value = response.get("value") if isinstance(response, dict) else None
    if not isinstance(value, dict):
        raise RuntimeError("rdog 查询缺少对象 value")
    return value


def _normalize_ax_text(value) -> str:
    """移除 AX 文本中的不可见方向控制字符,并去掉首尾空白。"""
    text = unicodedata.normalize("NFKC", str(value or ""))
    text = "".join(char for char in text if unicodedata.category(char) != "Cf")
    return text.strip()


def _app_window(bin_path: Path, app_name: str, cwd: Path, *, allow_missing: bool):
    """返回 app 的第一扇可交互窗口,缺失时按合同处理。"""
    response = _run_rdog_frame(
        bin_path,
        f'@window-find#2101:{{app:{json.dumps(app_name)},limit:10,include_state:true,include_recipes:false}}',
        cwd,
    )
    value = _response_value(response)
    matches = [
        window for window in (value.get("matches") or [])
        if (window.get("state") or {}).get("interactable") is True
    ]
    if not matches:
        if allow_missing:
            return None
        raise RuntimeError(f"缺少可交互窗口: {app_name}")
    return matches[0]


def _capture_app_state(bin_path: Path, case: dict, cwd: Path, prefix: str, *, allow_missing: bool) -> dict:
    """按 case 的 verify 类型捕获 fresh 窗口与 AX 文本状态。"""
    verify = case["verify"]
    window = _app_window(bin_path, case["app"], cwd, allow_missing=allow_missing)
    state: dict = {
        "exists": window is not None,
        "windowTitle": (window or {}).get("title") or "",
        "values": [],
    }
    if verify == "window-count-increase":
        # 窗口数验证 (菜单新建场景): 统计 app 全部可交互窗口数。
        response = _run_rdog_frame(
            bin_path,
            f'@window-find#2103:{{app:{json.dumps(case["app"])},limit:20,include_state:true,include_recipes:false}}',
            cwd,
        )
        value = _response_value(response)
        state["windowCount"] = len(value.get("matches") or [])
        return state
    if window is None:
        return state
    window_id = window["window_id"]
    if verify in ("ax-textarea-contains", "ax-statictext-contains"):
        role = "AXTextArea" if verify == "ax-textarea-contains" else "AXStaticText"
        response = _run_rdog_frame(
            bin_path,
            f'@ax-find#2102:{{window:{{window_id:"{window_id}"}},role:"{role}",depth:12,max_elements:5000,include_values:true,limit:80}}',
            cwd,
        )
        value = _response_value(response)
        state["values"] = [
            normalized
            for match in (value.get("matches") or [])
            if (normalized := _normalize_ax_text(match.get("value")))
        ]
    return state


def _quit_app(app_name: str, cwd: Path) -> None:
    """退出指定 app: 先优雅 quit, 失败 (如未保存弹窗) 则 killall 兜底。"""
    cwd.mkdir(parents=True, exist_ok=True)
    result = _run_process(
        ["osascript", "-e", f'tell application "{app_name}" to quit'],
        cwd=cwd, timeout_s=10,
    )
    if result.timed_out or result.returncode != 0:
        # 未保存文档会阻止优雅退出: 评测场景全部是我们自己创建的内容,
        # killall 兜底不会丢失用户数据。
        force = _run_process(["killall", app_name], cwd=cwd, timeout_s=10)
        if force.returncode != 0:
            raise RuntimeError(f"{app_name} 无法退出")
    time.sleep(0.5)


def _open_app(bin_path: Path, app_name: str, cwd: Path) -> None:
    """通过 rdog @open-app 真实打开指定 app。"""
    _run_rdog_frame(
        bin_path,
        f'@open-app#2103:{{app_name:{json.dumps(app_name)},wait_ms:1500}}',
        cwd,
    )
    time.sleep(1.0)


def _ensure_probe_image() -> Path:
    """确保 /tmp 下存在评测用测试图片 (纯色 PNG, 无 PIL 依赖)。"""
    probe = Path("/tmp/rdog-ops-probe.png")
    if probe.exists() and probe.stat().st_size > 0:
        return probe

    def chunk(tag: bytes, data: bytes) -> bytes:
        block = struct.pack(">I", len(data)) + tag + data
        block += struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)
        return block

    width, height = 320, 200
    raw = b"".join(b"\x00" + bytes((66, 135, 245)) * width for _ in range(height))
    png = (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(raw))
        + chunk(b"IEND", b"")
    )
    probe.write_bytes(png)
    return probe


def _prepare_case(bin_path: Path, case: dict, cwd: Path) -> dict:
    """建立每个 case 约定的初始 app 状态,返回 before 状态证据。"""
    setup = case["setup"]
    if setup == "textedit-empty-doc":
        # TextEdit 首启可能弹"打开"对话框: 先退出, 再打开并新建空文稿,
        # 保证模型面对的是干净的空白文本区。
        _quit_app("TextEdit", cwd)
        _open_app(bin_path, "TextEdit", cwd)
        _run_rdog_frame(bin_path, "@key:Esc", cwd)
        _run_rdog_frame(bin_path, "@key:Cmd+N", cwd)
        time.sleep(0.8)
        before = _capture_app_state(bin_path, case, cwd, "before", allow_missing=False)
        if not before["windowTitle"] and not before["values"]:
            raise RuntimeError("TextEdit 空文稿未就绪")
        return before
    if setup == "preview-file-ready":
        # 只预置测试图片与 "Preview 未运行" 状态, 打开动作留给模型完成。
        probe = _ensure_probe_image()
        _quit_app("Preview", cwd)
        return {"exists": False, "windowTitle": "", "values": [], "probeFile": str(probe)}
    # 其余场景: 确保 app 处于未运行状态, 由模型自己打开。
    app_by_setup = {
        "calendar-open": "Calendar",
        "safari-fresh-window": "Safari",
        "terminal-open": "Terminal",
    }
    _quit_app(app_by_setup[setup], cwd)
    before = _capture_app_state(bin_path, case, cwd, "before", allow_missing=True)
    if before["exists"]:
        raise RuntimeError("app 退出后仍存在窗口")
    return before


def _fresh_verification_observed(tool_results_raw: list[str]) -> bool:
    """只接受 rdog 实际返回的成功结构化读取结果。"""
    for text in tool_results_raw:
        for response in _extract_rdog_responses(text):
            if not isinstance(response, dict):
                continue
            value = response.get("value")
            if not isinstance(value, dict):
                value = response
            if value.get("error") or value.get("code") not in (None, 0):
                continue
            if value.get("kind") in _READ_KINDS:
                return True
    return False


def _classify_run(case: dict, rc: int, events: list, tool_results_raw: list[str], before: dict, after: dict) -> dict:
    """按真实 tool call、Pi 多轮和 fresh AX/window 结果联合判定 (对齐 archive)."""
    verify = case["verify"]
    # macOS 文本输入存在系统级"自动大写句首", 结果断言采用大小写不敏感比较。
    expected = case.get("expectedResult", "").casefold()
    if verify == "window-count-increase":
        result_observed = after.get("windowCount", 0) > before.get("windowCount", 0)
        window_observed = after.get("exists") is True
    elif verify == "window-title-contains":
        result_observed = expected in after.get("windowTitle", "").casefold()
        window_observed = after.get("exists") is True
    else:  # ax-textarea-contains / ax-statictext-contains
        result_observed = any(expected in v.casefold() for v in after.get("values", []))
        window_observed = after.get("exists") is True

    # TextEdit 空文稿就绪场景初始状态是 "窗口已存在", 其余是 "app 未运行"。
    if case.get("setup") in ("textedit-empty-doc", "textedit-save-dir"):
        initial_matched = bool(before.get("exists")) or bool(before.get("values"))
    else:
        initial_matched = before.get("exists") is False

    rdog_commands = [
        evt.get("args", {}).get("command", "")
        for evt in events
        if evt.get("type") == "tool_execution_start" and evt.get("toolName") == "bash"
    ]
    checks = {
        "processCompleted": rc == 0,
        "multiTurnVerified": len(rdog_commands) >= 1,
        "realRdogCallObserved": any("rdog control" in c for c in rdog_commands),
        "freshVerificationObserved": _fresh_verification_observed(tool_results_raw),
        "appWindowObserved": window_observed,
        "expectedResultObserved": result_observed,
        "initialStateMatched": initial_matched,
    }
    return checks


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
    # max-tool-iterations 按模型配置: Group A (deepseek/minimax-cn) 高 churn
    # 需要 30 (archive 外部 runner 的 maxToolIterations=30), Group B 16 够用。
    # 5x8 baseline (commit 5c7b9a6) 实证: 8 次迭代会截断 Group A 导致 0/8。
    max_iter = int(model_cfg.get("maxToolIterations", 30))
    # Pi 调用超时随迭代上限线性放大 (外部 runner processTimeoutSeconds=900)。
    pi_timeout_s = min(900, max(120, max_iter * 30))
    final_success = False
    final_state = "not_attempted"
    error = ""
    last_checks: dict = {}

    for attempt in range(1, max_attempts + 1):
        ledger._attempt = attempt
        session_dir = Path(tempfile.mkdtemp(prefix=f"pi-{case_id}-", dir=tempfile.gettempdir()))
        # prepare: 建立 case 约定的初始 app 状态 + before 证据。
        # 环境阻塞 (app 退不掉 / 窗口起不来) 时直接判 runner_error, 不重试。
        try:
            before = _prepare_case(bin_path, case, session_dir / "prepare")
        except Exception as e:
            final_state = "environment_blocked"
            error = str(e)
            ledger.record("", rdog_error=f"prepare_failed: {e}")
            break
        rc, events = _invoke_pi_rpc(
            provider_key=provider_key, model=model_id, api_key=api_key,
            extension_path=extension_path, system_prompt=system_prompt,
            prompt=_build_case_prompt(case),
            session_dir=session_dir,
            max_tool_iterations=max_iter,
            timeout_s=pi_timeout_s,
        )

        # Walk events to extract tool calls + tool results, feed into ledger.
        # Pi RPC emits tool_execution_start {toolName, args} then tool_execution_end
        # {toolName, result, isError}. toolName is "bash" (after extension translation),
        # args.command is the actual `rdog control mac.lab ...` invocation.
        last_tool_start: dict | None = None
        tool_results_raw: list[str] = []
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
                    text = _extract_tool_result_text(evt.get("result", ""))
                    tool_results_raw.append(text)
                    is_err = evt.get("isError", False)
                    ledger.record(
                        "", rdog_response=text[:500],
                        rdog_error=("tool_error" if is_err else ""),
                    )

        # after 状态捕获 + 严格判定 (对齐 archive: fresh window/AX + expected)。
        after = _capture_app_state(
            bin_path, case, session_dir / "verify", "after", allow_missing=True
        )
        # reset: 恢复现场, 保证下一样本互不污染。
        try:
            _quit_app(case.get("app", ""), session_dir / "reset")
        except Exception:
            pass

        checks = _classify_run(case, rc, events, tool_results_raw, before, after)
        last_checks = checks
        if all(checks.values()):
            final_success = True
            final_state = "success"
            break
        elif attempt == max_attempts:
            final_state = "max_attempts_exceeded"
            error = f"rc={rc}, checks={checks}"
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
        checks=last_checks,
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
                        ledger_summary={"error": str(e)}, checks={}, error=str(e),
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
