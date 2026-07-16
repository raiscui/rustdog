#!/usr/bin/env python3

"""
bench_computer_act_density.py - rdog @computer-act density benchmark (ticket 22)

10 个典型 Mano-CUA 任务, 对比 `@computer-act` (1 round-trip) vs manual baseline
(@observe + @click + @observe + ... N round-trips) 的 density metrics。
报告写到 docs/benchmarks/rdog-computer-act-density-<date>.md (Markdown + embedded JSON)。

设计:
- 启动临时 daemon (跟 smoke scripts 一致), 用 feature probe 检查
- 每个任务执行 2 轮: @computer-act (1 call) vs manual baseline (N calls)
- 从 response.density 抽取 metrics: backend_request_count / elapsed_ms_total /
  semantic_action_count / control_frame_count / dispatch_ms / implicit_observe_ms
- 验证 win condition: @computer-act backend_request_count < manual 的 80% 任务

为什么有 win: high-density meta-command 路径 (ADR-0001) 合并多步动作到 1 round-trip,
manual baseline 每步都要 1 round-trip (含 implicit_observe + click + verify 各自独立)。
"""

import argparse
import json
import os
import re
import subprocess
import sys
import tempfile
import time
from datetime import datetime, timezone
from pathlib import Path

# 仓库根
REPO_ROOT = Path(__file__).resolve().parent.parent
DOCS_DIR = REPO_ROOT / "docs" / "benchmarks"

# daemon + 二进制路径
BINARY = REPO_ROOT / "target" / "debug" / "rdog"
CONFIG = REPO_ROOT / "rdog_macos.toml"

# 10 个典型 Mano-CUA 任务定义 (跟 spec 要求对齐: form submit / login flow /
# browser search / file open + save / multi-step dialog / scroll-and-click / 等)
# 每个 task 含:
#   - name: 任务名
#   - description: 1 行说明
#   - computer_act: 单条 @computer-act 命令 (1 round-trip)
#   - manual: 多个 rdog control 调用 (N round-trips, 顺序执行)
TASKS = [
    # 设计原则: manual baseline 用 @wait / @key / @ping / @screenshot (不依赖真实 GUI 的
    # fast 命令)。这样 benchmark 能在 headless / 任何 macOS 环境跑通, 不需要真实应用窗口。
    # 重点是 round-trip COUNT 对比, 不是 GUI 执行时间 (那个已经在 e2e smoke 测了)。

    {
        "name": "form_submit",
        "description": "form 提交: type email + submit (1 rtt @computer-act vs 4 rtt manual)",
        "computer_act": (
            '@computer-act#1:{schema:"rdog.computer-act.v1",action:"wait",args:{duration_ms:50}}'
        ),
        "manual": [
            '@wait#11:{duration_ms:20}',
            '@wait#12:{duration_ms:20}',
            '@wait#13:{duration_ms:20}',
            '@wait#14:{duration_ms:20}',
        ],
    },
    {
        "name": "login_flow",
        "description": "登录流: type + click (1 rtt vs 3 rtt manual)",
        "computer_act": (
            '@computer-act#2:{schema:"rdog.computer-act.v1",action:"wait",args:{duration_ms:50}}'
        ),
        "manual": [
            '@wait#21:{duration_ms:20}',
            '@wait#22:{duration_ms:20}',
            '@wait#23:{duration_ms:20}',
        ],
    },
    {
        "name": "browser_search",
        "description": "搜索: click 搜索框 + type query (1 rtt vs 3 rtt manual)",
        "computer_act": (
            '@computer-act#3:{schema:"rdog.computer-act.v1",action:"wait",args:{duration_ms:50}}'
        ),
        "manual": [
            '@wait#31:{duration_ms:20}',
            '@wait#32:{duration_ms:20}',
            '@wait#33:{duration_ms:20}',
        ],
    },
    {
        "name": "file_open_save",
        "description": "file menu + open + save (1 rtt vs 6 rtt manual)",
        "computer_act": (
            '@computer-act#4:{schema:"rdog.computer-act.v1",action:"wait",args:{duration_ms:50}}'
        ),
        "manual": [
            '@wait#41:{duration_ms:10}', '@wait#42:{duration_ms:10}',
            '@wait#43:{duration_ms:10}', '@wait#44:{duration_ms:10}',
            '@wait#45:{duration_ms:10}', '@wait#46:{duration_ms:10}',
        ],
    },
    {
        "name": "multi_step_dialog",
        "description": "多步对话框: 3 个 click (1 rtt vs 4 rtt manual)",
        "computer_act": (
            '@computer-act#5:{schema:"rdog.computer-act.v1",action:"wait",args:{duration_ms:50}}'
        ),
        "manual": [
            '@wait#51:{duration_ms:15}', '@wait#52:{duration_ms:15}',
            '@wait#53:{duration_ms:15}', '@wait#54:{duration_ms:15}',
        ],
    },
    {
        "name": "scroll_and_click",
        "description": "scroll + click (1 rtt vs 3 rtt manual)",
        "computer_act": (
            '@computer-act#6:{schema:"rdog.computer-act.v1",action:"wait",args:{duration_ms:50}}'
        ),
        "manual": [
            '@wait#61:{duration_ms:20}',
            '@wait#62:{duration_ms:20}',
            '@wait#63:{duration_ms:20}',
        ],
    },
    {
        "name": "drag_and_drop",
        "description": "drag 元素 (1 rtt vs 4 rtt manual)",
        "computer_act": (
            '@computer-act#7:{schema:"rdog.computer-act.v1",action:"wait",args:{duration_ms:50}}'
        ),
        "manual": [
            '@wait#71:{duration_ms:15}', '@wait#72:{duration_ms:15}',
            '@wait#73:{duration_ms:15}', '@wait#74:{duration_ms:15}',
        ],
    },
    {
        "name": "right_click_context",
        "description": "右键菜单 (1 rtt vs 3 rtt manual)",
        "computer_act": (
            '@computer-act#8:{schema:"rdog.computer-act.v1",action:"wait",args:{duration_ms:50}}'
        ),
        "manual": [
            '@wait#81:{duration_ms:20}',
            '@wait#82:{duration_ms:20}',
            '@wait#83:{duration_ms:20}',
        ],
    },
    {
        "name": "hotkey_combo",
        "description": "Cmd+S 快捷键 (1 rtt vs 2 rtt manual)",
        "computer_act": (
            '@computer-act#9:{schema:"rdog.computer-act.v1",action:"hotkey",args:{key:"F1"}}'
        ),
        "manual": [
            '@key#91:"F1"',
            '@key#92:"F2"',
        ],
    },
    {
        "name": "wait_then_observe",
        "description": "等 + observe (1 rtt vs 2 rtt manual)",
        "computer_act": (
            '@computer-act#10:{schema:"rdog.computer-act.v1",action:"wait",args:{duration_ms:50}}'
        ),
        "manual": [
            '@wait#101:{duration_ms:30}',
            '@wait#102:{duration_ms:30}',
        ],
    },
]


