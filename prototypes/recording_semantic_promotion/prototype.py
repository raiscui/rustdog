#!/usr/bin/env python3
"""PROTOTYPE — 用可审阅 trace 验证 Recorder semantic promotion policy。"""

from __future__ import annotations

import argparse
import copy
import json
import sys
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any


SCHEMA = "rdog.semantic-promotion.prototype.v1"
DECISIONS = {"semantic", "parameterized-semantic", "guarded-coordinate", "reject"}
ACTIONS = {"click", "text", "shortcut", "scroll", "drag"}


def load_suite(path: Path) -> list[dict[str, Any]]:
    """读取 fixture,并拒绝会把 prototype 变成隐式评分器的字段。"""
    document = json.loads(path.read_text(encoding="utf-8"))
    if document.get("schema") != SCHEMA:
        raise ValueError(f"unsupported schema: {document.get('schema')!r}")

    # Journal candidate 不允许保存无证据 confidence。Prototype 同样只使用可数证据。
    if "confidence" in json.dumps(document, ensure_ascii=False).lower():
        raise ValueError("fixture must not contain confidence fields")

    scenarios = document.get("scenarios")
    if not isinstance(scenarios, list) or not scenarios:
        raise ValueError("scenarios must be a non-empty array")

    seen: set[str] = set()
    for scenario in scenarios:
        validate_scenario(scenario, seen)
        seen.add(scenario["id"])
    return scenarios


def validate_scenario(scenario: dict[str, Any], seen: set[str]) -> None:
    """只验证 decision prototype 真正依赖的输入,不造通用 schema framework。"""
    required = {
        "id",
        "title",
        "action",
        "tags",
        "physical",
        "semantic",
        "freshness",
        "window",
        "display",
        "verification",
        "action_data",
        "expected_decision",
    }
    missing = sorted(required - scenario.keys())
    if missing:
        raise ValueError(f"{scenario.get('id', '<unknown>')}: missing {missing}")
    if scenario["id"] in seen:
        raise ValueError(f"duplicate scenario id: {scenario['id']}")
    if scenario["action"] not in ACTIONS:
        raise ValueError(f"{scenario['id']}: unsupported action {scenario['action']}")
    if scenario["expected_decision"] not in DECISIONS:
        raise ValueError(f"{scenario['id']}: invalid expected decision")

    # Sensitive fixture 只能带 parameter id,不能为了演示方便夹带 secret literal。
    data = scenario["action_data"]
    if data.get("classification") in {"sensitive", "unknown"} and data.get("literal") is not None:
        raise ValueError(f"{scenario['id']}: sensitive fixture contains literal")


def snapshot(state: dict[str, Any], history: list[dict[str, Any]]) -> None:
    """每次 gate 变化后复制完整状态,让 human 能回看 decision 是怎么形成的。"""
    history.append(copy.deepcopy(state))


def transition(
    state: dict[str, Any],
    history: list[dict[str, Any]],
    phase: str,
    **changes: Any,
) -> None:
    state["phase"] = phase
    state.update(changes)
    snapshot(state, history)


def has_verification(scenario: dict[str, Any]) -> bool:
    required = set(scenario["verification"]["required"])
    available = set(scenario["verification"]["available"])
    return required.issubset(available) and bool(required)


def common_target_gate(scenario: dict[str, Any]) -> tuple[bool, list[str]]:
    window = scenario["window"]
    reasons: list[str] = []
    if not window["identity_resolved"]:
        reasons.append("window identity unresolved")
    if not window["ownership_match"]:
        reasons.append("target ownership mismatch")
    if not window["focus_verified"]:
        reasons.append("focus not freshly verified")
    return not reasons, reasons


def semantic_gate(scenario: dict[str, Any], capability: str) -> tuple[bool, list[str]]:
    semantic = scenario["semantic"]
    freshness = scenario["freshness"]
    reasons: list[str] = []

    if semantic["candidate_count"] != 1:
        reasons.append(f"semantic candidate count={semantic['candidate_count']}")
    if not semantic["candidate_owned"]:
        reasons.append("semantic candidate is not target-owned")
    if semantic["durable_selector"] is None:
        reasons.append("durable selector missing")
    if semantic["action_capability"] != capability:
        reasons.append(f"required capability {capability} missing")
    if freshness["execution_refind"] not in {"unique", "not_needed"}:
        reasons.append(f"execution refind={freshness['execution_refind']}")

    target_ok, target_reasons = common_target_gate(scenario)
    reasons.extend(target_reasons)
    return not reasons and target_ok, reasons


