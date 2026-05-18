## [2026-05-14 12:01:41] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 笔记: Ralph fresh verification 和 architect/deslop review

## 来源

### 代码与文档
- `src/control_mouse.rs`: 鼠标 parser/plan/backend/report 流程已落地。
- `src/control_core.rs`: structured success response 已避免把 mouse JSON 当 stdout 双重转义。
- `README.md`、`specs/control-line-protocol.md`、`specs/code-agent-rdog-control-usage.md`: 已同步 mouse commands、os-logical 坐标和权限边界。
- `/Users/cuiluming/.codex/skills/rdog-control`: 已同步 skill 主入口与 reference。

## 综合发现

### Architect Review
- 坐标真相源保持单一: `@click` / `@drag` / positioned `@wheel` 仍使用 screenshot manifest 的 `coordinate_space:"os-logical"`。
- raw press/release 语义保留: `@mouse-button mode:"press"` 不自动 release,文档和 skill 已明确恢复命令。
- 结构化成功响应仍走现有 response renderer,没有引入第二套 control-frame 模型。这是当前阶段合适的窄接入。
- 非阻断维护风险: `src/control_mouse.rs` 现在 1409 行,超过项目静态语言文件 1000 行偏好。建议后续拆为 parser / plan / execution / tests 子模块。

### Deslop Review
- 搜索 fallback/workaround/TODO/silent/clamp 等信号后,未发现 masking fallback slop。
- 发现的 fallback 字样均是既有 legacy / entry-point / permission 边界描述,属于明确兼容或安全边界。
- 本轮已修复一个真实边界 bug: drag 插值改成先升 `i64` 再计算 delta,避免极端 `i32` 坐标下 debug overflow。

### Fresh Verification
- `python3 /Users/cuiluming/.codex/skills/.system/skill-creator/scripts/quick_validate.py /Users/cuiluming/.codex/skills/rdog-control`: Skill is valid。
- `cargo fmt -- --check`: 通过。
- `git diff --check`: 通过。
- focused tests: 8 passed。
- `cargo test --package rustdog --bin rdog`: 153 passed。
- `cargo test --tests --no-run`: 通过。

## [2026-05-14 12:06:47] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 笔记: Ralph completion audit prompt-to-artifact checklist

## 审计范围

### 原始执行链
- 用户先要求按 `.omx/plans/rdog-mouse-control-implementation-plan.md` 的 Option A 继续 `$ralph`。
- 用户随后要求 review 当前 diff 后做 local commit。
- Stop hook 要求 fresh verification evidence。
- Stop hook 本轮指出缺少 completion audit,要求补 prompt-to-artifact checklist 和 verification evidence。

### Goal mode
- `get_goal` 返回 `goal: null`。因此没有需要 `update_goal({status:"complete"})` 的 thread-level goal。
- 完成契约以 Ralph plan、用户后续 commit 要求、hook 要求为准。

## Prompt-to-artifact checklist