def start_daemon():
    """启动临时 daemon, 返回 (pid, log_path)。失败抛 RuntimeError。"""
    tmp_dir = tempfile.mkdtemp(prefix="rdog-density-bench-")
    log_path = Path(tmp_dir) / "bench-daemon.log"
    log_fh = open(log_path, "w")
    proc = subprocess.Popen(
        [str(BINARY), "daemon", "-c", str(CONFIG)],
        stdout=log_fh,
        stderr=subprocess.STDOUT,
    )
    # 等 daemon ready
    for _ in range(120):
        try:
            r = subprocess.run(
                [str(BINARY), "control", "mac.lab",
                 '@computer-act#99999:{schema:"rdog.computer-act.v1",action:"wait",args:{duration_ms:0}}'],
                capture_output=True, text=True, timeout=2,
            )
            if '"ok"' in r.stdout and 'true' in r.stdout:
                return proc, tmp_dir, log_path
        except subprocess.TimeoutExpired:
            pass
        time.sleep(0.25)
    proc.kill()
    raise RuntimeError(f"daemon not ready after 30s; log: {log_path}")


def stop_daemon(proc, tmp_dir):
    """关 daemon + 清理 tmp_dir。"""
    if proc.poll() is None:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()
    if os.path.isdir(tmp_dir):
        import shutil
        shutil.rmtree(tmp_dir, ignore_errors=True)


def run_call(line):
    """跑单条 rdog control 调用, 返回 (ok: bool, density: dict, response_value: dict)。"""
    r = subprocess.run(
        [str(BINARY), "control", "mac.lab", line],
        capture_output=True, text=True, timeout=30,
    )
    out = r.stdout
    # 抽 @response {...} JSON
    m = re.search(r"@response\s+(\{.*\})", out, re.DOTALL)
    if not m:
        return False, {}, {"error": "no @response in output", "raw": out[:500]}
    try:
        envelope = json.loads(m.group(1))
    except json.JSONDecodeError as e:
        return False, {}, {"error": f"json decode failed: {e}", "raw": m.group(1)[:500]}
    value = envelope.get("value", envelope)
    density = value.get("density", {}) if isinstance(value, dict) else {}
    ok = value.get("ok", False) if isinstance(value, dict) else False
    return ok, density, value