def coordinate_gate(scenario: dict[str, Any]) -> tuple[bool, list[str]]:
    """坐标不是“语义失败就用”,而是一条拥有独立完整前置条件的路径。"""
    semantic = scenario["semantic"]
    freshness = scenario["freshness"]
    window = scenario["window"]
    display = scenario["display"]
    reasons: list[str] = []

    # 只要录制期存在语义身份,执行期失配就属于 stale target,不能偷降级到旧坐标。
    if semantic["candidate_count"] != 0:
        reasons.append("coordinate fallback requires zero semantic candidates")
    if semantic["durable_selector"] is not None:
        reasons.append("stale semantic selector forbids coordinate fallback")
    if semantic["captured_ref"] is not None:
        reasons.append("captured semantic ref forbids coordinate fallback")
    if freshness["observation_age_ms"] > freshness["observation_ttl_ms"]:
        reasons.append("coordinate evidence is stale")

    target_ok, target_reasons = common_target_gate(scenario)
    reasons.extend(target_reasons)
    if not window["geometry_precondition"]:
        reasons.append("window geometry precondition missing")
    if not window["rect_fresh"]:
        reasons.append("window rect is stale")
    if not display["topology_match"]:
        reasons.append("display topology mismatch")
    if not display["guard_present"]:
        reasons.append("display guard missing")
    if not display["points_valid"]:
        reasons.append("point or path leaves valid displays")
    if not has_verification(scenario):
        reasons.append("fresh post-action verifier missing")
    return not reasons and target_ok, reasons


def is_ambiguous(scenario: dict[str, Any]) -> bool:
    semantic = scenario["semantic"]
    return semantic["candidate_count"] > 1 or scenario["freshness"]["execution_refind"] == "ambiguous"


def semantic_click_command(scenario: dict[str, Any]) -> dict[str, Any]:
    selector = scenario["semantic"]["durable_selector"]
    if "web" in scenario["tags"]:
        return {
            "command": "@web-act",
            "target_resolution": "durable selector -> fresh AXWebArea match",
            "payload": {"match": selector, "action": "press", "verify": True},
        }
    return {
        "command": "@ax-action",
        "target_resolution": "durable selector -> fresh AX id",
        "payload": {"action": "AXPress"},
    }


def coordinate_command(scenario: dict[str, Any]) -> dict[str, Any]:
    action = scenario["action"]
    physical = scenario["physical"]
    data = scenario["action_data"]
    guard = {"display": {"id": scenario["display"]["display_id"]}}

    if action == "click":
        point = physical["point"]
        payload = {
            "x": point["x"],
            "y": point["y"],
            "button": data["button"],
            "count": data["count"],
            "coordinate_space": "os-logical",
            "guard": guard,
        }
        return {"command": "@click", "payload": payload}
    if action == "scroll":
        point = physical["point"]
        payload = {
            "x": point["x"],
            "y": point["y"],
            "delta_y": data["delta_y"],
            "coordinate_space": "os-logical",
            "guard": guard,
        }
        return {"command": "@wheel", "payload": payload}
    if action == "drag":
        payload = {
            "from": data["from"],
            "to": data["to"],
            "duration_ms": data["duration_ms"],
            "steps": data["steps"],
            "coordinate_space": "os-logical",
            "guard": guard,
        }
        return {"command": "@drag", "payload": payload}
    raise ValueError(f"coordinate fallback unsupported for {action}")


def decide_click(scenario: dict[str, Any]) -> tuple[str, dict[str, Any] | None, list[str]]:
    # Ambiguity 是独立硬阻断。即使旧坐标仍在窗口内,也不能证明它仍指向原目标。
    if is_ambiguous(scenario):
        return "reject", None, ["ambiguous semantic target; coordinate fallback forbidden"]

    semantic_ok, semantic_reasons = semantic_gate(scenario, "AXPress")
    if semantic_ok and has_verification(scenario):
        return "semantic", semantic_click_command(scenario), ["unique owned AXPress target"]

    coordinate_ok, coordinate_reasons = coordinate_gate(scenario)
    if coordinate_ok:
        return "guarded-coordinate", coordinate_command(scenario), semantic_reasons
    return "reject", None, semantic_reasons + coordinate_reasons


def decide_text(scenario: dict[str, Any]) -> tuple[str, dict[str, Any] | None, list[str]]:
    semantic = scenario["semantic"]
    data = scenario["action_data"]
    _, reasons = common_target_gate(scenario)
    if semantic["candidate_count"] != 1:
        reasons.append(f"semantic candidate count={semantic['candidate_count']}")
    if not semantic["candidate_owned"]:
        reasons.append("text candidate is not target-owned")
    if semantic["durable_selector"] is None:
        reasons.append("text durable selector missing")
    if semantic["action_capability"] not in {"AXValue", "TypeText"}:
        reasons.append("text action capability missing")
    if scenario["freshness"]["execution_refind"] != "unique":
        reasons.append("text target cannot be freshly resolved")
    if not has_verification(scenario):
        reasons.append("text post-action verifier missing")
    if reasons:
        return "reject", None, reasons

    base = {
        "command": "TypeText",
        "target_resolution": "durable selector -> fresh text target",
        "payload": {"mode": "auto", "allow_clipboard": False},
    }
    if data["classification"] == "ordinary" and data["committed"]:
        base["payload"]["text"] = {"literal": data["literal"]}
        return "semantic", base, ["ordinary final committed text confirmed"]

    # 参数值不在 fixture 或 command trace 中出现,这里只保留 canonical parameter id。
    if not data["parameter_id"]:
        return "reject", None, ["replay parameter id missing"]
    base["payload"]["text"] = {"parameter": data["parameter_id"]}
    return "parameterized-semantic", base, ["text value must be supplied at replay"]


