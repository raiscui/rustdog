## [2026-05-28 09:57:47] [Session ID: codex-native-20260528-web-find] 笔记: Phase 1 read-only `@web-find`

## 来源

### 来源1: `specs/rdog-computer-use-density-plan.md`

- 要点:
  - Phase 1 的目标是 read-only `@web-find`,不是 side-effectful `@web-act`。
  - 默认 scope 是 `active_web_area`,浏览器 chrome 必须排除。
  - 匹配字段顺序固定为 `description`、`name`、`value`。

### 来源2: `.codex/skills/rdog-control/references/cookbook-web-content.md`

- 要点:
  - 小红书页面左侧导航的 live 证据证明目标在 Chrome `AXWebArea` 子树下。
  - 错误路径是从整个 `AXWindow` 或 screenshot/OCR 开始,会增加 backend request 和 agent decision point。

### 来源3: `src/control_web.rs`

- 要点:
  - `@web-find` 通过现有 AX snapshot 获取窗口和元素结构,没有新增截图或鼠标真相源。
  - finder 选择 active browser window,再找 `AXWebArea`,最后只在该 subtree 内匹配 page-owned candidates。
  - 匹配文本在非 actionable child 上时,向上提升到同一 `AXWebArea` 内的最近 actionable ancestor。

## 综合发现

### 接口边界

- `@web-find` 返回 `rdog.web-find.v1`,包含 `window`、`web_area`、`matches`、`trace`、`match_count`、`returned_count` 和 `truncated`。
- structured blocker 使用 `not_found` / `needs_disambiguation` / `blocked`,而不是让 agent 继续猜坐标。
- `match_count` 按去重后的 actionable target 统计,`returned_count` 是受 `limit` 限制后实际返回的数量。

### 验证证据

- `cargo test --package rustdog --bin rdog -- control_web::tests control_protocol::tests::parse_should_support_web_find_command --quiet`: 5 个测试通过。
- `cargo test --package rustdog --bin rdog -- control_web::tests --quiet`: 4 个测试通过。
- `cargo test --package rustdog --bin rdog -- control_protocol::tests::parse_should_support_web_find_command --exact --quiet`: 1 个测试通过。
- `cargo test --package rustdog --test computer_use_density --quiet`: 2 个测试通过。
- `cargo test --package rustdog --test computer_use_density --no-run --quiet`: 通过。
- `git diff --check`: 通过。

## [2026-05-28 14:35:03] [Session ID: codex-native-20260528-web-act] 笔记: Phase 2 side-effectful `@web-act`

## 来源

### 来源1: `specs/rdog-computer-use-density-plan.md`

- 要点:
  - Phase 2 目标是 `@web-act` 的 action + verification,并且必须晚于 Phase 0 baseline 和 Phase 1 `@web-find`。
  - `@web-act` 是 side-effectful,必须返回 action trace 和 verification evidence。
  - 鼠标 fallback 不能成为默认路径。

### 来源2: `src/control_web/act.rs`

- 要点:
  - 请求结构复用 `WebFindRequest`,并新增 `action:"press"` 与 `verify:true/false`。
  - 主路径是当前 AX snapshot -> active browser window -> `AXWebArea` -> unique page-owned match -> `AXPress`。
  - 第一次 action 如果出现 stale-like target error,会重新 capture AX snapshot 并在同一 `AXWebArea` 语义下 re-find 一次,然后重试一次 `AXPress`。
  - verification 使用 fresh AX snapshot 和同一 match 条件重新匹配,返回 `verification.verified`、`match_count`、`same_target_id`。

### 来源3: `src/control_web/act/tests.rs`

- 要点:
  - 覆盖唯一目标执行并验证成功。
  - 覆盖多个候选时 `needs_disambiguation` 且不执行 action。
  - 覆盖匹配目标没有 `AXPress` 时 `blocked` 且不执行 action。
  - 覆盖 action 后 verification 失败的诚实返回。
  - 覆盖 stale target 首次失败后 re-find 一次并对 fresh target 执行。

