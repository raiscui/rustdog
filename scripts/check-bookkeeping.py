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
BASE_LEDGER_FILES = {
    "WORKLOG.md",
    "notes.md",
    "LATER_PLANS.md",
    "ERRORFIX.md",
    "EPIPHANY_LOG.md",
    "task_plan.md",
    "EXPERIENCE.md",
}

# 2026-11-26 (RAI-433): 账本按票分文件 —— `ledger/<票号>.md`。原因很实在: 单文件账本让任意
# 两张 PR 都在同一个文件尾部追加, 每次 rebase 都在同一处冲突, 而 rebase 又会重写所有 sha,
# 于是「记账」和「冲突」互相放大。按票分文件后, 同一票的提交只碰自己那份。
LEDGER_DIR = "ledger"


def ledger_files(root: Path) -> set[str]:
    names = set(BASE_LEDGER_FILES)
    ledger_dir = root / LEDGER_DIR
    if ledger_dir.is_dir():
        names.update(f"{LEDGER_DIR}/{path.name}" for path in ledger_dir.glob("*.md"))
    return names

SHA_RE = re.compile(r"\b[0-9a-f]{7,40}\b")
# 「待合入后补」的占位: PR 阶段拿不到稳定的 sha, 用提交标题占位, 合入后由纯账本提交换成 sha。
PENDING_RE = re.compile(r"^- 提交: 待合入后补 \(标题: `?(?P<subject>.+?)`?\)$", re.MULTILINE)


def git(root: Path, *args: str) -> str:
    result = subprocess.run(
        ["git", *args], cwd=root, capture_output=True, text=True, check=False
    )
    if result.returncode != 0:
        raise SystemExit(f"git {' '.join(args)} 失败: {result.stderr.strip()}")
    return result.stdout


def ledger_text(root: Path) -> str:
    chunks: list[str] = []
    for name in sorted(ledger_files(root)):
        path = root / name
        if path.is_file():
            chunks.append(path.read_text(encoding="utf-8"))
    if not chunks:
        raise SystemExit("找不到任何账本文件")
    return "\n".join(chunks)