def decide_shortcut(scenario: dict[str, Any]) -> tuple[str, dict[str, Any] | None, list[str]]:
    semantic = scenario["semantic"]
    data = scenario["action_data"]
    _, reasons = common_target_gate(scenario)
    if data["redaction_active"] or not data["classified_non_text"]:
        reasons.append("shortcut classification is unsafe or redacted")
    if not semantic["candidate_owned"]:
        reasons.append("shortcut target is not owned")
    if semantic["durable_selector"] is None:
        reasons.append("shortcut window selector missing")
    if semantic["action_capability"] != "KeyDelivery":
        reasons.append("shortcut KeyDelivery capability missing")
    if scenario["freshness"]["execution_refind"] != "unique":
        reasons.append("target window cannot be freshly resolved")
    if not has_verification(scenario):
        reasons.append("shortcut post-action verifier missing")
    if reasons:
        return "reject", None, reasons

    command = {
        "command": "@key",
        "target_resolution": "durable window selector -> fresh window id",
        "payload": {"key": data["chord"], "delivery": "window-targeted"},
    }
    return "semantic", command, ["explicit non-text chord with verified target window"]


def decide_scroll(scenario: dict[str, Any]) -> tuple[str, dict[str, Any] | None, list[str]]:
    if is_ambiguous(scenario):
        return "reject", None, ["ambiguous scroll container"]
    semantic_ok, semantic_reasons = semantic_gate(scenario, "AXScroll")
    if semantic_ok and has_verification(scenario):
        data = scenario["action_data"]
        command = {
            "command": "@ax-scroll",
            "target_resolution": "durable selector -> fresh scroll container",
            "payload": {"direction": data["direction"], "pages": data["pages"]},
        }
        return "semantic", command, ["unique AX scroll container"]

    coordinate_ok, coordinate_reasons = coordinate_gate(scenario)
    if coordinate_ok:
        return "guarded-coordinate", coordinate_command(scenario), semantic_reasons
    return "reject", None, semantic_reasons + coordinate_reasons


def decide_drag(scenario: dict[str, Any]) -> tuple[str, dict[str, Any] | None, list[str]]:
    # 当前没有通用 semantic drag。Canvas/free-space drag直接评估独立坐标门禁。
    coordinate_ok, reasons = coordinate_gate(scenario)
    if coordinate_ok:
        return "guarded-coordinate", coordinate_command(scenario), ["no generic semantic drag lane"]
    return "reject", None, ["no generic semantic drag lane"] + reasons


def evaluate(scenario: dict[str, Any]) -> dict[str, Any]:
    state: dict[str, Any] = {
        "scenario_id": scenario["id"],
        "phase": "ingest",
        "action": scenario["action"],
        "captured_observation_ref": scenario["semantic"]["captured_ref"],
        "observation_ref_persisted": False,
        "gates": {},
        "decision": None,
        "command_preview": None,
        "reasons": [],
    }
    history: list[dict[str, Any]] = []
    snapshot(state, history)

    # 录制期 ref只保留为trace provenance。Replay command永远不复制它。
    transition(
        state,
        history,
        "drop-ephemeral-ref",
        captured_observation_ref=None,
        observation_ref_persisted=False,
    )

    semantic = scenario["semantic"]
    transition(
        state,
        history,
        "inspect-semantic-candidates",
        gates={
            "candidate_count": semantic["candidate_count"],
            "candidate_owned": semantic["candidate_owned"],
            "selector_present": semantic["durable_selector"] is not None,
            "action_capability": semantic["action_capability"],
            "execution_refind": scenario["freshness"]["execution_refind"],
        },
    )

    decision_fn = {
        "click": decide_click,
        "text": decide_text,
        "shortcut": decide_shortcut,
        "scroll": decide_scroll,
        "drag": decide_drag,
    }[scenario["action"]]
    decision, command, reasons = decision_fn(scenario)

    transition(
        state,
        history,
        "verify-postcondition-lane",
        gates={
            **state["gates"],
            "required_verifiers": scenario["verification"]["required"],
            "available_verifiers": scenario["verification"]["available"],
            "verification_ready": has_verification(scenario),
        },
    )
    transition(
        state,
        history,
        "decision",
        decision=decision,
        command_preview=command,
        reasons=dedupe(reasons),
    )

    result = {
        "scenario": scenario,
        "history": history,
        "final": history[-1],
        "matched_expected": decision == scenario["expected_decision"],
    }
    validate_output(result)
    return result


