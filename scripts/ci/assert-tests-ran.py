#!/usr/bin/env python3
"""断言 `cargo test` 的日志里真的跑了测试 — 让"零测试也绿"变成红。

背景 (2026-11-26, RAI-397):
    `test (windows-latest)` 在 Build 阶段就红, 一次测试都没跑到, 但门上的红点与
    "测试真的失败"长得一样, 只能人肉翻日志。拆成 build/test 两个 job 解决大部分
    问题 (见 docs/agents/ci-merge-gate.md), 但还有一类更隐蔽的情况: test 步骤
    退出码 0 而实际一环测试都没执行 (filter 打错、`#[ignore]` 全忽略、平台门把
    整组测试编译掉了 —— 本仓库有大量 `#[cfg(windows)]` 测试, 这正是它们会消失的
    方式)。

用法:
    python3 scripts/ci/assert-tests-ran.py <cargo-test-log> [--min 1] [--label unit]
退出码: 0 = 跑了且没失败; 1 = 没跑到 / 有失败; 2 = 用法错误
"""

from __future__ import annotations

import argparse
import os
import re
import sys

ANSI = re.compile(r"\x1b\[[0-9;]*[A-Za-z]")
RESULT = re.compile(
    r"test result: (?P<status>\w+)\. (?P<passed>\d+) passed; (?P<failed>\d+) failed"
)


def _utf8_stdio() -> None:
    for stream in (sys.stdout, sys.stderr):
        if hasattr(stream, "reconfigure"):
            stream.reconfigure(encoding="utf-8", errors="replace")


def annotate(level: str, title: str, message: str) -> None:
    if os.environ.get("GITHUB_ACTIONS") == "true":
        print(f"::{level} title={title}::{message}")
    print(f"[{level}] {title}: {message}", file=sys.stderr)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("log", help="cargo test 的输出日志")
    parser.add_argument("--min", type=int, default=1, help="至少要通过多少个测试")
    parser.add_argument("--label", default="unit", help="报告里用的套件名")
    args = parser.parse_args()

    text = ANSI.sub("", open(args.log, encoding="utf-8", errors="replace").read())
    passed = failed = 0
    for match in RESULT.finditer(text):
        passed += int(match.group("passed"))
        failed += int(match.group("failed"))

    print(f"{args.label}: passed={passed} failed={failed} (log {args.log})")
    if failed:
        annotate("error", "tests failed", f"{args.label} suite: {failed} failed / {passed} passed")
        return 1
    if passed < args.min:
        annotate(
            "error",
            "tests did not run",
            f"{args.label} suite produced {passed} passing test(s), need >= {args.min}; "
            "the harness exited 0 without exercising anything",
        )
        return 1
    print(f"{args.label}: OK - {passed} test(s) really ran")
    return 0


if __name__ == "__main__":
    _utf8_stdio()
    raise SystemExit(main())