## 综合发现

### 接口边界

- `@web-act` 只支持 `action:"press"` / `AXPress`。
- 默认 `verify:true`。
- 没有 mouse fallback,也不把坐标 fallback 写成成功路径。
- 响应 schema 是 `rdog.web-act.v1`,核心字段包括 `selected_match`、`action_result`、`verification`、`performed`、`verified` 和 `trace`。

### 验证证据

- `cargo test --package rustdog --bin rdog -- control_web::act::tests --quiet`: 5 个测试通过。
- `cargo test --package rustdog --bin rdog -- control_web::tests control_web::act::tests --quiet`: 9 个测试通过。
- `cargo test --package rustdog --bin rdog -- control_protocol::tests::parse_should_support_web_find_command control_protocol::tests::parse_should_support_web_act_command --quiet`: 2 个测试通过。
- `cargo test --package rustdog --test computer_use_density --quiet`: 2 个测试通过。
- `cargo test --package rustdog --test computer_use_density --no-run --quiet`: 通过。
- `git diff --check`: 通过。

## [2026-05-28 16:17:16] [Session ID: codex-native-20260528-gui-bench] 笔记: Phase 3A `@gui-bench` / fixture bench runner

## 来源

### 来源1: `specs/rdog-computer-use-density-plan.md`

- 要点:
  - Phase 3 的目标是把 fixture/test schema 推进到可执行 bench runner。
  - Phase 3A 先做只读 fixture runner,不触发真实 GUI 副作用。
  - baseline-low-level 的价值是展示当前低级链路超过 dense target,不是证明它已经达标。

### 来源2: `tests/fixtures/computer_use_density/xhs_left_nav_home_baseline.json`

- 要点:
  - 当前唯一内置 case 是 `suite:"computer-use-density"` / `case:"xhs-left-nav-home"` / `variant:"baseline-low-level"`。
  - metrics 中 `backend_request_count=8`、`agent_decision_points=7`。
  - dense target 是 `max_backend_request_count=2`、`max_agent_decision_points=1`。

### 来源3: `src/control_gui_bench.rs`

- 要点:
  - `@gui-bench` 解析 `suite`、`case`、`variant` 三个必填字段。
  - runner 只读取内置 fixture,不调用 AX、窗口、截图、键盘或鼠标后端。
  - 响应 `status:"complete"` 表示 runner 成功完成; `dense_target_passed:false` 表示该 variant 未达密度阈值。

## 综合发现

### 接口边界

- 请求示例: `@gui-bench#501:{suite:"computer-use-density",case:"xhs-left-nav-home",variant:"baseline-low-level"}`。
- 响应 schema 是 `rdog.gui-bench.v1`,核心字段包括 `metrics`、`thresholds`、`checks`、`threshold_failures`、`steps_summary` 和 `trace`。
- Phase 3A 只支持 baseline fixture。其他 suite / case / variant 返回 `InvalidData`,避免误以为已经有 live replay runner。

### 验证证据

- `cargo fmt`: 通过。
- `cargo test --package rustdog --bin rdog -- gui_bench --quiet`: 6 个测试通过。
- `cargo test --package rustdog --test computer_use_density --quiet`: 2 个测试通过。
- `cargo test --package rustdog --test computer_use_density --no-run --quiet`: 通过。
- `git diff --check`: 通过。

## [2026-05-28 17:00:22] [Session ID: codex-native-20260528-gui-bench-p3b] 笔记: Phase 3B dense variant 和 artifact

## 来源

### 来源1: `LATER_PLANS__computer_use_density.md`

- 要点:
  - 需要增加 `@web-find` / `@web-act` dense variant fixture。
  - 需要可选 `target/rdog-bench/...` JSON artifact 输出。
  - live replay 必须显式 opt-in,不能让 `@gui-bench` 默认触发真实 GUI 副作用。

### 来源2: `tests/fixtures/computer_use_density/xhs_left_nav_home_baseline.json`

