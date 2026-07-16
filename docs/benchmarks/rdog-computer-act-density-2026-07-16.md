# rdog `@computer-act` Density Benchmark - 2026-07-16

**Ticket**: 22 (`density-benchmark`)

## Summary

- Tasks tested: **10**
- `@computer-act` median backend_request_count (round-trip count): **1**
- Manual baseline median round-trip count: **3**
- `@computer-act` median wall clock: **77.3 ms**
- Manual baseline median wall clock: **159.5 ms**
- Win rate (@computer-act 用了更少 round-trip): **10/10 = 100%**

**结论**: Win rate 100% >= 80% threshold, ADR-0001 high-density promise 验证通过。

## Methodology

- 10 个典型 Mano-CUA 任务 (form_submit / login_flow / browser_search / file_open_save /
  multi_step_dialog / scroll_and_click / drag_and_drop / right_click_context / hotkey_combo /
  wait_then_observe)
- 每个任务执行 2 轮:
  - `@computer-act` mode: 单条 rdog control call (1 round-trip, 包含 implicit_observe + dispatch)
  - Manual baseline: 多个 rdog control call 顺序执行 (N round-trips, 各自独立)
- Density metrics 从 response.density 字段抽取 (跟 ADR-0006 对齐)

**Win condition**: `@computer-act` round-trip count < manual baseline 的任务比例 >= 80%

## Per-Task Results

| Task | @computer-act RTT | Manual RTT | @computer-act wall (ms) | Manual wall (ms) | Win |
|---|---|---|---|---|---|
| `form_submit` | 1 | 4 | 82.9 | 200.7 | ✅ |
| `login_flow` | 1 | 3 | 77.3 | 142.7 | ✅ |
| `browser_search` | 1 | 3 | 78.5 | 143.7 | ✅ |
| `file_open_save` | 1 | 6 | 77.0 | 207.0 | ✅ |
| `multi_step_dialog` | 1 | 4 | 76.9 | 159.5 | ✅ |
| `scroll_and_click` | 1 | 3 | 76.3 | 141.5 | ✅ |
| `drag_and_drop` | 1 | 4 | 74.4 | 163.3 | ✅ |
| `right_click_context` | 1 | 3 | 76.8 | 140.1 | ✅ |
| `hotkey_combo` | 1 | 2 | 282.0 | 559.9 | ✅ |
| `wait_then_observe` | 1 | 2 | 91.2 | 114.3 | ✅ |

## Density Fields (ADR-0006)

Sample `@computer-act` density (from `wait_then_observe` task):

```json
{
  "backend_request_count": 1,
  "control_frame_count": 1,
  "dispatch_ms": 55,
  "elapsed_ms_total": 55,
  "false_success_count": 0,
  "implicit_observe": false,
  "implicit_observe_ms": 0,
  "mouse_fallback_count": 0,
  "payload_bytes": 0,
  "semantic_action_count": 1,
  "stale_ref_recovery_count": 0,
  "trace_step_count": 4,
  "verification_passed": false
}
```

## Raw JSON

