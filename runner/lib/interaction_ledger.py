"""Interaction ledger classifier (rdog.macos-ops.interaction-ledger.v1).

每个 agent decision (bash tool call) 分类到 6 档之一:
- query: 只读协议请求, 不是动作后验证或错误恢复
- action: 含通用状态改变 verb 的请求
- post_action_evidence: 动作后只读请求
- recovery: 紧接同一 attempt rdog/tool 错误后
- supporting_shell: 非 rdog control bash 调用 (sleep, mkdir, etc.)
- unknown: 无法可靠归类 (app/case/prompt 不读)

分类只用通用协议 verb + 错误响应 + 相邻请求顺序, 不读 app/case/expected_result.
rdogRequest (1 invocation) ≠ requestCount (含多 frame 的 invocation 算 1).
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field
from typing import Iterator


# 通用状态改变 verb (action). 不含 query 类.
ACTION_VERBS = {
    "@open-app", "@cmd", "@key", "@ax-press", "@ax-action", "@ax-set-value",
    "@type-text", "@mouse-move", "@mouse-button", "@click", "@drag", "@wheel",
    "@scroll", "@hotkey", "@hotkey-click", "@wait",
    "@window-resize", "@window-close", "@window-activate",
    "@computer-act",
}

# 只读协议 verb (query). 动作后验证 (post_action_evidence) 不在这一层.
QUERY_VERBS = {
    "@ping", "@capabilities", "@bootstrap", "@observe", "@window-find",
    "@ax-find", "@ax-get", "@screenshot", "@ax-tree", "@ax-diff",
}


@dataclass
class BashDecision:
    model_id: str
    case_id: str
    attempt: int
    seq: int
    raw_command: str
    classification: str
    rdog_response: str = ""
    rdog_error: str = ""
    artifact_path: str = ""

    def to_dict(self) -> dict:
        return {
            "model_id": self.model_id,
            "case_id": self.case_id,
            "attempt": self.attempt,
            "seq": self.seq,
            "raw_command": self.raw_command,
            "classification": self.classification,
            "rdog_response": self.rdog_response,
            "rdog_error": self.rdog_error,
            "artifact_path": self.artifact_path,
        }


def _strip_heredoc_body(command: str) -> str:
    """Strip heredoc body from a bash command so shell-shape classification
    doesn't get confused by apostrophes inside <<EOF ... EOF blocks."""
    return re.sub(r"<<-?\s*['\"]?(\w+)['\"]?\n.*?\n\s*\1", "", command, flags=re.DOTALL)


def _extract_rdog_frames(command: str) -> list[str]:
    """Extract unique `rdog control ...` invocations from a bash command.
    A single bash invocation with multiple control frames counts as 1 request."""
    return list(dict.fromkeys(re.findall(r"rdog\s+control\s+\S+(?:\s+['\"]?(?:[^'\"\\]|\\.)*?['\"]?)*", command)))


def _classify_frame(frame: str) -> str:
    """Classify a single rdog control invocation."""
    cmd = _strip_heredoc_body(frame)
    # Verify contains an action verb?
    has_action = any(verb in cmd for verb in ACTION_VERBS)
    has_query = any(verb in cmd for verb in QUERY_VERBS)
    if has_action and not has_query:
        return "action"
    if has_query and not has_action:
        return "query"
    if has_action and has_query:
        # ambiguous; contains both verbs. fall back to "action" because in our
        # 13-action lane a verify=best_effort always sends @computer-act with
        # implicit @observe preflight. treat as action with embedded query.
        return "action"
    # No recognized verb. classify as unknown; refuse to read app/case/prompt.
    return "unknown"


def classify_bash_command(command: str, *, recovery_after_error: bool) -> str:
    """Classify one bash command line into ledger classification.

    - If command contains `rdog control`, extract frames and pick the dominant
      class (action / query / mixed). Recovery flag promotes to "recovery".
    - If command is non-rdog shell (sleep, mkdir, etc.), return
      "supporting_shell".
    - Else "unknown".
    """
    if recovery_after_error:
        return "recovery"
    frames = _extract_rdog_frames(command)
    if not frames:
        return "supporting_shell"
    classes = {_classify_frame(f) for f in frames}
    if "unknown" in classes:
        return "unknown"
    if classes == {"action"}:
        return "action"
    if classes == {"query"}:
        return "query"
    return "action"  # mixed / multi-class falls back to action (rdogRequestCount).


@dataclass
class InteractionLedger:
    """Per-model-per-case attempt ledger."""
    model_id: str
    case_id: str
    decisions: list[BashDecision] = field(default_factory=list)
    _last_error: bool = False

    def record(self, command: str, *, rdog_response: str = "", rdog_error: str = "",
               artifact_path: str = "") -> BashDecision:
        classification = classify_bash_command(
            command, recovery_after_error=self._last_error
        )
        decision = BashDecision(
            model_id=self.model_id,
            case_id=self.case_id,
            attempt=self._attempt,
            seq=len(self.decisions) + 1,
            raw_command=command,
            classification=classification,
            rdog_response=rdog_response[:200] if rdog_response else "",
            rdog_error=rdog_error[:200] if rdog_error else "",
            artifact_path=artifact_path,
        )
        self.decisions.append(decision)
        # Set error flag for next decision.
        self._last_error = bool(rdog_error) or "code:64" in (rdog_response or "")
        return decision

    def summary(self) -> dict:
        agent_decisions = len(self.decisions)
        rdog_requests = sum(
            1 for d in self.decisions if d.classification != "supporting_shell"
        )
        by_class: dict[str, int] = {}
        for d in self.decisions:
            by_class[d.classification] = by_class.get(d.classification, 0) + 1
        return {
            "model_id": self.model_id,
            "case_id": self.case_id,
            "agent_decisions": agent_decisions,
            "rdog_requests": rdog_requests,
            "by_class": by_class,
        }
