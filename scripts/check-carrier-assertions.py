#!/usr/bin/env python3
"""载体断言对账 — skill / README / specs 里的坐标换算指引必须与 src/ 的真相源一致。

背景 (2026-10-22 实锤):
    `os_x = image_x + virtual_bounds.x` 这份过时的单段式换算公式, 曾经同时住在
    6 处载体里 (skill references/protocol.md ×2、skill references/control-workflow.md、
    README.md、specs/code-agent-rdog-control-usage.md、两份 coordinate plan spec)。
    代码侧的权威定义 (src/screenshot.rs 的 ScreenshotTransforms) 一直是对的, 但
    错的那份在 agent 实际会读的地方 —— Windows 4K@200% 下差 2 倍。
    根因: 载体是**手抄**的, 没有对账。

本脚本的判据:
    1. 权威公式从 src/screenshot.rs **抽取** (不硬编码), 载体必须与之字面一致
    2. 载体里出现 "用 virtual_bounds 做 click 坐标换算" 的写法 (公式或散文) 即失败,
       除非该行带明确的否定/校正标记 (作为"反例引用"保留是允许的)
    3. 脚本里的断言清单与 docs/agents/knowledge-carriers.md 的清单必须一致
       (清单本身也会漂移, 所以清单也要对账)

用法:
    python3 scripts/check-carrier-assertions.py [--repo-root .]
退出码: 0 = 全部一致; 1 = 有分叉/污染; 2 = 用法错误
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

# --------------------------------------------------------------------------
# 断言清单 —— 与 docs/agents/knowledge-carriers.md 一一对应
# --------------------------------------------------------------------------

# 断言 id -> (说明, 权威来源, 载体列表)
# 载体列表里的 "require" 表示该载体必须给出权威换算式 (或指向 manifest transforms);
# "scan" 表示只扫"旧式污染", 不强制它给公式 (设计稿/历史规格可以不写换算指引)。
ASSERTIONS: dict[str, dict] = {
    "coordinate-formula-composite": {
        "desc": "全桌面图的图上像素 → os-logical 换算式",
        "authoritative": "src/screenshot.rs (ScreenshotTransforms::composite)",
        "carriers": {
            "require": [
                ".codex/skills/rdog-control/references/protocol.md",
                ".codex/skills/rdog-control/references/control-workflow.md",
                "README.md",
                "specs/code-agent-rdog-control-usage.md",
                "specs/rdog-mouse-control-coordinate-plan.md",
                "specs/rdog-multi-display-screenshot-coordinate-plan.md",
            ],
            "scan": [
                "specs/windows-hidpi-coordinate-fix-plan.md",
                "specs/zenoh-screenshot-control-plan.md",
            ],
        },
    },
    "coordinate-formula-window": {
        "desc": "窗口裁剪图的换算式 (多一项 image_rect 平移)",
        "authoritative": "src/screenshot.rs (ScreenshotTransforms::window)",
        "carriers": {
            "require": [
                ".codex/skills/rdog-control/references/protocol.md",
            ],
            "scan": [],
        },
    },
    "coordinate-deprecated-formula": {
        "desc": "载体里不得把 virtual_bounds 当作 click 空间原点",
        "authoritative": "src/screenshot.rs (build_os_logical_bounds / build_virtual_bounds)",
        "carriers": {
            "require": [],
            "scan": [
                ".codex/skills/rdog-control/references/protocol.md",
                ".codex/skills/rdog-control/references/control-workflow.md",
                "README.md",
                "specs/code-agent-rdog-control-usage.md",
                "specs/rdog-mouse-control-coordinate-plan.md",
                "specs/rdog-multi-display-screenshot-coordinate-plan.md",
                "specs/windows-hidpi-coordinate-fix-plan.md",
                "specs/zenoh-screenshot-control-plan.md",
            ],
        },
    },
}

# 明确在否定/更正旧写法时, 允许保留旧式作为反例。
# 判定用**上下文窗口** (本行 + 前后各一行), 因为"校正"标记常写在引用块的标题行。
#
# 反向验证 (2026-10-22) 修掉的一个漏洞: 早期版本把 "not the" / "物理" / "历史"
# 也算免责标记, 结果一行里同时写着
#   "os_x = image_x + virtual_bounds.x ... virtual_bounds is not the origin"
# 的载体被判成"合规" —— 免责标记其实在否定**另一句话**。
# 所以免责词表收窄到"明确在讲这件事本身"的措辞。
SAFETY_MARKER = re.compile(
    r"校正|原先|旧式|旧写法|已废弃|不要|不得用|不再用|deprecat|wrong formula|"
    r"曾经|改成|instead of|rather than|改用",
    re.IGNORECASE,
)

# 载体里出现 "os_x =" / "os_y =" 说明这一行在**给公式**。
# 有公式就必须给对公式; 只写指针 (不提公式) 才允许走"指向 manifest transforms"的豁免。
FORMULA_PRESENT = re.compile(r"os_[xy]\s*=")

# "把 virtual_bounds 当 click 空间原点"的具体形态。
# 刻意收窄: 只匹配**明确的换算动作**, 避免把
#   - "virtual_bounds.x < 0 的平台验证" (负坐标说明)
#   - "virtual_bounds is OS-native and is not the click-space origin" (正确的否定陈述)
# 这类正当表述误判为污染。
DEPRECATED_PATTERNS = (
    re.compile(r"os_[xy]\s*=\s*[^`\n]*virtual_bounds"),                       # 公式行
    re.compile(r"(adding|加上|加到|相加)[^。\n]{0,40}virtual_bounds"),          # 散文: 相加
    re.compile(
        r"convert[^。\n]{0,60}(?:by|with|using|via)[^。\n]{0,20}virtual_bounds",
        re.IGNORECASE,
    ),                                                                          # 散文: 换算时用 virtual_bounds
    # 显式声称"把 virtual_bounds 当作原点"才命中 —— 不能只凭出现 "origin" 就判,
    # 否则 "virtual_bounds is not the click-space origin" 这种**正确的否定陈述**
    # 会被误判 (反向验证踩到的第二个坑)。
    re.compile(
        r"virtual_bounds[^。\n]{0,20}(?:作为|当作|为)[^。\n]{0,8}(?:原点|起点|origin)",
        re.IGNORECASE,
    ),
)

# 载体里出现这些片段, 视为"已指向 manifest 的自描述字段"
POINTER_MARKERS = (
    "transforms.image_to_os",
    "transforms.os_to_image",
    "transforms` ",
    "manifest 的 `transforms",
)


def normalize(text: str) -> str:
    """比较用: 去掉所有空白, 公式里的空格/换行不该影响判定。"""
    return re.sub(r"\s+", "", text)


def extract_transforms(root: Path) -> dict[str, dict[str, str]]:
    """从 src/screenshot.rs 抽取权威换算式 (真源在代码里, 不在脚本里)。

    返回 {"composite": {"x": expr, "y": expr}, "window": {...}} —— 按轴拆开,
    因为载体里通常只写 x 轴 ("same for y") 或把两轴分两行写。
    """
    source = root / "src" / "screenshot.rs"
    if not source.is_file():
        raise SystemExit(f"找不到权威来源: {source}")
    text = source.read_text(encoding="utf-8")

    out: dict[str, dict[str, str]] = {}
    for name in ("composite", "window"):
        block = re.search(
            r"fn\s+" + name + r"\(\)\s*->\s*Self\s*\{(.*?)\n    \}",
            text,
            re.DOTALL,
        )
        if not block:
            raise SystemExit(f"src/screenshot.rs 里找不到 ScreenshotTransforms::{name}")
        body = block.group(1)
        picked: dict[str, str] = {}
        for key in ("image_to_os", "os_to_image"):
            hit = re.search(key + r':\s*"([^"]+)"', body)
            if not hit:
                raise SystemExit(f"ScreenshotTransforms::{name} 里找不到 {key}")
            picked[key] = hit.group(1)
        axes: dict[str, str] = {}
        for statement in picked["image_to_os"].split(";"):
            axis_hit = re.match(r"\s*os_([xy])\s*=\s*(.+?)\s*$", statement)
            if not axis_hit:
                continue
            axes[axis_hit.group(1)] = axis_hit.group(2)
        if set(axes) != {"x", "y"}:
            raise SystemExit(f"ScreenshotTransforms::{name} 的 image_to_os 不是 x/y 两轴")
        out[name] = axes
    return out


def carrier_files(root: Path) -> set[str]:
    files: set[str] = set()
    for spec in ASSERTIONS.values():
        for group in spec["carriers"].values():
            files.update(group)
    return files


def check_positive(root: Path, transforms: dict[str, dict[str, str]], failures: list[str]) -> None:
    """载体给出的每个换算式, 必须与权威式一致; 且该给的载体必须给。

    判据细节 (每一条都是反向验证逼出来的):
    - **按轴 + 前缀比对**: 权威串在代码里是单行 "os_x = ...; os_y = ...", 载体里
      可能分两行、可能只写 x 轴 ("same for y")、可能公式本身跨行 —— 所以在
      **去空白** 之后做**前缀**匹配, 而不是整串相等
    - **反例引用放行**: "校正" / "不要" 语境下引用的旧式是**故意留的**, 不算分叉
    - **给了公式就必须给对**: 不能靠"别处提过 transforms"蒙混过关
    - **没给公式可以只给指针**: 指针不会过期, 手抄的会
    """
    variants = {
        "coordinate-formula-composite": ("composite", "全桌面图"),
        "coordinate-formula-window": ("window", "窗口裁剪图"),
    }
    # 每一轴的全部权威式 (载体可以给 composite 或 window 变体)
    authoritative_by_axis = {
        "x": {normalize(transforms[name]["x"]) for name, _ in variants.values()},
        "y": {normalize(transforms[name]["y"]) for name, _ in variants.values()},
    }
    expr_chars = re.compile(r"^[a-z0-9_.+*/()]+")

    for assertion_id, (variant, label) in variants.items():
        want = normalize(transforms[variant]["x"])
        for rel in ASSERTIONS[assertion_id]["carriers"]["require"]:
            path = root / rel
            if not path.is_file():
                failures.append(f"[{assertion_id}] 载体不存在: {rel}")
                continue
            body = path.read_text(encoding="utf-8")
            norm = normalize(body)
            # 覆盖: 该载体必须给出这个变体的换算式
            if want not in norm:
                failures.append(
                    f"[{assertion_id}] {rel} 未给出权威{label}换算式 "
                    f"(期望 os_x = {transforms[variant]['x']})"
                )
                continue
            # 正确性: 载体里出现的每个 os_x= / os_y= 都必须是某个权威式的前缀
            lines = body.splitlines()
            for axis in ("x", "y"):
                for match in re.finditer(r"os_" + axis + r"\s*=", body):
                    tail = normalize(body[match.end() : match.end() + 200])
                    expr_match = expr_chars.match(tail)
                    if not expr_match:
                        continue
                    expr = expr_match.group(0)
                    if any(expr.startswith(auth) for auth in authoritative_by_axis[axis]):
                        continue
                    line_no = body[: match.start()].count("\n") + 1
                    window = "\n".join(lines[max(0, line_no - 2) : line_no + 1])
                    if SAFETY_MARKER.search(window):
                        continue  # 反例引用 (校正/不要 语境), 故意留的
                    failures.append(
                        f"[{assertion_id}] {rel}:{line_no} 给出的 os_{axis} 换算式"
                        f"与 src/ 真相源不一致\n"
                        f"      载体: os_{axis} = {expr}\n"
                        f"      权威: {sorted(authoritative_by_axis[axis])}"
                    )


def check_deprecated(root: Path, failures: list[str]) -> None:
    """载体里不得把 virtual_bounds 当作 click 空间原点 (公式或散文都不行)。"""
    for rel in ASSERTIONS["coordinate-deprecated-formula"]["carriers"]["scan"]:
        path = root / rel
        if not path.is_file():
            failures.append(f"[coordinate-deprecated-formula] 载体不存在: {rel}")
            continue
        lines = path.read_text(encoding="utf-8").splitlines()
        for lineno, line in enumerate(lines, 1):
            if "virtual_bounds" not in line:
                continue
            if not any(pattern.search(line) for pattern in DEPRECATED_PATTERNS):
                continue
            # 安全标记按上下文窗口判定: "校正" 常写在引用块的标题行
            window = "\n".join(lines[max(0, lineno - 2) : lineno + 1])
            if SAFETY_MARKER.search(window):
                continue
            failures.append(
                f"[coordinate-deprecated-formula] {rel}:{lineno} "
                f"疑似把 virtual_bounds 当 click 空间原点: {line.strip()[:120]}"
            )


def check_inventory_sync(root: Path, failures: list[str]) -> None:
    """脚本里的断言清单与文档里的清单必须一致。"""
    doc = root / "docs" / "agents" / "knowledge-carriers.md"
    if not doc.is_file():
        failures.append("缺少 docs/agents/knowledge-carriers.md (断言清单的载体)")
        return
    text = doc.read_text(encoding="utf-8")
    for assertion_id in ASSERTIONS:
        if assertion_id not in text:
            failures.append(f"文档未列出断言 {assertion_id} (docs/agents/knowledge-carriers.md)")
    for found in re.findall(r"`(coordinate-[a-z-]+)`", text):
        if found not in ASSERTIONS:
            failures.append(f"文档列出的断言 {found} 在脚本里不存在")


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="载体断言对账")
    parser.add_argument("--repo-root", default=".", help="仓库根目录")
    args = parser.parse_args(argv)

    root = Path(args.repo_root).resolve()
    failures: list[str] = []

    try:
        transforms = extract_transforms(root)
    except SystemExit as exc:
        print(f"FAIL: {exc}", file=sys.stderr)
        return 2

    check_positive(root, transforms, failures)
    check_deprecated(root, failures)
    check_inventory_sync(root, failures)

    print("权威来源: src/screenshot.rs")
    for variant in ("composite", "window"):
        for axis in ("x", "y"):
            print(f"  {variant}.os_{axis} = {transforms[variant][axis]}")
    print(f"断言数: {len(ASSERTIONS)}  载体数: {len(carrier_files(root))}")

    if failures:
        print(f"\nFAIL: {len(failures)} 处分叉", file=sys.stderr)
        for item in failures:
            print(f"  {item}", file=sys.stderr)
        return 1

    print("OK: 载体断言与 src/ 真相源一致")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