- 要点:
  - 同一个 case 文件现在包含 `baseline-low-level`、`dense-web-find` 和 `dense-web-act` 三个 variant。
  - `dense-web-find` 使用一次 `@web-find` 请求,用于只读定位。
  - `dense-web-act` 使用一次 `@web-act` 请求,用于定位、AXPress、一次 stale retry 和 fresh AX verification。

### 来源3: `src/control_gui_bench.rs`

- 要点:
  - `@gui-bench` 支持 `variant:"all"`。
  - `write_artifact:true` 才会写 `target/rdog-bench/<suite>__<case>__<variant>.json`。
  - 单 variant 响应保留顶层 `metrics` / `thresholds` / `steps_summary`; `variant:"all"` 通过 `runs[]` 做对比。

## 综合发现

### 接口边界

- `@gui-bench#id:{suite:"computer-use-density",case:"xhs-left-nav-home",variant:"all"}` 只读对比三个 variant。
- `@gui-bench#id:{suite:"computer-use-density",case:"xhs-left-nav-home",variant:"all",write_artifact:true}` 会额外写 artifact。
- 顶层 `threshold_failures` 在多 variant 模式下带 variant 前缀,例如 `baseline-low-level:backend_request_count`。

### 验证证据

- `cargo fmt`: 通过。
- `cargo test --package rustdog --bin rdog -- gui_bench --quiet`: 10 个测试通过。
- `cargo test --package rustdog --test computer_use_density --quiet`: 3 个测试通过。
- `cargo test --package rustdog --test computer_use_density --no-run --quiet`: 通过。
- `git diff --check`: 通过。
- `find target/rdog-bench -maxdepth 1 -type f -print`: 无残留 artifact。

## [2026-05-29 17:42:55] [Session ID: 019e6d92-4146-7361-aae9-3e05a41b8c52] 笔记: Phase 3F live 首页点击闭环与快速路径

## 来源

### 来源1: live `rdog control mac.lab`

- 要点:
  - 当前 daemon 必须从 Terminal.app 这类有 Accessibility 权限的上下文启动。
  - `@capabilities#361` 显示 AX/键鼠/window/type_text 可用,daemon `@screenshot` 仍为 Screen Recording `permission_denied`。
  - 因此动作证据来自 `rdog control`,视觉证据来自本机 `screencapture`。

### 来源2: `@web-find` targeted WebArea refresh

- 要点:
  - `@web-find#401` 在当前二进制上返回 `status:"complete",match_count:1`。
  - trace 显示初始 `match-page-content` 为 `not_found`,随后 `refresh-web-area-subtree` 为 `ok,match_count=1`。
  - 目标 id 为 `pid:8231/window:0/path:0.0.0.0.1.0.0.0.0.0.0.1.1.0.0.0`,角色 `AXLink`,description 为“首页”。

### 来源3: live side-effect 验证

- 要点:
  - `@web-act verify:true` 多次执行后页面确实变化,但 final response 会超时,不能作为快速闭环。
  - `@web-act verify:false` 可返回 `performed:true`,但耗时约 `real 11.25s`。
  - 直接 `@ax-action` 复用已发现的 AX id,三次耗时约 `0.03s/0.04s/0.03s`,且截图 diff 都显示瀑布流变化。

## 综合发现

### 成功口径

- “首页”点击成功必须由点击前后瀑布流截图变化证明。
- 本轮最终当前二进制证据:
  - 前图: `target/rdog-live-e2e/xhs-home/before_direct_ax_action_final_full.png`
  - 后图: `target/rdog-live-e2e/xhs-home/after_direct_ax_action_final_full.png`
  - crop 区域: `2540x1380+400+420`
  - `imgdiff --threshold 0.05`: `Different pixels: 2502677`
  - ImageMagick AE: `2.99857e+06 (0.855464)`

### 更快方法