| 要求 / 验收项 | 产物 | 证据 | 状态 |
| --- | --- | --- | --- |
| 实现 `@mouse-move` 绝对 `os-logical` 和相对移动 | `src/control_mouse.rs`, `src/control_protocol.rs` | `MouseMoveRequest` 和 `MouseCoordinateSpace` 在 `src/control_mouse.rs:16-82`; dispatch 在 `src/control_protocol.rs:213-218`; parser tests 在 `src/control_protocol.rs:1401-1429` | 通过 |
| 实现 `@mouse-button` press/release/click,raw press 不自动 release | `src/control_mouse.rs` | `MouseButtonMode` 在 `src/control_mouse.rs:52-67`; plan builder press/release/click 在 `src/control_mouse.rs:607-637`; 注释说明 raw press 不自动 release 在 `src/control_mouse.rs:771-774`; parser test 在 `src/control_protocol.rs:1430-1439` | 通过 |
| 实现 `@click` move -> press -> hold -> release | `src/control_mouse.rs` | `ClickRequest` 在 `src/control_mouse.rs:91-100`; plan builder 在 `src/control_mouse.rs:639-679`; focused test `click_plan_should_move_press_hold_release` 通过 | 通过 |
| 实现 `@drag` move(from) -> press -> sampled move -> release,失败时尽力 release | `src/control_mouse.rs` | `DragRequest` 在 `src/control_mouse.rs:102-110`; plan builder 在 `src/control_mouse.rs:681-729`; recovery executor 在 `src/control_mouse.rs:775` 起; focused tests `drag_failure_after_press_should_attempt_release` / `drag_failure_should_report_release_failure` 通过 | 通过 |
| 实现 `@wheel` 垂直/水平滚轮和可选移动 | `src/control_mouse.rs` | `WheelRequest` 在 `src/control_mouse.rs:112-119`; plan builder 在 `src/control_mouse.rs:731-769`; focused test `wheel_plan_should_use_vertical_then_horizontal_order` 通过 | 通过 |
| 坐标真相源唯一: `@click` / `@drag` / positioned `@wheel` 复用 screenshot manifest 的 `os-logical` | code + docs + skill | 代码拒绝非 `os-logical`: `src/control_mouse.rs:639-684`, `src/control_mouse.rs:1005-1014`; README 对应 `README.md:247-254`; skill 对应 `/Users/cuiluming/.codex/skills/rdog-control/SKILL.md:37-42` | 通过 |
| 成功响应必须是结构化 mouse value,不能只返回 `@response 0` | `src/control_mouse.rs`, `src/control_actions.rs`, `src/control_core.rs` | `MouseExecutionReport` 在 `src/control_mouse.rs:149-180`; action executor 返回 `response_value_json` 在 `src/control_actions.rs:228-237`; core renderer 在 `src/control_core.rs:68-78` 和 `src/control_core.rs:172-174`; focused test `explicit_request_should_render_structured_success_without_double_escaping` 通过 | 通过 |
| PermissionDenied -> code 77,Unsupported -> code 78 语义保持 | `src/control_actions.rs`, `src/control_mouse.rs`, `src/control_core.rs` | macOS input permission 文案覆盖 mouse commands 在 `src/control_actions.rs:447-453`; unsupported negative coordinate guard 在 `src/control_mouse.rs:1131-1138`; core error mapping既有测试仍在全量 bin 通过 | 通过 |
| 至少补一条 ignored Zenoh safe mouse smoke | `tests/zenoh_router_client.rs` | ignored test `control_should_execute_safe_mouse_move_in_zenoh_profile` 在 `tests/zenoh_router_client.rs:719-771`,发送 `@mouse-move#10:{dx:0,dy:0,coordinate_space:"relative"}` | 通过 |
| README / formal protocol / code-agent guide 同步 | `README.md`, `specs/control-line-protocol.md`, `specs/code-agent-rdog-control-usage.md` | README command table 与安全边界在 `README.md:219-255`, `README.md:459-504`; protocol mouse section 在 `specs/control-line-protocol.md:262-288`; agent guide rows 在 `specs/code-agent-rdog-control-usage.md:109-173` | 通过 |
| 全局 `rdog-control` skill 同步,确保新 Codex 会话能触发正确用法 | `/Users/cuiluming/.codex/skills/rdog-control` | main skill mentions mouse commands and safe smoke at `SKILL.md:3`, `SKILL.md:37-42`; protocol/control-workflow/zenoh-hardware references include mouse examples and manifest contract | 通过 |
| review 当前 diff 后做 local commit,不 push | local repository history | local commit `f0b0dfc Enable mouse control over the existing desktop coordinate contract`; `git status` shows branch ahead 3 and no push was run | 通过 |
| Architect/deslop review | `notes__mouse_ralph.md`, `LATER_PLANS__mouse_ralph.md` | Architect/deslop findings recorded at earlier `notes__mouse_ralph.md` entry; non-blocking large-file risk recorded in `LATER_PLANS__mouse_ralph.md` | 通过 |
| Fresh verification evidence | 命令输出 | 本审计轮重新运行 focused tests、bin tests、integration compile、fmt check、diff check、skill validation,全部通过 | 通过 |

## Fresh verification evidence

- `cargo test --package rustdog --bin rdog -- control_protocol::tests::parse_should_support_mouse_requests control_protocol::tests::parse_should_reject_invalid_mouse_payloads control_mouse::tests::click_plan_should_move_press_hold_release control_mouse::tests::drag_interpolation_should_not_overflow_i32_delta control_mouse::tests::drag_failure_after_press_should_attempt_release control_mouse::tests::drag_failure_should_report_release_failure control_mouse::tests::wheel_plan_should_use_vertical_then_horizontal_order control_core::tests::explicit_request_should_render_structured_success_without_double_escaping`
  - 结果: 8 passed, 0 failed。
- `cargo test --package rustdog --bin rdog`
  - 结果: 153 passed, 0 failed。
- `cargo test --tests --no-run`
  - 结果: integration test executables all built successfully。
- `cargo fmt -- --check`
  - 结果: 通过。
- `git diff --check`
  - 结果: 通过。
- `python3 /Users/cuiluming/.codex/skills/.system/skill-creator/scripts/quick_validate.py /Users/cuiluming/.codex/skills/rdog-control`
  - 结果: `Skill is valid!`。

## 审计结论

- 没有缺失的原始需求项。
- 没有 pending / in_progress 的当前任务项。
- 当前剩余未跟踪文件是支线记录文件,不是未完成业务实现。
- 真实 GUI / Zenoh mouse smoke 是 ignored test,因为需要 daemon host 的 GUI input permission。已有 ignored smoke 用于后续有权限环境执行。
- 本轮 completion audit 可以通过。
