#!/usr/bin/env python3
"""提交即记账 — 检查 git 提交是否都进了六文件账本。

背景 (2026-10-22 实锤):
    2026-10-07 ~ 10-20 期间的产出 (五个 commit + 一次跨机实战 + 一次
    wayfinder 建图) **一条六文件记录都没有**。知识还在 git 里, 不算丢失 ——
    但**索引缺失**: 下个会话读 task_plan.md 会以为 10-06 之后无事发生。
    这类账本滞后比知识缺失更隐蔽。

判定规则:
    - 账本文件集合 = 六文件 + EXPERIENCE.md (收件箱)
    - 范围 = 当前分支上, 距"最近一个已被账本引用的 commit"之后的全部 commit
      (没有则取最近 --limit 个)
    - 每个 commit 要么:
        a) 它的 sha 出现在某个账本文件里 (被记账), 要么
        b) 它只改了账本文件 (记账提交本身, 豁免)
      否则判为"漏记账"

两种模式:
    默认 (strict)  = 漏记账 -> 退出码 1, 适合本地 pre-flight
    --report-only   = 只报告, 永远退出码 0, 适合 CI (不阻断推送)
      在 GitHub Actions 里会额外发 `::warning::` 注解, 所以漏账条目会显示在
      提交/PR 视图里, 而不是淹没在日志中。

用法:
    python3 scripts/check-bookkeeping.py [--repo-root .] [--since <ref>] [--limit 30]
    python3 scripts/check-bookkeeping.py --report-only
    python3 scripts/check-bookkeeping.py --ref-only [HEAD]   # 只打印可粘贴的 sha

退出码: 0 = 全部已记账 (或 report-only); 1 = 有漏记账 (仅 strict); 2 = 用法/环境错误
"""

from __future__ import annotations

import argparse
import os
import re
import subprocess
import sys
from pathlib import Path

# 账本文件: 六文件 + 经验收件箱。只改这些文件的提交 = 记账提交, 豁免。
LEDGER_FILES = {
    "WORKLOG.md",
    "notes.md",
    "LATER_PLANS.md",
    "ERRORFIX.md",
    "EPIPHANY_LOG.md",
    "task_plan.md",
    "EXPERIENCE.md",
}

SHA_RE = re.compile(r"\b[0-9a-f]{7,40}\b")


def git(root: Path, *args: str) -> str:
    result = subprocess.run(
        ["git", *args], cwd=root, capture_output=True, text=True, check=False
    )
    if result.returncode != 0:
        raise SystemExit(f"git {' '.join(args)} 失败: {result.stderr.strip()}")
    return result.stdout


def ledger_text(root: Path) -> str:
    chunks: list[str] = []
    for name in sorted(LEDGER_FILES):
        path = root / name
        if path.is_file():
            chunks.append(path.read_text(encoding="utf-8"))
    if not chunks:
        raise SystemExit("找不到任何账本文件")
    return "\n".join(chunks)


def commit_shas(root: Path, since: str | None, limit: int) -> list[str]:
    args = ["log", "--format=%H", "--first-parent"]
    if since:
        args.append(f"{since}..HEAD")
    else:
        args.append(f"-{limit}")
    return [line.strip() for line in git(root, *args).splitlines() if line.strip()]


def changed_files(root: Path, sha: str) -> list[str]:
    out = git(root, "show", "--name-only", "--format=", sha)
    return [line.strip() for line in out.splitlines() if line.strip()]


def is_ledger_only(root: Path, sha: str) -> bool:
    files = changed_files(root, sha)
    if not files:
        return False
    return all(Path(name).name in LEDGER_FILES for name in files)


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="提交即记账检查")
    parser.add_argument("--repo-root", default=".")
    parser.add_argument("--since", default=None, help="起点 ref (如 origin/main)")
    parser.add_argument("--limit", type=int, default=30, help="无 --since 时回看多少个 commit")
    parser.add_argument(
        "--report-only",
        action="store_true",
        help="只报告不失败 (CI 用): 永远退出 0, 但漏账条目会发 ::warning:: 注解",
    )
    parser.add_argument(
        "--ref-only",
        nargs="?",
        const="HEAD",
        default=None,
        help="只打印该 ref 的短 sha (用来填账本条目)",
    )
    args = parser.parse_args(argv)

    root = Path(args.repo_root).resolve()

    if args.ref_only:
        print(git(root, "rev-parse", "--short", args.ref_only).strip())
        return 0

    ledger = ledger_text(root)
    referenced = set(SHA_RE.findall(ledger))

    shas = commit_shas(root, args.since, args.limit)
    if not shas:
        print("范围内没有 commit")
        return 0

    # 范围起点 = 最近一个已被账本引用的 commit。git log 是新 -> 旧, 所以
    # 第一个命中的就是最新的已记账 commit; 它**之后**的 commit 才需要检查
    # (更早的历史在账本追平的那一刻就已经有归属了)。
    baseline_index: int | None = None
    for index, sha in enumerate(shas):
        if sha[:7] in referenced or sha in referenced:
            baseline_index = index
            break

    if baseline_index is None:
        scope = shas
        scope_note = f"最近 {args.limit} 个 commit (无已记账基线)"
    else:
        scope = shas[:baseline_index]
        scope_note = "最近一个已被账本引用的 commit 之后"

    if args.since:
        scope_note = f"{args.since}..HEAD"

    unaccounted: list[tuple[str, str, list[str]]] = []
    accounted = 0
    exempt = 0
    for sha in scope:
        short = sha[:7]
        subject = git(root, "log", "-1", "--format=%s", sha).strip()
        if short in referenced or sha in referenced:
            accounted += 1
            continue
        if is_ledger_only(root, sha):
            exempt += 1
            continue
        unaccounted.append((short, subject, changed_files(root, sha)))

    print(f"检查范围: {scope_note}  共 {len(scope)} 个 commit")
    print(f"  已记账: {accounted}  记账提交(豁免): {exempt}  漏记账: {len(unaccounted)}")

    if unaccounted:
        mode = "只报告 (report-only)" if args.report_only else "阻断 (strict)"
        print(f"\n{'⚠️' if args.report_only else '❌'} {len(unaccounted)} 个 commit 没有进账本 [{mode}]")
        for short, subject, files in unaccounted:
            print(f"  {short} {subject}")
            print(f"      改了: {', '.join(files[:6])}")
            # GitHub Actions 注解: 漏账条目会显示在提交/PR 视图, 而不是只埋进日志
            if os.environ.get("GITHUB_ACTIONS"):
                print(
                    f"::warning title=提交未进账本::"
                    f"{short} {subject} —— 六文件里找不到这个 commit 的条目"
                )
        print(
            "\n修法: 在 WORKLOG.md (或 notes/LATER_PLANS/…) 的对应条目末尾写一行\n"
            "  - 提交: <sha>\n"
            "再提交该账本改动 (纯账本提交本身豁免)。\n"
            "取 sha: python3 scripts/check-bookkeeping.py --ref-only HEAD"
        )
        if args.report_only:
            print("\n(report-only 模式: 不阻断, 但漏账条目已按 ::warning:: 注解上报)")
            return 0
        print(
            "\n(strict 模式: 退出码 1。CI 用 --report-only, 本地 pre-flight 用默认 strict。)",
            file=sys.stderr,
        )
        return 1

    print("OK: 范围内的 commit 都已进账本")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