- 首次发现目标: 用 `@web-find` 获取 page-owned “首页”AX id,当前会通过 WebArea subtree refresh 找到深层链接。
- 重复点击: 直接调用 `@ax-action` 复用该 AX id,再用截图 diff 验证视觉变化。
- 若 id stale,再回退到 `@web-find` refresh 重新获取 id。
- 如果只需要快速拿 Chrome pid,`@cmd#394:"pgrep -f Chrome.app/Contents/MacOS/Google | head -n 1"` 约 `0.07s`,可结合稳定 path 拼出 AX id,但这条比 `@web-find` 更依赖 Chrome / 小红书结构稳定性。

### 未闭合风险

- `@web-act verify:true` 在页面变更后仍可能超时,即使 verification 已优先尝试 WebArea subtree refresh。
- 当前建议不要把它作为小红书首页 live E2E 的最快验收路径。
- 后续应做 bounded AX verification 或把视觉 diff verifier 作为 live opt-in 的一等结果,避免 side effect 已发生但 final response 超时。

## [2026-05-29 00:05:09] [Session ID: codex-native-20260528-protocol-tests-split] 笔记: Phase 3E protocol parser 测试拆分

## 来源

### 来源1: `src/control_protocol/tests.rs`

- 要点:
  - 拆分前 1158 行,超过项目对 Rust 文件 1000 行以内的质量建议。
  - 文件主要是 parser 测试,没有生产逻辑。
  - web/gui/selector/invalid mouse 相关测试是一组自然边界,适合先移到子模块。

### 来源2: `src/control_protocol/tests/web_gui.rs`

- 要点:
  - 新增子模块承载 `@web-find`、`@web-act`、`@gui-bench`、selector 和 invalid mouse payload parser 测试。
  - 测试函数名保持不变,避免破坏已有过滤命令。
  - 使用 `use super::*` 复用父测试模块上下文,不改变 parser 入口。

## 综合发现

### 拆分结果

- `src/control_protocol/tests.rs`: 926 行。
- `src/control_protocol/tests/web_gui.rs`: 242 行。
- 本轮只拆测试文件,没有改变 control protocol parser 行为。

### 验证证据

- `cargo fmt`: 通过。
- `cargo test --package rustdog --bin rdog -- control_protocol::tests --quiet`: 20 个测试通过。
- `cargo test --package rustdog --bin rdog -- gui_bench --quiet`: 16 个测试通过。
- `cargo test --package rustdog --test computer_use_density --quiet`: 3 个测试通过。
- `cargo test --package rustdog --bin rdog -- shell::tests::control_receiver_should_execute_gui_bench_with_real_fixture_runner shell::tests::control_receiver_should_execute_gui_bench_all_variants shell::tests::control_receiver_should_write_gui_bench_artifact_for_ci_collection --quiet`: 3 个测试通过。
- `cargo test --package rustdog --test computer_use_density --no-run --quiet`: 通过。
- `git diff --check`: 通过。
- `find target/rdog-bench -maxdepth 1 -type f -print`: 无残留 artifact。

## [2026-05-28 18:08:17] [Session ID: codex-native-20260528-gui-bench-p3d] 笔记: Phase 3D live replay opt-in

## 来源

### 来源1: `src/control_gui_bench.rs`

- 要点:
  - `GuiBenchRequest` 新增 `runner` 和 `allow_side_effects`。
  - 默认值保持 `runner:"fixture"` / `allow_side_effects:false`,因此旧 `@gui-bench` 请求仍然只跑 fixture。
  - `runner:"live"` 必须配 `allow_side_effects:true`,否则返回 `InvalidData`。
  - live replay 拒绝 `variant:"all"`,避免一次请求执行多个真实 GUI 动作。
  - live `dense-web-find` / `dense-web-act` 复用现有 `@web-find` / `@web-act` response builder。
  - live response 在 `runs[].live_replay` 里记录真实响应摘要,包括 `performed`、`verified`、`status` 和 `error_code`。

### 来源2: `src/control_gui_bench/tests.rs`