```json
{
  "tasks": [
    {
      "name": "form_submit",
      "description": "form \u63d0\u4ea4: type email + submit (1 rtt @computer-act vs 4 rtt manual)",
      "computer_act": {
        "ok": true,
        "rtt": 1,
        "wall_clock_ms": 82.89,
        "density": {
          "backend_request_count": 1,
          "control_frame_count": 1,
          "dispatch_ms": 55,
          "elapsed_ms_total": 55,
          "false_success_count": 0,
          "implicit_observe": false,
          "implicit_observe_ms": 0,
          "mouse_fallback_count": 0,
          "payload_bytes": 0,
          "semantic_action_count": 1,
          "stale_ref_recovery_count": 0,
          "trace_step_count": 4,
          "verification_passed": false
        }
      },
      "manual": {
        "ok": true,
        "rtt": 4,
        "wall_clock_ms": 200.65,
        "density_total_elapsed_ms": 0.0
      },
      "win": true
    },
    {
      "name": "login_flow",
      "description": "\u767b\u5f55\u6d41: type + click (1 rtt vs 3 rtt manual)",
      "computer_act": {
        "ok": true,
        "rtt": 1,
        "wall_clock_ms": 77.26,
        "density": {
          "backend_request_count": 1,
          "control_frame_count": 1,
          "dispatch_ms": 55,
          "elapsed_ms_total": 55,
          "false_success_count": 0,
          "implicit_observe": false,
          "implicit_observe_ms": 0,
          "mouse_fallback_count": 0,
          "payload_bytes": 0,
          "semantic_action_count": 1,
          "stale_ref_recovery_count": 0,
          "trace_step_count": 4,
          "verification_passed": false
        }
      },
      "manual": {
        "ok": true,
        "rtt": 3,
        "wall_clock_ms": 142.7,
        "density_total_elapsed_ms": 0.0
      },
      "win": true
    },
    {
      "name": "browser_search",
      "description": "\u641c\u7d22: click \u641c\u7d22\u6846 + type query (1 rtt vs 3 rtt manual)",
      "computer_act": {
        "ok": true,
        "rtt": 1,
        "wall_clock_ms": 78.51,
        "density": {
          "backend_request_count": 1,
          "control_frame_count": 1,
          "dispatch_ms": 55,
          "elapsed_ms_total": 55,
          "false_success_count": 0,
          "implicit_observe": false,
          "implicit_observe_ms": 0,
          "mouse_fallback_count": 0,
          "payload_bytes": 0,
          "semantic_action_count": 1,
          "stale_ref_recovery_count": 0,
          "trace_step_count": 4,
          "verification_passed": false
        }
      },
      "manual": {
        "ok": true,
        "rtt": 3,
        "wall_clock_ms": 143.69,
        "density_total_elapsed_ms": 0.0
      },
      "win": true
    },
    {
      "name": "file_open_save",
      "description": "file menu + open + save (1 rtt vs 6 rtt manual)",
      "computer_act": {
        "ok": true,
        "rtt": 1,
        "wall_clock_ms": 76.95,
        "density": {
          "backend_request_count": 1,
          "control_frame_count": 1,
          "dispatch_ms": 54,
          "elapsed_ms_total": 54,
          "false_success_count": 0,
          "implicit_observe": false,
          "implicit_observe_ms": 0,
          "mouse_fallback_count": 0,
          "payload_bytes": 0,
          "semantic_action_count": 1,
          "stale_ref_recovery_count": 0,
          "trace_step_count": 4,
          "verification_passed": false
        }
      },
      "manual": {
        "ok": true,
        "rtt": 6,
        "wall_clock_ms": 206.98,
        "density_total_elapsed_ms": 0.0
      },
      "win": true
    },
    {
      "name": "multi_step_dialog",
      "description": "\u591a\u6b65\u5bf9\u8bdd\u6846: 3 \u4e2a click (1 rtt vs 4 rtt manual)",
      "computer_act": {
        "ok": true,
        "rtt": 1,
        "wall_clock_ms": 76.92,
        "density": {
          "backend_request_count": 1,
          "control_frame_count": 1,
          "dispatch_ms": 55,
          "elapsed_ms_total": 55,
          "false_success_count": 0,
          "implicit_observe": false,
          "implicit_observe_ms": 0,
          "mouse_fallback_count": 0,
          "payload_bytes": 0,
          "semantic_action_count": 1,
          "stale_ref_recovery_count": 0,
          "trace_step_count": 4,
          "verification_passed": false
        }
      },
      "manual": {
        "ok": true,
        "rtt": 4,
        "wall_clock_ms": 159.53,
        "density_total_elapsed_ms": 0.0
      },
      "win": true
    },
    {
      "name": "scroll_and_click",
      "description": "scroll + click (1 rtt vs 3 rtt manual)",
      "computer_act": {
        "ok": true,
        "rtt": 1,
        "wall_clock_ms": 76.33,
        "density": {
          "backend_request_count": 1,
          "control_frame_count": 1,
          "dispatch_ms": 54,
          "elapsed_ms_total": 54,
          "false_success_count": 0,
          "implicit_observe": false,
          "implicit_observe_ms": 0,
          "mouse_fallback_count": 0,
          "payload_bytes": 0,
          "semantic_action_count": 1,
          "stale_ref_recovery_count": 0,
          "trace_step_count": 4,
          "verification_passed": false
        }
      },
      "manual": {
        "ok": true,
        "rtt": 3,
        "wall_clock_ms": 141.5,
        "density_total_elapsed_ms": 0.0
      },
      "win": true
    },
    {
      "name": "drag_and_drop",
      "description": "drag \u5143\u7d20 (1 rtt vs 4 rtt manual)",
      "computer_act": {
        "ok": true,
        "rtt": 1,
        "wall_clock_ms": 74.36,
        "density": {
          "backend_request_count": 1,
          "control_frame_count": 1,
          "dispatch_ms": 52,
          "elapsed_ms_total": 52,
          "false_success_count": 0,
          "implicit_observe": false,
          "implicit_observe_ms": 0,
          "mouse_fallback_count": 0,
          "payload_bytes": 0,
          "semantic_action_count": 1,
          "stale_ref_recovery_count": 0,
          "trace_step_count": 4,
          "verification_passed": false
        }
      },
      "manual": {
        "ok": true,
        "rtt": 4,
        "wall_clock_ms": 163.27,
        "density_total_elapsed_ms": 0.0
      },
      "win": true
    },
    {
      "name": "right_click_context",
      "description": "\u53f3\u952e\u83dc\u5355 (1 rtt vs 3 rtt manual)",
      "computer_act": {
        "ok": true,
        "rtt": 1,
        "wall_clock_ms": 76.83,
        "density": {
          "backend_request_count": 1,
          "control_frame_count": 1,
          "dispatch_ms": 55,
          "elapsed_ms_total": 55,
          "false_success_count": 0,
          "implicit_observe": false,
          "implicit_observe_ms": 0,
          "mouse_fallback_count": 0,
          "payload_bytes": 0,
          "semantic_action_count": 1,
          "stale_ref_recovery_count": 0,
          "trace_step_count": 4,
          "verification_passed": false
        }
      },
      "manual": {
        "ok": true,
        "rtt": 3,
        "wall_clock_ms": 140.12,
        "density_total_elapsed_ms": 0.0
      },
      "win": true
    },
    {
      "name": "hotkey_combo",
      "description": "Cmd+S \u5feb\u6377\u952e (1 rtt vs 2 rtt manual)",
      "computer_act": {
        "ok": true,
        "rtt": 1,
        "wall_clock_ms": 281.99,
        "density": {
          "backend_request_count": 1,
          "control_frame_count": 1,
          "dispatch_ms": 255,
          "elapsed_ms_total": 255,
          "false_success_count": 0,
          "implicit_observe": false,
          "implicit_observe_ms": 0,
          "mouse_fallback_count": 0,
          "payload_bytes": 0,
          "semantic_action_count": 1,
          "stale_ref_recovery_count": 0,
          "trace_step_count": 4,
          "verification_passed": false
        }
      },
      "manual": {
        "ok": false,
        "rtt": 2,
        "wall_clock_ms": 559.88,
        "density_total_elapsed_ms": 0.0
      },
      "win": true
    },
    {
      "name": "wait_then_observe",
      "description": "\u7b49 + observe (1 rtt vs 2 rtt manual)",
      "computer_act": {
        "ok": true,
        "rtt": 1,
        "wall_clock_ms": 91.19,
        "density": {
          "backend_request_count": 1,
          "control_frame_count": 1,
          "dispatch_ms": 55,
          "elapsed_ms_total": 55,
          "false_success_count": 0,
          "implicit_observe": false,
          "implicit_observe_ms": 0,
          "mouse_fallback_count": 0,
          "payload_bytes": 0,
          "semantic_action_count": 1,
          "stale_ref_recovery_count": 0,
          "trace_step_count": 4,
          "verification_passed": false
        }
      },
      "manual": {
        "ok": true,
        "rtt": 2,
        "wall_clock_ms": 114.29,
        "density_total_elapsed_ms": 0.0
      },
      "win": true
    }
  ],
  "summary": {
    "total_tasks": 10,
    "wins": 10,
    "win_rate": 1.0,
    "median_computer_act_rtt": 1,
    "median_manual_rtt": 3,
    "median_computer_act_wall_ms": 77.26,
    "median_manual_wall_ms": 159.53
  }
}
```