def dedupe(items: list[str]) -> list[str]:
    return list(dict.fromkeys(item for item in items if item))


def validate_output(result: dict[str, Any]) -> None:
    scenario = result["scenario"]
    final = result["final"]
    command_text = json.dumps(final["command_preview"], ensure_ascii=False)
    captured_ref = scenario["semantic"]["captured_ref"]
    if captured_ref and captured_ref in command_text:
        raise ValueError(f"{scenario['id']}: command persisted observation ref")
    if final["decision"] != "reject" and not has_verification(scenario):
        raise ValueError(f"{scenario['id']}: executable decision lacks verifier")
    if not result["matched_expected"]:
        raise ValueError(
            f"{scenario['id']}: expected {scenario['expected_decision']}, got {final['decision']}"
        )


def build_summary(results: list[dict[str, Any]]) -> dict[str, Any]:
    decisions = Counter(result["final"]["decision"] for result in results)
    by_action: dict[str, Counter[str]] = defaultdict(Counter)
    for result in results:
        by_action[result["scenario"]["action"]][result["final"]["decision"]] += 1

    return {
        "scenario_count": len(results),
        "decision_counts": dict(sorted(decisions.items())),
        "by_action": {action: dict(sorted(counts.items())) for action, counts in sorted(by_action.items())},
        "edge_coverage": {
            tag: sum(tag in result["scenario"]["tags"] for result in results)
            for tag in ("ambiguity", "stale_target", "dynamic_page", "no_ax")
        },
        "unsafe_fallback_prevented": sum(
            result["final"]["decision"] == "reject"
            and result["scenario"]["action"] in {"click", "drag"}
            for result in results
        ),
        "executable_with_verifier": sum(
            result["final"]["decision"] != "reject" and has_verification(result["scenario"])
            for result in results
        ),
        "persisted_observation_ref_count": sum(
            result["final"]["observation_ref_persisted"] for result in results
        ),
    }


def render_text(results: list[dict[str, Any]], include_history: bool) -> None:
    for result in results:
        scenario = result["scenario"]
        final = result["final"]
        print(f"\n=== {scenario['id']} | {scenario['title']} ===")
        if include_history:
            for index, state in enumerate(result["history"], start=1):
                print(f"\n-- state {index}: {state['phase']} --")
                print(json.dumps(state, ensure_ascii=False, indent=2, sort_keys=True))
        print(
            f"decision={final['decision']} expected={scenario['expected_decision']} "
            f"matched={result['matched_expected']}"
        )
        print(f"reasons={json.dumps(final['reasons'], ensure_ascii=False)}")
        print(f"command={json.dumps(final['command_preview'], ensure_ascii=False, sort_keys=True)}")

    print("\n=== suite summary ===")
    print(json.dumps(build_summary(results), ensure_ascii=False, indent=2, sort_keys=True))


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    selection = parser.add_mutually_exclusive_group()
    selection.add_argument("--all", action="store_true", help="运行全部场景,也是默认行为")
    selection.add_argument("--scenario", help="只运行一个 scenario id")
    selection.add_argument("--list", action="store_true", help="列出场景")
    parser.add_argument("--format", choices=("text", "json"), default="text")
    parser.add_argument(
        "--summary-only",
        action="store_true",
        help="文本模式只打印最终decision和suite summary,省略完整state history",
    )
    parser.add_argument(
        "--fixtures",
        type=Path,
        default=Path(__file__).with_name("scenarios.json"),
        help="fixture JSON路径",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        scenarios = load_suite(args.fixtures)
        if args.list:
            for scenario in scenarios:
                print(f"{scenario['id']}\t{scenario['action']}\t{scenario['title']}")
            return 0
        if args.scenario:
            scenarios = [scenario for scenario in scenarios if scenario["id"] == args.scenario]
            if not scenarios:
                raise ValueError(f"unknown scenario: {args.scenario}")

        results = [evaluate(scenario) for scenario in scenarios]
        if args.format == "json":
            print(
                json.dumps(
                    {"schema": SCHEMA, "results": results, "summary": build_summary(results)},
                    ensure_ascii=False,
                    indent=2,
                    sort_keys=True,
                )
            )
        else:
            render_text(results, include_history=not args.summary_only)
        return 0
    except (OSError, ValueError, json.JSONDecodeError) as error:
        print(f"prototype error: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