- 要点:
  - 覆盖 parser 默认 fixture runner。
  - 覆盖 live runner 需要显式字段。
  - 覆盖 live runner 未给 `allow_side_effects:true` 时拒绝。
  - 覆盖 live runner 拒绝 `variant:"all"`。
  - 用 stubbed builder 覆盖 live `dense-web-act` 成功和 verification failure,不在单测里触碰真实 GUI。

### 来源3: `specs/rdog-computer-use-density-plan.md`

- 要点:
  - 正式记录 Phase 3D live replay opt-in 契约。
  - 明确默认 fixture runner 不触碰 live GUI。
  - 明确 live replay 后续扩展也必须继续显式 opt-in。

## 综合发现

### 接口边界

- 默认请求仍然是:
  - `@gui-bench#id:{suite:"computer-use-density",case:"xhs-left-nav-home",variant:"all"}`
- live replay 请求必须显式写成:
  - `@gui-bench#id:{suite:"computer-use-density",case:"xhs-left-nav-home",variant:"dense-web-act",runner:"live",allow_side_effects:true}`
- live `dense-web-act` 只有 `performed:true` 和 `verified:true` 同时成立时才算 replay passed。
- live artifact 用 `__live.json` 后缀,避免和 fixture artifact 路径冲突。

### 验证证据

- `cargo fmt`: 通过。
- `cargo test --package rustdog --bin rdog -- gui_bench --quiet`: 16 个测试通过。
- `cargo test --package rustdog --test computer_use_density --quiet`: 3 个测试通过。
- `cargo test --package rustdog --bin rdog -- shell::tests::control_receiver_should_execute_gui_bench_with_real_fixture_runner shell::tests::control_receiver_should_execute_gui_bench_all_variants shell::tests::control_receiver_should_write_gui_bench_artifact_for_ci_collection --quiet`: 3 个测试通过。
- `cargo test --package rustdog --test computer_use_density --no-run --quiet`: 通过。
- `git diff --check`: 通过。
- `find target/rdog-bench -maxdepth 1 -type f -print`: 无残留 artifact。

## [2026-05-28 17:18:30] [Session ID: codex-native-20260528-gui-bench-p3c] 笔记: Phase 3C CI artifact collection

## 来源

### 来源1: `src/control_gui_bench.rs`

- 要点:
  - `@gui-bench` 当前 `GuiBenchResponse` 构造里只有一个 `threshold_failures` 初始化项。
  - `write_artifact:true` 通过 `prepare_artifact()` 写到 `target/rdog-bench/<suite>__<case>__<variant>.json`。
  - artifact 内容使用 pretty JSON,和响应 schema 一致,仍然来自 fixture runner。

### 来源2: `src/shell/tests.rs`

- 要点:
  - Phase 3C 新增 `control_receiver_should_write_gui_bench_artifact_for_ci_collection`。
  - 测试走真实 `run_control_receiver_with_executor` + `SystemControlActionExecutor`,不是只调用内部 helper。
  - 测试发送 `@gui-bench#503:{suite:"computer-use-density",case:"xhs-left-nav-home",variant:"all",write_artifact:true}`。
  - 测试读取 artifact JSON,校验 `schema`、`runner`、`variant`、`variant_count`、`runs[]`、`threshold_failures[]` 和 `artifact.path`,最后删除文件。

### 来源3: `specs/rdog-computer-use-density-plan.md`

- 要点:
  - Phase 3C 已记录 CI artifact collection 的已验证入口。
  - live replay 仍然是后续显式 opt-in,不能让 `@gui-bench` 默认触碰真实 GUI。

## 综合发现

### 证伪结果

- handoff 中提到的重复 `threshold_failures` 字段在当前代码中不成立。
- `cargo test --package rustdog --bin rdog -- gui_bench --quiet` 实际执行 11 个测试并通过。

### 验证证据