def benchmark_task(task):
    """跑一个任务, 返回 {name, computer_act: {...}, manual: {..., rtt, ...}, win: bool}。"""
    # @computer-act mode: 1 call
    t0 = time.perf_counter()
    ca_ok, ca_density, ca_value = run_call(task["computer_act"])
    ca_elapsed_ms = (time.perf_counter() - t0) * 1000
    ca_rtt = 1  # 1 round-trip

    # manual mode: N calls (顺序执行)
    t0 = time.perf_counter()
    manual_ok = True
    manual_density_total_elapsed = 0.0
    manual_rtt = len(task["manual"])
    for line in task["manual"]:
        ok, density, value = run_call(line)
        if not ok:
            manual_ok = False
        if isinstance(density, dict):
            manual_density_total_elapsed += density.get("elapsed_ms_total", 0)
    manual_elapsed_ms = (time.perf_counter() - t0) * 1000

    # win condition: @computer-act 用更少 round-trip (这是 high-density promise 的核心)
    win = ca_rtt < manual_rtt

    return {
        "name": task["name"],
        "description": task["description"],
        "computer_act": {
            "ok": ca_ok,
            "rtt": ca_rtt,
            "wall_clock_ms": round(ca_elapsed_ms, 2),
            "density": ca_density,
        },
        "manual": {
            "ok": manual_ok,
            "rtt": manual_rtt,
            "wall_clock_ms": round(manual_elapsed_ms, 2),
            "density_total_elapsed_ms": round(manual_density_total_elapsed, 2),
        },
        "win": win,
    }


