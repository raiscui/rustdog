#!/usr/bin/env python3
"""跑一批 integration test target, 并把"每个 target 的实际执行计数"变成硬判据。

背景 (2026-11-26, RAI-411):
    在 RAI-397 之前, CI 只跑 `tests/` 里 19 个 target 中的 1 个 (`recording_e2e`);
    其余 18 个**从未在任何平台执行过**。本脚本是放量的执行器, 它守两件事:

    1. **执行**: 清单里每个 target 的 `passed` 数必须 > 0, 否则判红 —— 「退出码 0
       但一个测试都没跑」在 CI 上同样是失败。
    2. **归属**: `tests/` 下每个文件必须在本脚本的 `CLASSIFICATION` 里有明确归属
       (在哪些平台执行 / 只编译 / 为什么豁免)。**新增 `tests/foo.rs` 而没登记 = 红**,
       否则它既不执行也不进 summary, 脚本照旧全绿 —— 那正是本票要消灭的形态。

用法:
    python3 scripts/ci/run-integration-targets.py --os ubuntu-latest
退出码: 0 = 全部执行且计数达标; 1 = 有失败 / 零执行 / 有未登记的 target; 2 = 用法错误
"""

from __future__ import annotations

import argparse
import os
import re
import subprocess
import time
import sys
from pathlib import Path

ANSI = re.compile(r"\x1b\[[0-9;]*[A-Za-z]")
RESULT = re.compile(
    r"test result: (?P<status>\w+)\. (?P<passed>\d+) passed; (?P<failed>\d+) failed; "
    r"(?P<ignored>\d+) ignored"
)

ALL_OS = ("ubuntu-latest", "macos-latest", "windows-latest")

# target -> 归属。取值:
#   "all"                 三个平台都执行 (文件没有平台 cfg 门)
#   "unix"                只在 unix 平台执行 (ubuntu + macos, 文件是 `#![cfg(unix)]`)
#   "macos" / "windows"   只在那个平台执行 (文件是 `#![cfg(target_os = ...)]`)
#   "compile-only:macos"  只编译不执行, 且只在那个平台的 build/test 阶段有意义
#   "exempt:<理由>"       完全不在 CI 里跑, 理由必须写明
# 没登记 = 红 (见 verify_inventory)。
CLASSIFICATION: dict[str, str] = {
    "connect_modes": "unix",
    "control_lanes": "unix",
    "control_mode": "unix",
    "control_pty": "unix",
    "control_tty": "unix",
    "shell_pty": "unix",
    "daemon_inbound": "unix",
    "listen_suspend": "unix",
    "zenoh_router_client": "unix",
    "computer_use_density": "all",
    "control_websocket": "all",
    "zenoh_unixpipe_fast_path": "macos",
    "zenoh_router_client_windows": "windows",
    "control_ax_e2e": "compile-only:macos",
    "control_display_aware_e2e": "compile-only:macos",
    "control_mouse_e2e": "compile-only:macos",
    "control_ocr_e2e": "compile-only:macos",
    "control_window_e2e": "compile-only:macos",
    "recording_e2e": "exempt:由 ci.yml 的独立步骤执行 (unix), 不在本脚本清单内",
}


OS_KIND = {"ubuntu-latest": "unix", "macos-latest": "macos", "windows-latest": "windows"}
# macos 同时是 unix 平台, 所以它吃的 kind 集合是 {"all", "unix", "macos"}。
OS_KINDS: dict[str, set[str]] = {
    "ubuntu-latest": {"all", "unix"},
    "macos-latest": {"all", "unix", "macos"},
    "windows-latest": {"all", "windows"},
}


def executed_targets(os_name: str) -> list[str]:
    kinds = OS_KINDS[os_name]
    return [target for target, kind in CLASSIFICATION.items() if kind in kinds]


def compile_only_targets(os_name: str) -> list[str]:
    return [
        target
        for target, kind in CLASSIFICATION.items()
        if kind == f"compile-only:{OS_KIND[os_name]}"
    ]


def verify_inventory(repo_root: Path) -> list[str]:
    """`tests/*.rs` 与 CLASSIFICATION 双向对账。"""
    problems: list[str] = []
    for path in sorted((repo_root / "tests").glob("*.rs")):
        if path.stem not in CLASSIFICATION:
            problems.append(
                f"tests/{path.name} 没有归属: 在 CLASSIFICATION 里登记为 executed / compile-only / exempt"
            )
    for target in CLASSIFICATION:
        if not (repo_root / "tests" / f"{target}.rs").exists():
            problems.append(f"CLASSIFICATION 里的 `{target}` 在 tests/ 下没有对应文件")
    return problems


def annotate(level: str, title: str, message: str) -> None:
    if os.environ.get("GITHUB_ACTIONS") == "true":
        print(f"::{level} title={title}::{message}")
    print(f"[{level}] {title}: {message}", file=sys.stderr)


def summarize(text: str) -> tuple[int, int, int]:
    passed = failed = ignored = 0
    for match in RESULT.finditer(text):
        passed += int(match.group("passed"))
        failed += int(match.group("failed"))
        ignored += int(match.group("ignored"))
    return passed, failed, ignored