- `cargo fmt`: 通过。
- `cargo test --package rustdog --bin rdog -- gui_bench --quiet`: 11 个测试通过。
- `cargo test --package rustdog --test computer_use_density --quiet`: 3 个测试通过。
- `cargo test --package rustdog --bin rdog -- shell::tests::control_receiver_should_execute_gui_bench_with_real_fixture_runner shell::tests::control_receiver_should_execute_gui_bench_all_variants shell::tests::control_receiver_should_write_gui_bench_artifact_for_ci_collection --quiet`: 3 个测试通过。
- `cargo test --package rustdog --test computer_use_density --no-run --quiet`: 通过。
- `git diff --check`: 通过。
- `find target/rdog-bench -maxdepth 1 -type f -print`: 无残留 artifact。
## [2026-06-01 15:30:57] [Session ID: 019e6d92-4146-7361-aae9-3e05a41b8c52] 笔记: window-scoped `@web-find` 产品化

## 来源

### 来源1: `src/control_web.rs` / `src/control_web/parse.rs`

- `WebFindTarget` 已新增 `window_id`。
- parser 支持 `@web-find:{target:{window_id:"pid:.../window:..."},match:{text:"..."}}`。
- 带 `window_id` 时,窗口选择 trace 使用 `target-browser-window`。
- 不带 `window_id` 时,旧 `active-browser-window` 逻辑保持原样。

### 来源2: live `rdog control mac.lab`

- `@window-find#2` 找到小红书 Chrome 窗口 `pid:96405/window:0`。
- `@web-find#3` 指定该 `window_id` 后返回 `status:"complete"`、`scope:"target_window_web_area"`、`match_count:1`。
- 返回 match 是 `AXLink.description:"首页"`,actions 包含 `AXPress`。

## 综合发现

### 成功点

- 这次没有把窗口激活或点击混进 read-only 定位。
- window-scoped target 解决的是多浏览器窗口下的定位歧义,不是页面动作成功。
- `@web-act` 因复用 `WebFindRequest`,自然继承同一 target schema。

### 边界

- `@gui-probe` 仍是规格里的后续 read-only composite,本轮没有把它伪装成已实现命令。
- `target.window_ref` 还没有落地。下一步如果实现,应基于 observation ref 解析出窗口 id,并保持 read-only。
- 页面变更类任务仍必须使用 before/after 视觉或状态差异验证。

## [2026-06-01 18:03:20] [Session ID: 019e6d92-4146-7361-aae9-3e05a41b8c52] 笔记: `target.window_ref` read-only 解析

## 来源

### 来源1: `src/control_observation.rs` / `src/control_ax.rs` / `src/control_window.rs`

- observation store 通过 `resolve_observation_ref(observation_id, ref_id)` 解析短期 ref。
- AX snapshot 和 window-find 给 window ref 记录的 `ObservationRefEntry.kind` 都是 `window`。
- window ref 的 `backend_id` 是 `pid:.../window:...`。

### 来源2: `src/control_web.rs`

- `resolve_target_window_id()` 会先看 `target.window_id`。
- 若存在 `target.window_ref`,则要求 `observation_id`,并解析 observation ref。
- 解析成功后只接受 `entry.kind == "window"`。
- 非窗口 ref、过期 ref 或 stale ref 都会进入 `WINDOW_REF_INVALID`。

### 来源3: live `rdog control mac.lab`

- `@window-find#3` 返回 Chrome 窗口 `ref:"@e1"` 和 observation `obs-1780308085449-2`。
- `@web-find#4` 使用 `target:{window_ref:"@e1",observation_id:"obs-1780308085449-2"}` 后返回 `status:"complete"`、`scope:"target_window_web_area"`、`match_count:2`。

## 综合发现

### 成功点

- `window_ref` 没有变成第二套 durable selector,它仍然是短期 observation ref。
- `window_ref` 不执行 activation/focus/click,只是把窗口身份从 ref 解析到 backend id。
- `@web-act` 的 blocker 分支必须和 `@web-find` 同步,否则新增 resolution variant 会造成非穷尽 match 编译错误。

### 后续方向

- 真正的 `@gui-probe` composite 可以复用 `browser active` / `window_id` / `window_ref` 三种 target schema。
- 如果需要跨 observation 或 daemon restart 恢复,应该走 selector/refind,不是扩大 `window_ref` 生命周期。