def commit_shas(
    root: Path,
    since: str | None,
    limit: int,
    first_parent: bool = True,
    exclude_merges: bool = False,
) -> list[str]:
    """范围内的 commit。

    `first_parent` 默认开着 (分支上只看本分支), 但 **main 上必须关掉**: main 落的是
    `Merge pull request #N`, PR 的工作 commit 在 merge commit 的第二父线上, `--first-parent`
    永远走不到它们 —— 于是「main 上漏账即失败」这句承诺会变成「一个都看不见」(2026-11-26
    评审实测: 范围恒为 1 个 merge commit, 而它恰好落进「只改账本」豁免)。
    main 模式同时 `exclude_merges`: merge commit 本身不承载工作, 不该要求它进账本。
    """
    args = ["log", "--format=%H"]
    if first_parent:
        args.append("--first-parent")
    if exclude_merges:
        args.append("--no-merges")
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
    allowed = ledger_files(root)
    for name in files:
        path = Path(name)
        if str(path) in allowed or path.name in BASE_LEDGER_FILES:
            continue
        return False
    return True


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
        "--main",
        action="store_true",
        help=(
            "main 分支模式: 关掉 --first-parent (否则只看得到 merge commit)、排除 merge 本身, "
            "范围内最新 --main-grace 个工作 commit 记宽限, 其余缺 sha 即失败 (RAI-433)"
        ),
    )
    parser.add_argument(
        "--main-grace",
        type=int,
        default=10,
        help="main 上允许的「刚合入、还没补 sha / 还留着占位」的工作 commit 数, 默认 10 (一次合并的提交量)",
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
    # 占位条目按**提交标题**认领: PR 阶段的 sha 会随 rebase 变, 但标题不变。
    pending_subjects = {
        match.group("subject").strip() for match in PENDING_RE.finditer(ledger)
    }

    shas = commit_shas(
        root,
        args.since,
        args.limit,
        first_parent=not args.main,
        exclude_merges=args.main,
    )
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

    if args.main:
        # main 上**不用基线**裁剪: 基线的定义是「最新的已被引用 commit」, 于是一个漏账 commit
        # 只要后面有一个 commit 被记账, 它就会永远落在基线之外 —— 再也不会被判红。窗口改成
        # 固定看最近 --limit 个, 漏账只能靠被推出窗口逃掉, 那是个有界且写明了的代价。
        scope = shas
        scope_note = f"最近 {args.limit} 个 commit (main 模式: 按窗口而不是按基线)"
    elif baseline_index is None:
        scope = shas
        scope_note = f"最近 {args.limit} 个 commit (无已记账基线)"
    else:
        scope = shas[:baseline_index]
        scope_note = "最近一个已被账本引用的 commit 之后"

    if args.since:
        scope_note = f"{args.since}..HEAD"

    subject_counts: dict[str, int] = {}
    for sha in scope:
        subject_counts[git(root, "log", "-1", "--format=%s", sha).strip()] = (
            subject_counts.get(git(root, "log", "-1", "--format=%s", sha).strip(), 0) + 1
        )

    unaccounted: list[tuple[str, str, list[str]]] = []
    graced: list[tuple[str, str]] = []
    accounted = 0
    exempt = 0
    pending: list[tuple[str, str]] = []
    for index, sha in enumerate(scope):
        short = sha[:7]
        subject = git(root, "log", "-1", "--format=%s", sha).strip()
        if short in referenced or sha in referenced:
            accounted += 1
            continue
        if is_ledger_only(root, sha):
            exempt += 1
            continue
        # 占位在**宽限窗口内**算「待补」而不是漏账 —— 否则一次合并的提交数超过宽限时, main 会在
        # sha 补齐提交合入之前就变红 (2026-11-26 实测: 5 个提交的 PR 合入后 main 立刻红)。
        # 超出宽限还留着占位才是漏账: 那时 sha 早已稳定。
        if subject in pending_subjects and (not args.main or index < args.main_grace):
            # 占位按标题认领, 所以标题必须唯一: 否则一条 `fix: typo` 这类通用占位能吞掉
            # 同标题的任意多个 commit, 一个占位放行一片。
            if subject_counts.get(subject, 0) > 1:
                unaccounted.append(
                    (
                        short,
                        subject,
                        [
                            f"占位标题不唯一: 范围内有 {subject_counts[subject]} 个 commit 用同一标题, "
                            "占位只能认领一个"
                        ],
                    )
                )
                continue
            pending.append((short, subject))
            continue
        # main 上「刚合入还没补 sha」是**正常时序**: sha 只有在合入后才稳定。所以最新
        # --main-grace 个 commit 记宽限, 更早的还缺 sha 就是漏账 (RAI-433)。
        if args.main and index < args.main_grace:
            graced.append((short, subject))
            continue
        if subject in pending_subjects:
            unaccounted.append(
                (
                    short,
                    subject,
                    [
                        f"占位超期: 账本里还写着「待合入后补」, 但该 commit 已在宽限 ({args.main_grace}) "
                        "之外 —— sha 早已稳定, 该补了"
                    ],
                )
            )
            continue
        unaccounted.append((short, subject, changed_files(root, sha)))

    print(f"检查范围: {scope_note}  共 {len(scope)} 个 commit")
    print(
        f"  已记账: {accounted}  占位待补 sha: {len(pending)}  记账提交(豁免): {exempt}"
        f"  漏记账: {len(unaccounted)}"
        + (f"  宽限内待补 sha: {len(graced)}" if graced else "")
    )
    if graced and args.main:
        for short, subject in graced:
            print(f"  (宽限) {short} {subject} —— 合入后由纯账本提交补 `- 提交: {short}`")

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
            "\n修法: 在该票的账本条目末尾写一行 (RAI-433 起账本按票分文件)\n"
            "  - 提交: <sha>\n"
            "账本位置: ledger/<票号>.md (新票), WORKLOG.md 是索引 + 2026-11-26 前的归档。\n"
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
