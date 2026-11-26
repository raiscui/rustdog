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
import signal
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
    "zenoh_unixpipe_fast_path": "compile-only:macos",
    "zenoh_router_client_windows": "windows",
    "control_ax_e2e": "compile-only:macos",
    "control_display_aware_e2e": "compile-only:macos",
    "control_mouse_e2e": "compile-only:macos",
    "control_ocr_e2e": "compile-only:macos",
    "control_window_e2e": "compile-only:macos",
    "recording_e2e": "exempt:由 ci.yml 的独立步骤执行 (unix), 不在本脚本清单内",
}

# target -> {os: 理由}: 该 target 在这个平台上**不执行**, 其余平台照常。
# 这是「平台不兼容 / 稳定红」的显式出口 —— 理由必须写明并挂 follow-up 票, 且会在 job summary
# 里单列一行 "exempt on this platform", 所以它既不是静默 skip, 也不会被当成"跑过了"。
PLATFORM_EXEMPTIONS: dict[str, dict[str, str]] = {
    "zenoh_router_client": {
        "ubuntu-latest": (
            "跨进程连接 e2e 在 ubuntu runner 上不稳定 (连续三轮各挂一个不同用例, 全是 client "
            "连不上刚启动的 daemon; macos 同用例全绿); 见 issue #163 / RAI-464"
        )
    },
    "control_websocket": {
        "windows-latest": (
            "windows runner 上整 target 挂死 (1500s 墙钟上限才终止), 用例未隔离 HOME; "
            "见 issue #162"
        )
    },
    "zenoh_router_client_windows": {
        "windows-latest": (
            "windows runner 上 daemon 因遗留 service_name 守卫拒启动 (用例未隔离 HOME); "
            "见 issue #162"
        )
    },
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
    return [
        target
        for target, kind in CLASSIFICATION.items()
        if kind in kinds and os_name not in PLATFORM_EXEMPTIONS.get(target, {})
    ]


def platform_exempt_targets(os_name: str) -> list[tuple[str, str]]:
    """本平台被显式豁免的 target 与理由 (只列本来会执行的那些)。"""
    kinds = OS_KINDS[os_name]
    return [
        (target, PLATFORM_EXEMPTIONS[target][os_name])
        for target, kind in CLASSIFICATION.items()
        if kind in kinds and os_name in PLATFORM_EXEMPTIONS.get(target, {})
    ]


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
    for target, os_reasons in PLATFORM_EXEMPTIONS.items():
        if target not in CLASSIFICATION:
            problems.append(f"PLATFORM_EXEMPTIONS 里的 `{target}` 没有在 CLASSIFICATION 登记")
        for os_name in os_reasons:
            if os_name not in ALL_OS:
                problems.append(f"PLATFORM_EXEMPTIONS 里的 `{target}` 写了未知平台 `{os_name}`")
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

# 一次运行 (整个 job 步骤) 的总预算。**上限必须是 job 级而不是每 target 常数**: 两次各 1500s 的
# 挂起就把 60 分钟的 job 撑爆, GitHub 一 cancel, 日志只剩 "The job has been cancelled" ——
# 「点不出是哪个 target」原样复现。留出余量给收尾与 summary。
JOB_BUDGET_SECONDS = int(os.environ.get("RDOG_JOB_BUDGET_SECONDS", "3300"))

# 剩余预算低于这个数就不再起新 target: 起了也只会被截断, 不如直接报「预算不足」并点名。
BUDGET_FLOOR_SECONDS = 120


def _spawn(argv: list[str]) -> subprocess.Popen:
    """起一个**可整组回收**的子进程。

    `cargo test` 自己会把测试二进制、daemon 当子进程派生出去。只 kill 直接子进程时它们会变成
    孤儿留在 runner 上, 污染同一个 job 里后续的 target (2026-11-26 实测: 一个挂死的 target 让
    后面 target 的全局状态断言连带变红, 看起来像另一个用例的问题)。所以子进程进独立进程组。
    """
    kwargs: dict = {
        "stdout": subprocess.PIPE,
        "stderr": subprocess.PIPE,
        "text": True,
        "encoding": "utf-8",
        "errors": "replace",
    }
    if os.name == "nt":
        kwargs["creationflags"] = subprocess.CREATE_NEW_PROCESS_GROUP
    else:
        kwargs["start_new_session"] = True
    return subprocess.Popen(argv, **kwargs)


def _kill_tree(proc: subprocess.Popen) -> str:
    """按 PID 整组终止 (windows 用 taskkill /T, 绝不按映像名)。返回做了什么, 供日志留痕。"""
    if proc.poll() is not None:
        return "already exited"
    if os.name == "nt":
        subprocess.run(
            ["taskkill", "/F", "/T", "/PID", str(proc.pid)],
            capture_output=True,
            check=False,
        )
        return f"taskkill /T /PID {proc.pid}"
    try:
        os.killpg(os.getpgid(proc.pid), signal.SIGKILL)
        return f"killpg {os.getpgid(proc.pid)}"
    except ProcessLookupError:
        return "process group already gone"


def run(argv: list[str], timeout: int | None = None) -> tuple[int, str]:
    # encoding 固定 utf-8: 默认按平台 locale 解码 (windows runner 是 cp1252), 而 cargo test
    # 的输出里有 UTF-8 中文断言文案 —— 会在这里抛 UnicodeDecodeError 让整步崩掉, 归因看起来
    # 像"target 红"但其实是解码失败。errors=replace 保证任何残余字节都不会中断。
    proc = _spawn(argv)
    try:
        stdout, stderr = proc.communicate(timeout=timeout)
        return proc.returncode, ANSI.sub("", (stdout or "") + (stderr or ""))
    except subprocess.TimeoutExpired:
        # 先整组杀, 再把管道里已有的输出读干净 (杀完管道会 EOF, communicate 立刻返回)。
        killed = _kill_tree(proc)
        try:
            stdout, stderr = proc.communicate(timeout=30)
        except subprocess.TimeoutExpired:
            stdout, stderr = "", ""
        partial = ANSI.sub("", (stdout or "") + (stderr or ""))
        return (
            124,
            partial + f"\n[timeout] 超过 {timeout}s 未结束, 已整组终止 ({killed})\n",
        )


def main() -> int:
    # windows runner 的控制台是 cp1252: 表格与失败详情里有中文, 直接 print 会抛
    # UnicodeEncodeError 让整步崩掉 (归因看起来像 target 红)。解码与编码两端都固定 utf-8。
    for stream in (sys.stdout, sys.stderr):
        if hasattr(stream, "reconfigure"):
            stream.reconfigure(encoding="utf-8", errors="replace")

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
    budget_skipped: list[str] = []

    # 预算从这一步开始时算起, 不是从 job 开始 (前面的 build 步骤耗时与本步无关)。
    deadline = time.monotonic() + JOB_BUDGET_SECONDS

    def budget_timeout() -> int:
        """本次可用的墙钟: 取「单 target 上限」与「剩余预算」的小者。"""
        remaining = deadline - time.monotonic()
        return max(1, int(min(TARGET_TIMEOUT_SECONDS, remaining)))

    def out_of_budget() -> bool:
        return deadline - time.monotonic() <= BUDGET_FLOOR_SECONDS

    for target in executed_targets(args.os):
        if out_of_budget():
            budget_skipped.append(target)
            continue
        started = time.monotonic()
        code, log = run(["cargo", "test", "--test", target], timeout=budget_timeout())
        passed, failed, ignored = summarize(log)
        elapsed = round(time.monotonic() - started)
        executed_rows.append(
            f"| `{target}` | {passed} | {failed} | {ignored} | {code} | {elapsed}s |"
        )
        if code != 0 or failed or passed == 0:
            failures.append(f"{target}: exit={code} passed={passed} failed={failed}")
            print(f"--- {target} failure detail ---\n{failure_detail(target, log)}\n", file=sys.stderr)

    for target in compile_only_targets(args.os):
        if out_of_budget():
            budget_skipped.append(target)
            continue
        # compile-only 也要上限: `--no-run` 一样会挂 (链接/依赖解析), 没有上限时它撑到 job
        # timeout, 同样点不出名字。
        code, log = run(
            ["cargo", "test", "--no-run", "--test", target], timeout=budget_timeout()
        )
        compile_rows.append(f"| `{target}` | {code} |")
        if code != 0:
            failures.append(f"{target}: compile failed (exit={code})")
            tail = "\n".join(log.strip().splitlines()[-25:])
            print(f"--- {target} compile log tail ---\n{tail}\n", file=sys.stderr)

    # 一个 target 都没执行 = 绿, 和「一条测试都没跑」长得一模一样 (RAI-397 要消灭的形态)。
    # 今天不可达 (每个平台至少 1 个), 但豁免可以让 executed 集合变空, 所以在这里判红。
    if budget_skipped:
        # 预算耗尽不是「跳过」而是失败: 这些 target 没有被验过, 报绿等于把「没跑」当成「通过」。
        failures.append(
            "job 预算耗尽: "
            + ", ".join(budget_skipped)
            + f" 未执行 (JOB_BUDGET_SECONDS={JOB_BUDGET_SECONDS})"
        )
        annotate(
            "error",
            "integration target budget",
            f"job 预算 {JOB_BUDGET_SECONDS}s 不足, 未执行: {', '.join(budget_skipped)}",
        )

    if not executed_rows:
        annotate("error", "integration target inventory", f"{args.os} 没有任何 target 被执行")
        return 1

    summary = [
        f"### integration targets on `{args.os}`",
        "",
        "**executed** — 下列 target 的 `passed` 计数必须 > 0:",
        "",
        "| target | passed | failed | ignored | exit | elapsed |",
        "| --- | --- | --- | --- | --- | --- |",
        *executed_rows,
    ]
    exempt_rows = [
        f"| `{target}` | {reason} |" for target, reason in platform_exempt_targets(args.os)
    ]
    if exempt_rows:
        summary += [
            "",
            "**exempt on this platform** — 本来会在本平台执行, 现在是显式豁免 (带理由与票号), "
            "**不计入 ran 集合**, 也不等于跑过了:",
            "",
            "| target | 豁免理由 |",
            "| --- | --- |",
            *exempt_rows,
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