def failure_detail(target: str, log: str) -> str:
    """把测试失败的可读部分挑出来, 并把完整日志落到 target/ci-logs/。

    只打最后 25 行会把编译告警当成失败原因 (2026-11-26 实测), 真正的
    `---- <test> stdout ----` / `failures:` 块更早。
    """
    log_dir = Path("target") / "ci-logs"
    log_dir.mkdir(parents=True, exist_ok=True)
    (log_dir / f"{target}.log").write_text(log, encoding="utf-8")

    lines = log.splitlines()
    picked: list[str] = []
    for index, line in enumerate(lines):
        if line.startswith("---- ") or line.startswith("failures:") or "panicked at" in line:
            picked.extend(lines[index : index + 12])
    if not picked:
        picked = lines[-40:]
    return "\n".join(picked[:160]) + f"\n(full log: {log_dir / (target + '.log')})"


# 单 target 的墙钟上限。跑不完就判这个 target 红并留下它已有的输出, 而不是让整个 job 撞上
# GitHub 的 job timeout 被 cancel —— 后者在日志里只显示 "The job has been cancelled", 分不清
# 是哪个 target 挂住, 也无法区分"慢"与"死等"。
TARGET_TIMEOUT_SECONDS = 1500


def run(argv: list[str], timeout: int | None = None) -> tuple[int, str]:
    # encoding 固定 utf-8: 默认按平台 locale 解码 (windows runner 是 cp1252), 而 cargo test
    # 的输出里有 UTF-8 中文断言文案 —— 会在这里抛 UnicodeDecodeError 让整步崩掉, 归因看起来
    # 像"target 红"但其实是解码失败。errors=replace 保证任何残余字节都不会中断。
    try:
        completed = subprocess.run(
            argv,
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=timeout,
        )
    except subprocess.TimeoutExpired as expired:
        # text=True 下 expired.stdout/stderr 仍可能是 bytes, 逐段判定再拼。
        partial = "".join(
            chunk if isinstance(chunk, str) else (chunk or b"").decode("utf-8", "replace")
            for chunk in (expired.stdout, expired.stderr)
            if chunk
        )
        return 124, ANSI.sub("", partial) + f"\n[timeout] 超过 {timeout}s 未结束, 已终止\n"
    return completed.returncode, ANSI.sub("", completed.stdout + completed.stderr)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--os", required=True, choices=sorted(ALL_OS))
    parser.add_argument("--repo-root", default=".")
    args = parser.parse_args()

    inventory_problems = verify_inventory(Path(args.repo_root).resolve())
    if inventory_problems:
        for problem in inventory_problems:
            annotate("error", "integration target inventory", problem)
        return 1

    executed_rows: list[str] = []
    compile_rows: list[str] = []
    failures: list[str] = []

    for target in executed_targets(args.os):
        started = time.monotonic()
        code, log = run(["cargo", "test", "--test", target], timeout=TARGET_TIMEOUT_SECONDS)
        passed, failed, ignored = summarize(log)
        elapsed = round(time.monotonic() - started)
        executed_rows.append(
            f"| `{target}` | {passed} | {failed} | {ignored} | {code} | {elapsed}s |"
        )
        if code != 0 or failed or passed == 0:
            failures.append(f"{target}: exit={code} passed={passed} failed={failed}")
            print(f"--- {target} failure detail ---\n{failure_detail(target, log)}\n", file=sys.stderr)

    for target in compile_only_targets(args.os):
        code, log = run(["cargo", "test", "--no-run", "--test", target])
        compile_rows.append(f"| `{target}` | {code} |")
        if code != 0:
            failures.append(f"{target}: compile failed (exit={code})")
            tail = "\n".join(log.strip().splitlines()[-25:])
            print(f"--- {target} compile log tail ---\n{tail}\n", file=sys.stderr)

    summary = [
        f"### integration targets on `{args.os}`",
        "",
        "**executed** — 下列 target 的 `passed` 计数必须 > 0:",
        "",
        "| target | passed | failed | ignored | exit | elapsed |",
        "| --- | --- | --- | --- | --- | --- |",
        *executed_rows,
    ]
    if compile_rows:
        summary += [
            "",
            "**compiled, not executed** — 需要真实桌面会话 (已登录 GUI + Accessibility / "
            "Screen Recording 授权), GitHub runner 没有; 这里只证明能编译, **不计入 ran 集合**。",
            "真机 lane 跑法: 仓库根执行 `cargo test --test <target>`。",
            "",
            "| target | compile exit |",
            "| --- | --- |",
            *compile_rows,
        ]
    body = "\n".join(summary)
    print(body)
    if os.environ.get("GITHUB_STEP_SUMMARY"):
        with open(os.environ["GITHUB_STEP_SUMMARY"], "a", encoding="utf-8") as handle:
            handle.write(body + "\n")

    if failures:
        annotate("error", "integration targets failed", "; ".join(failures))
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
