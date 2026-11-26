#!/usr/bin/env python3
"""跑一批 integration test target, 并把"每个 target 的实际执行计数"变成硬判据。

背景 (2026-11-26, RAI-411):
    在 RAI-397 之前, CI 只跑 `tests/` 里 19 个 target 中的 1 个 (`recording_e2e`);
    其余 18 个**从未在任何平台执行过** —— 包括 AGENTS.md 点名的协议回归面
    (`control_lanes` / `control_mode`) 和 zenoh fast path。本脚本是放量的执行器:
    target 清单按平台写在 `TARGETS` 里, 每个 target 的 `passed` 数必须 > 0,
    否则判红 —— 「退出码 0 但一个测试都没跑」在 CI 上同样是失败。

    编译期覆盖由 build 阶段的 `cargo test --no-run` 负责 (全 target), 所以这里
    只负责执行。`COMPILE_ONLY` 里的 target 需要真实桌面会话 (AX / 屏幕捕获 / TCC),
    GitHub runner 没有 —— 它们**编译但不执行**, 并且必须在 job summary 里显式列出,
    不允许静默消失。

用法:
    python3 scripts/ci/run-integration-targets.py --os ubuntu-latest
退出码: 0 = 全部执行且计数达标; 1 = 有失败 / 零执行; 2 = 用法错误
"""

from __future__ import annotations

import argparse
import os
import re
import subprocess
import sys

ANSI = re.compile(r"\x1b\[[0-9;]*[A-Za-z]")
RESULT = re.compile(
    r"test result: (?P<status>\w+)\. (?P<passed>\d+) passed; (?P<failed>\d+) failed; "
    r"(?P<ignored>\d+) ignored"
)

UNIX_TARGETS = [
    "connect_modes",
    "control_lanes",
    "control_mode",
    "control_pty",
    "control_tty",
    "shell_pty",
    "daemon_inbound",
    "listen_suspend",
    "zenoh_router_client",
    "computer_use_density",
    "control_websocket",
]

TARGETS = {
    "ubuntu-latest": UNIX_TARGETS,
    "macos-latest": UNIX_TARGETS + ["zenoh_unixpipe_fast_path"],
    "windows-latest": ["zenoh_router_client_windows"],
}

COMPILE_ONLY = {
    "macos-latest": [
        "control_ax_e2e",
        "control_display_aware_e2e",
        "control_mouse_e2e",
        "control_ocr_e2e",
        "control_window_e2e",
    ],
}


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


def run(argv: list[str]) -> tuple[int, str]:
    completed = subprocess.run(argv, capture_output=True, text=True)
    return completed.returncode, ANSI.sub("", completed.stdout + completed.stderr)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--os", required=True, choices=sorted(TARGETS))
    args = parser.parse_args()

    rows: list[str] = []
    failures: list[str] = []

    for target in TARGETS[args.os]:
        code, log = run(["cargo", "test", "--test", target])
        passed, failed, ignored = summarize(log)
        rows.append(f"| `{target}` | {passed} | {failed} | {ignored} | {code} |")
        if code != 0 or failed or passed == 0:
            failures.append(f"{target}: exit={code} passed={passed} failed={failed}")
            tail = "\n".join(log.strip().splitlines()[-25:])
            print(f"--- {target} log tail ---\n{tail}\n", file=sys.stderr)

    for target in COMPILE_ONLY.get(args.os, []):
        code, log = run(["cargo", "test", "--no-run", "--test", target])
        rows.append(f"| `{target}` | compiled, not executed | — | — | {code} |")
        if code != 0:
            failures.append(f"{target}: compile failed (exit={code})")

    summary = [
        f"### integration targets on `{args.os}`",
        "",
        "| target | passed | failed | ignored | exit |",
        "| --- | --- | --- | --- | --- |",
        *rows,
    ]
    if COMPILE_ONLY.get(args.os):
        summary += [
            "",
            "**compiled, not executed** (5 个 macOS GUI 面): 需要真实桌面会话 —— 已登录的 GUI "
            "用户、Accessibility 与 Screen Recording 授权。GitHub runner 没有这些条件, 所以这里只"
            "证明它们能编译, **不计入 ran 集合**。真机 lane 的跑法: 在真机仓库根执行 "
            "`cargo test --test control_ax_e2e`(或 control_display_aware_e2e / control_mouse_e2e / "
            "control_ocr_e2e / control_window_e2e), 前置是系统设置 → 隐私与安全性 里给终端 "
            "Accessibility + Screen Recording 权限。",
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