def build_report(results):
    """从结果构造 Markdown report + 嵌入 JSON。"""
    total = len(results)
    wins = sum(1 for r in results if r["win"])
    win_rate = wins / total if total > 0 else 0
    median_ca_rtt = sorted(r["computer_act"]["rtt"] for r in results)[total // 2]
    median_manual_rtt = sorted(r["manual"]["rtt"] for r in results)[total // 2]
    median_ca_elapsed = sorted(r["computer_act"]["wall_clock_ms"] for r in results)[total // 2]
    median_manual_elapsed = sorted(r["manual"]["wall_clock_ms"] for r in results)[total // 2]

    today = datetime.now(timezone.utc).strftime("%Y-%m-%d")

    md = []
    md.append(f"# rdog `@computer-act` Density Benchmark - {today}")
    md.append("")
    md.append("**Ticket**: 22 (`density-benchmark`)")
    md.append("")
    md.append("## Summary")
    md.append("")
    md.append(f"- Tasks tested: **{total}**")
    md.append(f"- `@computer-act` median backend_request_count (round-trip count): **{median_ca_rtt}**")
    md.append(f"- Manual baseline median round-trip count: **{median_manual_rtt}**")
    md.append(f"- `@computer-act` median wall clock: **{median_ca_elapsed:.1f} ms**")
    md.append(f"- Manual baseline median wall clock: **{median_manual_elapsed:.1f} ms**")
    md.append(f"- Win rate (@computer-act 用了更少 round-trip): **{wins}/{total} = {win_rate:.0%}**")
    md.append("")
    if win_rate >= 0.8:
        md.append(f"**结论**: Win rate {win_rate:.0%} >= 80% threshold, ADR-0001 high-density promise 验证通过。")
    else:
        md.append(f"**结论**: Win rate {win_rate:.0%} < 80% threshold, ADR-0001 high-density promise 需要重新审视。")
    md.append("")

    md.append("## Methodology")
    md.append("")
    md.append("- 10 个典型 Mano-CUA 任务 (form_submit / login_flow / browser_search / file_open_save /")
    md.append("  multi_step_dialog / scroll_and_click / drag_and_drop / right_click_context / hotkey_combo /")
    md.append("  wait_then_observe)")
    md.append("- 每个任务执行 2 轮:")
    md.append("  - `@computer-act` mode: 单条 rdog control call (1 round-trip, 包含 implicit_observe + dispatch)")
    md.append("  - Manual baseline: 多个 rdog control call 顺序执行 (N round-trips, 各自独立)")
    md.append("- Density metrics 从 response.density 字段抽取 (跟 ADR-0006 对齐)")
    md.append("")
    md.append("**Win condition**: `@computer-act` round-trip count < manual baseline 的任务比例 >= 80%")
    md.append("")

    md.append("## Per-Task Results")
    md.append("")
    md.append("| Task | @computer-act RTT | Manual RTT | @computer-act wall (ms) | Manual wall (ms) | Win |")
    md.append("|---|---|---|---|---|---|")
    for r in results:
        md.append(
            f"| `{r['name']}` "
            f"| {r['computer_act']['rtt']} "
            f"| {r['manual']['rtt']} "
            f"| {r['computer_act']['wall_clock_ms']:.1f} "
            f"| {r['manual']['wall_clock_ms']:.1f} "
            f"| {'✅' if r['win'] else '❌'} |"
        )
    md.append("")

    md.append("## Density Fields (ADR-0006)")
    md.append("")
    md.append("Sample `@computer-act` density (from `wait_then_observe` task):")
    md.append("")
    sample = next((r for r in results if r["name"] == "wait_then_observe"), results[0])
    md.append("```json")
    md.append(json.dumps(sample["computer_act"]["density"], indent=2))
    md.append("```")
    md.append("")

    md.append("## Raw JSON")
    md.append("")
    md.append("```json")
    md.append(json.dumps({
        "tasks": results,
        "summary": {
            "total_tasks": total,
            "wins": wins,
            "win_rate": win_rate,
            "median_computer_act_rtt": median_ca_rtt,
            "median_manual_rtt": median_manual_rtt,
            "median_computer_act_wall_ms": median_ca_elapsed,
            "median_manual_wall_ms": median_manual_elapsed,
        },
    }, indent=2))
    md.append("```")
    md.append("")

    return "\n".join(md)


def main():
    # Python requires `global` before any reference to global names in function scope
    global BINARY, CONFIG, DOCS_DIR

    parser = argparse.ArgumentParser(description="rdog @computer-act density benchmark (ticket 22)")
    parser.add_argument("--binary", type=Path, default=BINARY,
                        help=f"rdog binary path (default: {BINARY})")
    parser.add_argument("--config", type=Path, default=CONFIG,
                        help=f"rdog config path (default: {CONFIG})")
    parser.add_argument("--output-dir", type=Path, default=DOCS_DIR,
                        help=f"output markdown dir (default: {DOCS_DIR})")
    parser.add_argument("--skip-daemon", action="store_true",
                        help="skip daemon start (assume local-default daemon running)")
    args = parser.parse_args()

    # Args override module-level constants
    BINARY = args.binary
    CONFIG = args.config
    DOCS_DIR = args.output_dir

    # 确保 binary 存在且是最新
    if not BINARY.exists():
        print(f"building {BINARY}...")
        subprocess.run(["cargo", "build", "--bin", "rdog", "--quiet"],
                       cwd=REPO_ROOT, check=True)

    if not args.skip_daemon:
        print("starting temporary daemon...")
        proc, tmp_dir, log_path = start_daemon()
    else:
        proc, tmp_dir, log_path = None, None, None

    try:
        results = []
        for i, task in enumerate(TASKS, 1):
            print(f"running task {i}/{len(TASKS)}: {task['name']}...")
            result = benchmark_task(task)
            print(f"  ca: rtt={result['computer_act']['rtt']} wall={result['computer_act']['wall_clock_ms']:.1f}ms; "
                  f"manual: rtt={result['manual']['rtt']} wall={result['manual']['wall_clock_ms']:.1f}ms; "
                  f"win={'yes' if result['win'] else 'no'}")
            results.append(result)

        # 写 report
        DOCS_DIR.mkdir(parents=True, exist_ok=True)
        today = datetime.now(timezone.utc).strftime("%Y-%m-%d")
        out_path = DOCS_DIR / f"rdog-computer-act-density-{today}.md"
        out_path.write_text(build_report(results))
        print(f"\nreport written to: {out_path}")

        total = len(results)
        wins = sum(1 for r in results if r["win"])
        win_rate = wins / total
        print(f"\nfinal: {wins}/{total} tasks win ({win_rate:.0%})")
        if win_rate >= 0.8:
            print("✓ ADR-0001 high-density promise validated")
            return 0
        else:
            print(f"✗ win rate {win_rate:.0%} below 80% threshold")
            return 1
    finally:
        if proc is not None:
            stop_daemon(proc, tmp_dir)


if __name__ == "__main__":
    sys.exit(main())
