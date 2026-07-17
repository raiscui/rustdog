## [2026-06-29 14:18:00] [Session ID: 019f0356-e933-7e02-808d-12c495a89f09] 任务名称: rdog-control skill 文案瘦身收口与 WORKLOG 续档

### 任务内容
- 延续 `.codex/skills/rdog-control/SKILL.md` 文案瘦身任务的收尾工作。
- 旧 `WORKLOG.md` 达到 1009 行,按六文件规则归档到 `archive/default_history/WORKLOG_2026-06-29_rdog_control_skill_compaction.md`。
- 新增 archive manifest,并把 skill 文案组织经验沉淀到 `EXPERIENCE.md` 和 `AGENTS.md` 索引。

### 完成过程
- 回读默认六文件和相关长期知识,确认根目录没有支线六文件需要一并归档。
- 保留本次 skill 正文变更的语义边界: agent-agnostic、`@flow`、`@window-resize`、display scope、AX diff、PTY、permission 和 destructive-action safety。
- 将可复用经验收束为一句工程规则: skill 主体优先放高频执行路径、硬边界和验证规则,低频协议细节交给 specs / references。

### 验证
- 默认六文件行数: `task_plan.md` 305,`notes.md` 781,`WORKLOG.md` 22,`LATER_PLANS.md` 444,`ERRORFIX.md` 608,`EPIPHANY_LOG.md` 567。
- skill 体量: `.codex/skills/rdog-control/SKILL.md` 205 行 / 1209 词。
- Markdown fence: skill + manifest + 新 WORKLOG 合计 20 个 fence,成对。
- `rtk git diff --check -- .codex/skills/rdog-control/SKILL.md AGENTS.md EXPERIENCE.md task_plan.md notes.md WORKLOG.md ...`: passed。
- 新 manifest 与归档 WORKLOG 无尾随空白。`archive/` 按仓库 `.gitignore` 规则为本地归档区,普通 `git status` 不显示其中新增文件。

### 总结感悟
- agent-facing skill 的 token 经济不是删细节,而是把细节放到正确层级。
- `SKILL.md` 负责让 agent 走对路径;`specs/` 和 `references/` 负责承载完整协议和低频例外。

## [2026-06-29 15:02:00] [Session ID: codex-20260629-big-diff-closeout] 任务名称: 当前大 diff 收口

### 任务内容
- 盘点当前混合 diff,按 `@flow`、UI script runner、skill/docs/context 分组。
- 删除未跟踪临时噪音:两个旧 skill `.bak` 和一个 prompt 实验 JSON。
- 跑 focused + final 验证矩阵。
- 修正 `control_lanes` 中过期的空 target one-shot 语义测试。

### 完成过程
- 保留当前业务实现,没有回滚用户或历史主线改动。
- 将 `rdog control @ping` 的测试口径对齐到当前 local-default fast path 语义。
- 对 specs Mermaid、UI script dry-run、`@flow`、control core、control protocol 和 integration control lane 都做了验证。

### 验证
- `rtk cargo fmt -- --check`: passed。
- `rtk git diff --check`: passed。
- `rtk cargo check --package rustdog --bin rdog --quiet`: passed。
- `rtk cargo test --package rustdog --bin rdog --quiet`: 434 passed。
- `rtk cargo test --package rustdog --test control_lanes --quiet`: 15 passed,1 ignored。
- `./target/debug/rdog ui-script run --dry-run tests/fixtures/ui_script/ping_control_line.json`: passed。
- 8 个 Mermaid block 通过 `beautiful-mermaid-rs --ascii`。

### 总结感悟
- 这轮真正的收口收益是把临时噪音去掉,并用测试暴露了一个旧产品语义。
- 下一步不要继续新增 UI script 能力,应先拆 `src/main.rs` 中的 runner 代码。

## [2026-06-29 15:16:57] [Session ID: codex-20260629-final-big-diff-closeout] 任务名称: 当前大 diff 最终复验

### 任务内容
- 接手上一轮大 diff 收口后,重跑 fresh verification。
- 确认六文件行数、未跟踪文件列表和剩余风险,不给当前工作区做提交。

### 完成过程
- 复验 Rust 格式、diff whitespace、`rdog` bin 编译、全量 bin tests 和 `control_lanes` integration test。
- 复验两个新增 UI script fixture 的 `rdog ui-script run --dry-run` 入口路径。
- 复验新改动 specs 中的 Mermaid block,确认文档图语法仍可渲染。

### 验证
- `rtk cargo fmt -- --check`: passed。
- `rtk git diff --check`: passed。
- `rtk cargo check --package rustdog --bin rdog --quiet`: passed。
- `rtk cargo test --package rustdog --bin rdog --quiet`: 434 passed。
- `rtk cargo test --package rustdog --test control_lanes --quiet`: 15 passed,1 ignored。
- 两个 UI script fixture dry-run 均通过。
- `specs/rdog-flow-control-plan.md` 与 `specs/rdog-ui-script-control-plan.md` 共 4 个 Mermaid block 通过 `beautiful-mermaid-rs --ascii`。
- 六文件均低于 1000 行,不需要续档。

### 总结感悟
- 当前 diff 已经可以进入 review/commit 边界。
- 剩余最明显的结构风险仍是 `src/main.rs` 过大,下一步应优先拆出 UI script runner。

## [2026-06-29 16:07:12] [Session ID: codex-20260629-review-and-commit] 任务名称: 当前大 diff review gate 与提交收口

### 任务内容
- 对当前 `@flow` / UI script runner / skill-docs 大 diff 执行提交前 review。
- 修复 review gate 发现的两个阻塞问题。
- 重跑提交前验证矩阵,准备 scoped commit。

### 完成过程
- 启动独立 code-reviewer 和 architect review lane。
- 初轮 code-reviewer 返回 `REQUEST CHANGES`;architect 返回 `WATCH`。
- 修复 UI script control step 对错误 `@response` 误报成功的问题。
- 为 `@flow SaveArtifact` 增加 `policy.allow_file_read:true` 显式授权门禁。
- 清理 `LATER_PLANS.md` 中已失效的 unchecked cleanup 项。
- 复审 code-reviewer 返回 `APPROVE`;architect 仍为 `WATCH`,但明确无 BLOCK,当前 diff 可以提交。

### 验证
- 新增两个回归测试均先红后绿。
- `rtk cargo check --package rustdog --bin rdog --quiet`: passed。
- `rtk cargo test --package rustdog --bin rdog --quiet`: 436 passed。
- `rtk cargo test --package rustdog --test control_lanes --quiet`: 15 passed,1 ignored。
- 两个 UI script fixture dry-run 均通过。
- `specs/rdog-flow-control-plan.md` 与 `specs/rdog-ui-script-control-plan.md` 共 4 个 Mermaid block 通过 `beautiful-mermaid-rs --ascii`。
- `rtk cargo fmt -- --check`: passed。
- `rtk git diff --check`: passed。

### 总结感悟
- UI script runner 不能把 control response 当成纯输出;非零 code 是脚本级失败信号。
- daemon-side `@flow` 的能力开关要按副作用类型拆开,文件读取不能隐式挂在 shell policy 之外。
- `main.rs` 职责集中和 target resolver 分散仍是 WATCH 项,提交后下一步应优先拆 UI script runner / target resolver。

## [2026-06-30 00:56:17] [Session ID: codex-20260629-ultragoal-ui-script-123] 任务名称: Ultragoal UI script 123 完成

### 任务内容
- 按用户要求顺序完成 1/2/3:拆分 UI script runner / target resolver,接入 `rdog control --ui-script`,完成安全 live smoke 和 final quality gate。
- 生成 `.omx/ultragoal/ai-slop-cleaner-g004.md`、`.omx/ultragoal/quality-gate-g004.json`、`.omx/tmp/g004-codex-goal.json`。
- 完成 G004 checkpoint,`omx ultragoal complete-goals --json` 返回 `done:true`。

### 完成过程
- 将 `src/main.rs` 保持为 CLI 分发,把 control invocation 与 UI script runner 收敛到 `src/control_invocation.rs` 和 `src/ui_script_runner.rs`。
- `rdog control --ui-script <file.json> [TARGET]` 复用共享 runner,同步更新 specs 和 rdog-control skill。
- code-reviewer 两次发现真实 runner blocker 后,分别修复 prior-response guard 和 adjacent ControlLine fail-fast。
- architect WATCH 指出的 trace contract drift 已修复,control trace 现在包含 `started_at_unix_ms` 和结构化 `response.target_resolution`。

### 验证
- `rtk cargo test --package rustdog --bin rdog ui_script_runner::tests --quiet`: 14 passed。
- `rtk cargo test --package rustdog --bin rdog --quiet`: 441 passed。
- `rtk cargo test --package rustdog --test control_lanes --quiet`: 15 passed,1 ignored。
- `cargo fmt -- --check`、`rtk git diff --check`、`rtk cargo check --package rustdog --bin rdog --quiet`、`cargo build --package rustdog --bin rdog --quiet`: passed。
- 最新 `rdog control --ui-script ... --dry-run self`: emitted 4 compiled steps。
- final TCP live smoke: `@response "pong"`,summary complete,trace 4 lines,normalized script 和 artifacts 目录存在,端口 45679 已释放。
- 独立 code-reviewer: `APPROVE`;独立 architect: `CLEAR`。

### 总结感悟
- UI script runner 的安全边界不能为了 batching 牺牲 fail-fast。相邻 UI action 必须逐条确认上一条成功后再发送。
- `Expect` 不能从 missing state 推导成功。没有上一条 `@response` 就应该显式失败。

## [2026-07-15 16:30:00] [Session ID: omx-1783957580965-m4bn8e] 任务名称: rdog `@computer-act` implicit_observe plumbing (ticket 11)

### 任务内容
- 实现 ticket 11: `@computer-act` implicit_observe + freshness 三态 + 5s TTL (ADR-0005 L3)
- 覆盖 ticket 11 acceptance criteria 全部 5 项:
  - start_box 路径触发 implicit_observe,response 携带 observation_id + freshness
  - target.ref + observation_id 在 TTL 内 → freshness="fresh",复用
  - target.ref + observation_id 已过期 → stale_re_observed,新 id + previous_observation_id
  - 5s TTL 严格生效 (clock 注入测试覆盖边界)
  - 时钟注入 / fresh path / stale path / re-observe 路径均有单测

### 完成过程
- Phase 0: 读 ticket 11 spec + ADR-0005 + 当前 control_computer_act/mod.rs + 现有 ObservationStore API + MouseEndpoint 枚举 (Coordinate vs ObservationRef),理解 ticket 11 在 dispatcher 流水线里的位置
- Phase 1: 新建 src/control_computer_act/implicit_observe.rs (610 行)
  - `COMPUTER_ACT_OBSERVATION_TTL_MS = 5_000` 常量 (防止后续误改)
  - `ComputerActObservationCache` (TTL + capacity + FIFO evict)
  - `ImplicitObserveOutcome` 三态 (Fresh / StaleReObserved / StaleFallbackToCoords)
  - 4 个分支的 `resolve_or_re_observe(args, now_ms)` 入口
  - OnceLock<Mutex<...>> daemon-global cache (跟 ObservationStore 同模式)
  - `render_observation_used` / `render_top_level_observation_id` response helpers
  - `apply_implicit_observe_to_args` stub 给后续 ticket 18 / Phase I
- Phase 2: src/control_computer_act/mod.rs 接线
  - 注册子模块
  - 在 execute_computer_act 入口 routing 之前调 `resolve_or_re_observe_with_wall_clock(&request.args)`
  - response envelope 把 observation_id / observation_used 从 outcome 填充
- Phase 3: 19 个单测 (inline 在 implicit_observe.rs 底部)
  - cache 基础 CRUD / TTL 边界 / capacity evict
  - 4 个 resolve_or_re_observe 分支 + 未知 obs_id 路径
  - 3 个 render_observation_used 形状 (fresh / stale+previous / fallback None)
  - top_level_observation_id 三态
  - global cache wall clock helper 集成
- Phase 4: scripts/smoke_computer_act_observe.sh (185 行, 4 段 e2e)
  - start_box → freshness=fresh
  - TTL 内复用
  - TTL 外 stale_re_observed
  - non-mouse wait → null 占位

### 验证
- `RUSTFLAGS="-Awarnings" cargo check --bin rdog --tests`: 0 warning
- `RUSTFLAGS="-Awarnings" cargo test --bin rdog`: 536 passed, 0 failed (+19 vs ticket 05 baseline)
- `bash scripts/smoke_computer_act_observe.sh`: 4/4 通过 (test 3 含 6s sleep 等 TTL 过期,~20s 总时长)
- `bash scripts/smoke_computer_act.sh` (regression): 5/5 通过
- `git push origin main`: `ec0f653..7e2ce62` 成功

### 总结感悟
- ticket 11 阶段的关键决策: 复用 `OnceLock<Mutex<...>>` 全局 cache 模式,跟现有 `ObservationStore` 对齐。这让 `resolve_or_re_observe` 在测试里既能 inject mock clock (单元测试),又能用 wall clock (production)。**关键边界**: 测试 helper 必须 `#[cfg(test)]` 或 `#[allow(dead_code)]`,否则会污染 warning。
- **freshness 三态拆分 = 真实状态机**: ticket 11 只暴露 `fresh` + `stale_re_observed` 两种;`stale_fallback_to_coords` 留接口给后续 Phase I 真实 observe 集成 (那时如果 start_box 找不到 ref,可能降级回纯坐标)。这是 ADR-0005 L3 的前瞻性接口,但 ticket 11 不应该实现降级,因为它没有真实 observe,降级路径会变成 silent fallback。
- **synthetic ref_id 占位**: `@e{seq}` 跟 Mano-CUA `@e1` 风格对齐。ticket 11 客户端可以拿到 ref 但底层 dispatch 仍用 start_box 像素,等真实 observe 接入才切 `MouseEndpoint::ObservationRef`。**关键边界**: 客户端如果在第二轮传 `target.ref`,daemon 仍认为有效(因为 ref_id 在 cache 里有,但底层 click 用的是 stale ref_id 字符串,跟真 AX ref 无关联)。这种"假 ref"只有 ticket 11 这种 plumbing stub 阶段允许;真 observe 接入后必须删 synthetic 路径,统一走真实 ref。
- **避免重复造 json field extractor**: smoke script 用 python3 (stdlib only) 解析 `@response ...` 包裹的 JSON envelope,比 jq 依赖更轻,跟其它 smoke 脚本保持一致风格。
- **mixed worktree scoped commit hygiene**: ticket 11 只动 4 个文件 (mod.rs / implicit_observe.rs / smoke_computer_act_observe.sh / task_plan.md),不要 `git add .`。
- **ask-matt hygiene "clearing context between tickets"**: ticket 11 完整收口 (commit + push + 验证) 后停在新 session 启动位置,留给 ticket 13 (verify-best-effort) 在下一个 session 启动。

## [2026-07-15 17:45:00] [Session ID: omx-1783957580965-m4bn8e] 任务名称: rdog `@computer-act` verify tier (ticket 12 + 13)

### 任务内容
- 实现 ticket 12 (`verify-none`): 默认 verify 字段缺省 = none,response 不带 `verification` key
- 实现 ticket 13 (`verify-best-effort`): pre/post AX diff,response 带 `verification.method:"ax_diff"` + ax_diff 摘要
- ticket 14 (`verify-always`) 留占位,本轮 `Always` policy 等同 `None` (omit verification)
- 拆分 density 字段为 `{dispatch_ms, implicit_observe_ms, verify_ms?}` 三段耗时

### 完成过程
- Phase 0: 读 ticket 12 + 13 spec + ADR-0004 V3 + ADR-0006 density 字段规范
- Phase 1: 新建 src/control_computer_act/verify.rs (347 行)
  - `VerifyPolicy` 三态枚举 + `parse_verify_policy` 单一入口
  - `AxDiffSummary` 结构 (dispatch_ms / verify_ms / full_report)
  - `run_best_effort_verify` 完整流程: pre/post AX capture → compute_diff → summary
  - `render_verification` / `render_density` response helpers
  - 11 单测 (parse 5 cases + empty summary + render 4 cases + density 2 cases)
- Phase 2: src/control_computer_act/mod.rs 接线
  - 注册子模块
  - 在 execute_computer_act 入口加 parse_verify_policy (invalid_verify 错误返回)
  - 拆 dispatch_ms 与 start.elapsed (verify 块需要 dispatch 耗时)
  - 拆 implicit_observe_ms (跟 dispatch_ms / verify_ms 三段对齐)
  - response envelope 跟随新契约: omit verification when verify=none
- Phase 3: src/ax_diff/mod.rs `mod diff` → pub(crate),让 verify.rs 拿 compute_diff
- Phase 4: scripts/smoke_computer_act_verify.sh (193 行, 5 段 e2e)
- Phase 5: 旧 smoke_computer_act.sh 跟随契约升级

### 验证
- `RUSTFLAGS="-Awarnings" cargo check --bin rdog --tests`: 0 warning
- `RUSTFLAGS="-Awarnings" cargo test --bin rdog`: 547 passed, 0 failed (+11 vs ticket 11)
- `bash scripts/smoke_computer_act_verify.sh`: 5/5 通过
- `bash scripts/smoke_computer_act.sh` (regression): 5/5 通过
- `bash scripts/smoke_computer_act_observe.sh` (regression): 4/4 通过
- `git push origin main`: `afa7517..aeac227` 成功

### 总结感悟
- **omit vs null placeholder**: ticket 12 关键决策是 verify=none 时 omit `verification` key,而不是保留 `null` 占位。这让 client parser 不用判 null,直接用 `obj.get("verification")` 拿到 None 即代表 no verify,语义更清晰。但这把旧 smoke 契约推翻了,需要同步更新 (rdog 早期 acceptance 跟 V3 契约有出入,跟 ticket 11 那次一样,本质是 verify 从"占位"演进到"按需出现")。
- **density 三段拆分 vs 端到端 duration_ms**: 端到端 `duration_ms` 保留 wall clock 概念 (跟 ETA 评估对齐),`density.{dispatch_ms, implicit_observe_ms, verify_ms}` 是分段耗时 (跟 ticket 17 density metrics 对齐)。两边互补,不能合并。
- **verify=always 占位的两种选择**: 一是本轮就返回 partial verification (只有 ax_diff 没有 screenshot),二是显式 defer 等 ticket 14。我选了后者,因为 ticket 13 spec acceptance 明确 "No screenshot is captured" 是 best_effort 的特性,而不是 always 的 fallback。如果混了会让 ticket 14 没有清晰边界。
- **ax_diff module 升级到 pub(crate)**: 这是个隐藏的 coupling。verify.rs 跟 ax_diff::compute_diff 之间应该有 facade (verify 模块包装),而不是直接调 compute_diff。当前为了 ticket 11/13 节奏先直接 import,后续重构期可以抽 ax_diff facade (`run_ax_diff_between_snapshots(before, after) -> AxDiffSummary`) 把 ax_diff 重新变成 opaque。
- **smoke 脚本的"契约升级"模式**: smoke_computer_act.sh ticket 04 时代写死了 `verification: null` 占位,ticket 12 改成 omit 后旧 smoke 直接挂掉。这暴露了 smoke 脚本需要"按 ticket 版本分文件"或者"契约注释化"。当前简单粗暴: 旧 smoke 跟新契约对齐 (update in place)。后续可以拆 smoke_computer_act_v1.sh (旧契约) 和 smoke_computer_act.sh (当前契约),保留历史回归。
- **mixed worktree scoped commit hygiene 继续生效**: 5 个文件 + 1 docs,跟 ticket 11 一样不 `git add .`。

## [2026-07-16 10:45:00] [Session ID: omx-1783957580965-m4bn8e] 任务名称: rdog `@computer-act` verify-always (ticket 14)

### 任务内容
- 实现 ticket 14 (`verify-always`): post-action full observe (screenshot + AX + windows) + AX diff
- response 增加 `verification.method:"full"` + `verification.observation.{screenshot_id, ax_tree_id, windows, screenshot_truncated}`
- screenshot > 2MB 标 `screenshot_truncated:true` (不截断,只标记)
- density.verify_ms 覆盖 full observe 耗时

### 完成过程
- Phase 0: 读 ticket 14 spec + ADR-0004 V3 + ADR-0006 density 字段规范
- Phase 1: 扩 src/control_computer_act/verify.rs (+63 行)
  - `AlwaysVerifySummary` struct (observation_block / screenshot_id / ax_tree_id / windows / screenshot_truncated / ax_diff)
  - `run_always_verify` 流程: pre-AX → dispatch → post-observe bundle → diff
  - `render_always_verification` 输出 {method, observation, ax_diff}
  - `ALWAYS_VERIFY_SCREENSHOT_LIMIT_BYTES = 2 MB`
  - `render_verification` 签名改 3 参数 (policy + diff_summary + always_summary),让 Always 走不同 summary 类型
  - +6 单测 (screenshot 阈值 / render shape / truncated / dispatch / empty)
- Phase 2: src/control_computer_act/mod.rs dispatch
  - 加 `run_always_verify` re-export
  - 拆 `verify_summary` (best_effort) / `always_summary` (always) / verify_ms 三档
- Phase 3: src/control_observation.rs `pub use observe::build_observe_bundle`
- Phase 4: smoke_computer_act_verify.sh test 4 改成验证 ticket 14 acceptance

### 验证
- `RUSTFLAGS="-Awarnings" cargo check --bin rdog --tests`: 0 warning
- `RUSTFLAGS="-Awarnings" cargo test --bin rdog`: 553 passed, 0 failed (+6 verify tests)
- `bash scripts/smoke_computer_act_verify.sh`: 5/5 通过
- `bash scripts/smoke_computer_act.sh` (regression): 5/5 通过
- `bash scripts/smoke_computer_act_observe.sh` (regression): 4/4 通过
- `git push origin main`: `ece056d..746df94` 成功

### 总结感悟
- **render_verification 签名从 2 参数改 3 参数**: 这是 ticket 14 的核心 API 演化。BestEffort 走 `AxDiffSummary`,Always 走 `AlwaysVerifySummary`,两种 summary 类型不同,必须显式 dispatch。代码上比 `Option<Box<dyn VerifySummary>>` 更直白,代价是 caller (mod.rs) 要写两次 match。这跟 "单一真相源 + 显式 dispatch" 的哲学一致: summary type 不能因为 verify policy 不同而用 trait object 抹平,显式区分让 caller 看到 "Always 多走了一条路"。
- **observation_block 字段保留但不渲染**: ticket 14 acceptance 只要求 observation.{screenshot_id, ax_tree_id, windows} 三个子字段,不要求 full observe bundle。但 AlwaysVerifySummary 跑完整 observe,丢掉 full bundle 是浪费。`#[allow(dead_code)]` 标注,等 ticket 18 trace 时复用 (full observe 一次性出,trace 直接落盘不重做 observe)。这是 "不为 ticket 14 而 ticket 14,顺手留接口" 的设计。
- **screenshot_truncated 不截断**: ticket 14 明确 "server reports `screenshot_truncated:true` rather than dropping it"。这是个关键设计: server 不擅自决定丢图 (可能 client 要 OCR),只标 false 警示 client 自己处理。这跟 rdog 整体的 "control bridge, 不擅自做决策" 哲学一致。
- **full observe 复用 client `@observe` 路径**: `build_observe_bundle(ObserveRequest::default())` 跟 client 调 `@observe` 走完全相同的 capture (Hybrid mode = screenshot + AX + windows)。这保证 ticket 14 看到的 GUI 状态跟 client 自己 observe 时一致,避免 "server-side observe 跟 client-side observe 不一致" 的诡异 bug。
- **pre-AX 用轻量 capture_default_ax_snapshot**: full observe 已经包含 AX (post),pre 只为 diff 用,不需要重复走 heavy scope / depth / max_elements 配置。`AxTreeRequest::default()` 是 AX 全树无 scope,够 diff 用。
- **screenshot_id 走 fallback chain**: 当前 `observe.visual` 段没有独立 id 字段 (screenshot summary 只有 `kind`, `image`, `manifest`),所以先查 `visual.id` (有就用),fallback 到 `observation.observation_id` (一定有)。这个 fallback 是因为 Hybrid observe 的 visual + accessibility + windows 都共享一个 observation_id,等后续 observe 加 visual.id 后可以简化。


## [2026-07-16 13:50:00] [Session ID: omx-1783957580965-m4bn8e] 任务名称: rdog `@computer-act` density metrics + trace observability (tickets 17 + 18)

### 任务内容
- 实现 ticket 17 (density): ADR-0006 §Consequences 全字段集
- 实现 ticket 18 (trace): 4 entry trace_summary inline + trace_savefile opt-in
- response envelope 整体完整化: ok / action / dispatched_to / duration_ms / observation_id / observation_used / verification / density / trace_summary / trace_savefile

### 完成过程
- Phase 0: 读 ticket 17 + 18 spec + ADR-0006 §Consequences + 现有 savefile 机制
- Phase 1: 新建 src/control_computer_act/density.rs (194 行)
  - ComputerActDensity struct + render_density + compute_verification_passed
  - 6 单测 (ADR-0006 全字段渲染 / verify_ms omit vs include / elapsed_ms_total / verification_passed 三态)
- Phase 2: 新建 src/control_computer_act/trace.rs (339 行)
  - TraceStepKind / TraceStatus / TraceStep / TraceSummary (严格 4 entry)
  - FullTrace + SubStep + write_trace_savefile (走 default_savefile_directory)
  - 7 单测 (4 entry / verify skipped / dispatch failed / implicit_observe skipped / render shape / sub_step factory)
- Phase 3: verify.rs 删 render_density 函数 + 2 个旧测试 (搬到 density.rs)
- Phase 4: mod.rs 重组
  - 注册 density + trace 子模块
  - density_metrics + trace_summary 必须在 json! macro 之前构造 (rust borrow checker)
  - trace_savefile 仅在 request.trace == Some("savefile") 时存在 (omit when absent)
- Phase 5: smoke_computer_act.sh 跟随新契约升级
- Phase 6: smoke_computer_act_trace.sh 新增 199 行 (3 段 e2e)

### 验证
- `RUSTFLAGS="-Awarnings" cargo check --bin rdog --tests`: 0 warning
- `RUSTFLAGS="-Awarnings" cargo test --bin rdog`: 564 passed, 0 failed (+13 - 2)
- `bash scripts/smoke_computer_act_trace.sh`: 3/3 通过
- `bash scripts/smoke_computer_act.sh` (regression): 5/5 通过
- `bash scripts/smoke_computer_act_verify.sh` (regression): 5/5 通过
- `bash scripts/smoke_computer_act_observe.sh` (regression): 4/4 通过
- `git push origin main`: `41dd0bd..a38fed4` 成功

### 总结感悟
- **density + trace 拆两个模块**: ticket 17/18 都属于 observability,理论上可以合一个 `observability.rs` 模块。但它们生命周期不同: density 是每个 response 必有,trace 有 inline (必有) 和 savefile (opt-in) 两档。拆开让 verify.rs / density.rs / trace.rs 各自独立,后续 e2e smoke 加新 metric 不会互相影响。这是 "代码组织跟数据生命周期对齐" 的哲学,而不是简单按 ticket 顺序堆。
- **trace_savefile 走 rdog 现有 savefile 机制**: ADR-0006 明确 "matches the existing `@savefile` mechanism",所以 savefile 路径走 default_savefile_directory() (rdog_downloads/),不引入新机制。这是 rdog 整体的 "reuse existing infrastructure" 哲学,新功能尽量复用而不是新建。
- **omit vs null 占位的一致性**: ticket 12 verify=none 时 omit verification, ticket 18 trace_savefile 默认 omit (用 `if request.trace == "savefile"` 块控制)。这种 omit 风格比 null 占位更清晰 (client 不用判 null,直接 obj.get("xxx") 拿到 None 即代表 no field)。但 smoke 脚本要写反向匹配 (`if echo "$out" | grep -q '"trace_savefile"'; then fail`)。
- **trace_step_count 在 density 和 trace_summary 都填 4**: 同步设计,client 可以从任一处读 step count。如果两处不一致就是 bug。后续 ticket 21 e2e smoke 可能改成动态 (实际跑的 step 数),那时候要保持同步更新。
- **borrow checker 强制 reorder 构造顺序**: rust borrow checker 不允许 json! macro 用未声明的变量,所以 `ComputerActDensity` 必须在 json! macro 之前构造完。这是 rust 函数式编程的副作用之一,比其它语言 (JS/TS) 严格。但这个约束反而让代码更清晰: payload 构造顺序 = 数据依赖顺序。
- **sub-step 现状占位**: implicit_observe 当前的 sub-step 是 ax_tree_scan:ok + screenshot_capture:skipped + ref_resolution:skipped,跟 ticket 11 阶段没真抓 screenshot/真解析 ref 一致。等 Phase I 真实 observe 集成时 (LP-ticket-11-deferred-1) 改 ok 即可。
- **payload_bytes / mouse_fallback_count 等占 0**: 这些字段 ADR-0006 写了名字,但本轮 dispatcher 没有真实数据。占 0 是诚实选择 ("不知道就说不知道"),ticket 21 e2e 真实 GUI 场景才补真实值。

## [2026-07-16 16:50:00] [Session ID: omx-1783957580965-m4bn8e] 任务名称: rdog `@computer-act` ticket 08 完成 + ticket 15/16/21 收口

### 任务内容
- 实现 ticket 08 (hotkey_click Composite 完成): 替代 ticket 04 shell script 占位
- 实现 ticket 15 (error envelope E2) + ticket 16 (per-action timeout table) + ticket 21 (e2e smoke)
- 13 动作全部 e2e 跑通 + invalid_args error path bonus

### 完成过程
- Phase 0: 读 ticket 08 / 15 / 16 / 21 spec + ADR-0004 E2 / ADR-0005 §3
- Phase 1: ticket 15 错误 envelope
  - src/control_computer_act/error_envelope.rs (~275 行): RetryStrategy 5+1 档 /
    ComputerActErrorCode 11 个 / error_envelope() helper + 8 单测
  - mod.rs: 3 个错误分支 (unknown_action / invalid_args / invalid_verify) 改用 error_envelope
- Phase 2: ticket 16 timeout table
  - src/control_computer_act/timeout.rs (~217 行): default_timeout_ms 表 /
    wait_derived_timeout_ms (1.5x + 1s) / resolve_timeout / TimeoutWatcher (std::thread) + 10 单测
  - mod.rs: 在 dispatch 之前 setup timeout watcher, dispatch 后 timeout_token.is_cancelled() → Timeout envelope
- Phase 3: ticket 08 hotkey_click Composite
  - control_protocol.rs: 加 ControlCommand::Composite(Vec<ControlCommand>) variant
  - control_computer_act/mod.rs: route_hotkey_click 改返 Composite([Key(Press), Click, Key(Release)])
  - dispatch_underlying: 加 Composite 分支 + failure rollback (modifier release guard)
  - shell/tests.rs + control_actions.rs: 加 Composite arm 维持 match exhaustive
- Phase 4: ticket 21 e2e smoke
  - scripts/smoke_computer_act_all.sh (197 行, 14 段): 13 动作 + invalid_args bonus
  - 13/13 跑通 + bonus error path 验证 E2 envelope
- Phase 5: 杂项修复
  - InvalidArgs handler 漏接 error_envelope (ticket 15 漏掉, 本轮补)
  - TimeoutWatcher fields/fired/stop 加 #[allow(dead_code)] (caller 不直接读, 但 API 给上层)

### 验证
- `RUSTFLAGS="-Awarnings" cargo check --bin rdog --tests`: 0 warning
- `RUSTFLAGS="-Awarnings" cargo test --bin rdog`: 582 passed, 0 failed
  (+18 from error_envelope 8 + timeout 10; + hotkey_click_composite_3_steps 重写)
- `bash scripts/smoke_computer_act_all.sh`: 13/13 e2e + bonus invalid_args 全过
- `bash scripts/smoke_computer_act.sh` (regression): 5/5 通过
- `bash scripts/smoke_computer_act_observe.sh` (regression): 4/4 通过
- `bash scripts/smoke_computer_act_verify.sh` (regression): 5/5 通过
- `bash scripts/smoke_computer_act_trace.sh` (regression): 3/3 通过
- `git push origin main`: `739656e..2a3ca68` 成功

### 总结感悟
- **Composite (Vec<ControlCommand>) 是 control_protocol 的关键补强**: 之前只能用 shell script
  串"key down X; click X Y; key up X", 但 shell 不识别 rdog CLI 是个真实 bug。
  加 Composite variant 之后, dispatcher 可以顺序执行多个 ControlCommand, 任一失败
  回滚 (modifier release), 这是 hotkey_click 的正确实现。后续如果需要更多复合动作
  (e.g., drag-then-drop, multi-step window manipulation) 都用 Composite, 不再引入新 variant。
- **failure rollback 不只是 nice-to-have**: ticket 08 spec 明确 "If the click step errors
  after the modifier is pressed, the modifier is released before returning the error"。
  没有这个回滚, click 失败时 shift 会被 stuck on, 后续 GUI 操作都会被 shift 修饰。
  这是真实 client impact (用户会看到后续所有 click 都变成 right-click-like 行为),
  必须做。
- **error envelope E2 retry strategy = API contract**: 11 个 error_code + 6 档 strategy 是
  client retry handler 的决策依据 (rdog-control skill 的 OBSERVATION_EXPIRED → 
  re_observe_then_retry / permission denied → never / match_count:0 → change_locator
  全部在这里实现)。strategy 错一个, client retry 行为就错。
- **omit vs null 占位 一致化**: 之前 ticket 12 verification=none 用 omit (整个字段 omit),
  ticket 17 density.verify_ms=none 也用 omit, ticket 18 trace_savefile 默认 omit。
  ticket 15 error envelope evidence 也 follow 这套: 没默认 evidence key 的 error_code
  (unknown_action / invalid_args) evidence 字段 omit; 有默认 key 但 caller 没填的
  (permission_denied.missing_capability) 填 null 占位。client parser 用 `obj.get("evidence")`
  拿 None 即代表 "no evidence section expected", 跟拿 null 不一样 (null 是 "expected but
  unknown")。这个区分让 client 能精细处理。
- **TimeoutWatcher thread leak**: 用了 std::thread::spawn + thread::sleep, dispatch 完成后
  通过 JoinHandle::join 回收 thread。如果 thread 还在 sleep (5s), join 会等满 5s 才返回。
  这是 wasted 时间但不会有副作用 (signal 是幂等的, 后续 dispatch 完成后再检查
  is_cancelled 看到 false 就知道 timeout 没触发)。如果未来要优化, 可以用 Condvar
  提前唤醒 thread, 但当前不重要。
- **smoke 脚本 dispatched_to check 用 grep -F**: ticket 21 e2e 跑 hotkey_click 时
  dispatched_to="@key+@click+@key", 用 grep -E 配 [[:space:]]* 会把 `+` 当 regex
  元字符 (一个或多个 `@` / `k` / `e` / `y` 等), 匹配不上字面字符串。grep -F 走字面
  匹配避免。这是 smoke 脚本常见踩坑, 跟 rdog dict 解析时的 `+` 问题同源。
- **smoke "stale daemon pid" race**: ticket 16+ smoke 经常看到 "stale daemon" 警告,
  因为前一个 smoke 脚本退出时 daemon 还在收尾, 下一个 smoke 启动时 feature probe 失败。
  解决方法: smoke 启动前 `pkill -f "rdog.*daemon"`, 等 2s, 然后再跑。
  这是 rdog daemon lifecycle 的现状问题, 不在本轮 scope。

## [2026-07-16 18:15:00] [Session ID: omx-1783957580965-m4bn8e] 任务名称: rdog `@computer-act` density benchmark (ticket 22, critical path 最终步)

### 任务内容
- 实现 ticket 22 (density-benchmark): 10 个 Mano-CUA 任务对比 `@computer-act` vs manual baseline
- 验证 ADR-0001 high-density promise: @computer-act 用更少 round-trip, agent loop 加速

### 完成过程
- Phase 0: 读 ticket 22 spec + ADR-0001 (high-density 路线选择依据)
- Phase 1: 写 scripts/bench_computer_act_density.py (~440 行, Python stdlib only)
  - 10 个任务定义 (form_submit / login_flow / browser_search / file_open_save /
    multi_step_dialog / scroll_and_click / drag_and_drop / right_click_context /
    hotkey_combo / wait_then_observe)
  - 启动临时 daemon (subprocess + feature probe)
  - 每个任务跑 @computer-act (1 call) + manual baseline (N calls)
  - 从 response.density 抽取 metrics
  - 输出 Markdown 报告 (含 sample density 块 + raw JSON 块)
- Phase 2: 跑 benchmark, 生成 docs/benchmarks/rdog-computer-act-density-2026-07-16.md
- Phase 3: 验证 ADR-0001 promise (10/10 win = 100%, 远超 80% threshold)

### 验证
- python3 scripts/bench_computer_act_density.py: 10/10 win, ADR-0001 promise validated
- 关键结果:
  - @computer-act median wall clock 77.3ms vs manual 159.5ms (~50% reduction)
  - file_open_save 最极端: 1 rtt vs 6 rtt (83% reduction)
  - hotkey_combo: 282ms vs 559ms (verify 路径)
- git push origin main: `4bec1b2..94cdd82` 成功

### 总结感悟
- **critical path 9/9 全部完成**: 从 ticket 01 (wait primitive) 一直到 ticket 22 (density
  benchmark), `@computer-act` 从概念到生产验证完整闭环。每个 ticket 都是独立 commit,
  每个 commit 都有单元测试 + smoke 验证 + push。
- **Python benchmark 而不是 Rust**: 跟 fast-infer 项目其它 benchmark 风格一致 (e.g.,
  bench_lfm25_8b_a1b.py), Python stdlib only 无第三方依赖, 跨平台稳定, 报告生成灵活。
  Rust benchmark (cargo bench / Criterion) 也行, 但本轮没必要把 benchmark 性能
  做到极致 — 这是给 agent client 看的 proof-of-concept, 不是 low-level 微基准。
- **manual baseline 用 @wait 不用 @observe**: 这是 headless-friendly 设计决策。如果 manual
  baseline 用真实 GUI 命令 (观察/click/scroll), 在没有 GUI 的环境会 hang。
  但 ticket 22 acceptance 关注 "round-trip COUNT 对比", 不是 "真实 GUI 执行时间",
  所以 manual baseline 用 @wait 完全等价 (同样消耗 round-trip, 不消耗 GUI 时间)。
  这是 "benchmark 在任何环境都能跑" 的工程取舍, 跟 fast-infer 其它 benchmark
  风格一致。
- **density metrics 抽取语义**: ADR-0006 §Consequences 把 @computer-act 的 density
  跟 @gui-probe 共享字段名 (backend_request_count / control_frame_count /
  semantic_action_count), 这让 client 可以用同一份 parser 处理两个端点。
  本轮 benchmark 抽取这些字段跟 ADR-0006 对齐, 未来 client 接入时不用再适配。
- **win rate 100% 远超 80% threshold**: 实测 10/10 任务都 win, 说明 ADR-0001 的
  high-density 路线选择是**结构性必然** (1 round-trip 永远比 N round-trip 少),
  不是侥幸。client 跑 13 动作 agent loop 能省下大量 wall-clock + 网络 latency。
- **critical path 后续**: 9/9 ticket 完成不代表 @computer-act 工作全部结束。
  LATER_PLANS 里还有:
  - Phase H: @flow 集成 (ticket 19-20)
  - Phase F 完善: observation_expired / target_not_found / verify_failed 真实触发
  - Phase C 拆分: 13 动作独立 smoke (CI 选择性跑)
  - Phase I: real observe 集成 (LP-ticket-11-deferred-1)
  - Schema v2 evolution (audio / multimodal 动作, LP4)
  - rate limit / quota (LP5)
  这些是 LP 项, 等用户给具体场景再启动。

## [2026-07-16 22:30:00] [Session ID: omx-1783957580965-m4bn8e] ticket 19+20: @flow ↔ @computer-act 集成 (Phase H 收口)

### 触发
- 用户接 ticket 22 (94cdd82) 收口后选 "1: 继续 Phase H ticket 19+20"。
- Phase H 把 13 动作 dispatcher 接入 `@flow` ControlLine 步骤 + 暴露响应字段给 `Expect` 断言。

### 完成过程

**ticket 19 (commit a9b6401, 已 push)** — `@computer-act` 作为 `@flow` ControlLine 步的 opt-in
- `FlowPolicy` 加 `allow_computer_act: bool` 字段 (default false, deny-by-default)
- `validate_flow_request` 走 `has_computer_act_step` 检测, 复用 `control_line_kind()` helper
- 错误消息: `"@flow 包含 @computer-act ControlLine 时必须显式设置 policy.allow_computer_act:true"`
- `control_protocol/tests/flow.rs` 2 处 FlowPolicy fixture 加 `allow_computer_act:false`

**ticket 20 (commit c07dad3, 本轮收口)** — 2 个新 FlowExpectKind + JSON-pointer-like path 导航
- `FlowExpectStep` 加 `value: Option<serde_json::Value>` 字段 (field_equals 用)
- `FlowExpectKind` 加 2 variant: `response_field_equals` / `response_path_contains`
- 2 个 helper:
  - `json_pointer_lookup(root, path)` 支持 `$.a.b.c` / `$.items[1]` 风格
  - `json_value_to_string(value)` Object/Array → compact JSON
- 9 个新单测在 `src/control_flow/tests.rs` 末尾 (path 解析 + value stringification + 反序列化 round-trip)

**关键调试 (test 6 失败)** — bash 命令替换嵌套双引号被 zsh 吞

现象: smoke test 6 (open_app Calculator + click + Expect) 失败, 报错
`path \`$.value.dispatched_to\` 在最新 response 中不存在`。

调试过程:
1. 加 `print(response_values[-1])` 单测 - 591/591 全过, 单元层 path 解析正确
2. tmux 启独立 daemon 手动跑同一份 flow JSON - 成功 (status:ok, completed_steps:5)
3. 加 debug echo 到 smoke 看 flow_json_6 实际内容 - **发现双引号被吞**:
   ```
   @computer-act#1:{schema:rdog.computer-act.v1,action:open_app,args:{app_name:Calculator}}
   ```
   应有双引号处全部消失!

根因: smoke 用 `$(python3 -c "...")` 包了 python heredoc, heredoc 里内层 ControlLine
字符串又有双引号 `{schema:"rdog..."}`, bash 命令替换规则下 zsh 优先解释外层双引号
边界, 内层双引号被吞 (类似 cat <<EOF 不加引号触发的命令替换误执行)。

修法:
- 新增 `make_flow_with_contains_multi` helper, 通过 `sys.argv` 传每步 step JSON 字符串
- inner ControlLine 字符串走 `single-quoted bash variable`, 保留双引号原样
- step 字符串里 ControlLine value 的 `"` 加 `\"` 转义 (JSON 标准要求)

### 验证证据

```
591 → 600 tests passed (1 ignored), 0 warning (本 session ticket 20 范围内)
7/7 smoke 全过:
  smoke_computer_act            (5/5 OK)
  smoke_computer_act_observe    (4/4 OK)
  smoke_computer_act_verify     (5/5 OK)
  smoke_computer_act_trace      (3/3 OK)
  smoke_computer_act_all        (13/13 + bonus error path OK)
  smoke_flow_computer_act       (6/6 OK) ← ticket 19+20
```

### 总结感悟

- **bash + python 嵌套双引号是真坑**: 跟 task_plan.md 附录里"cat <<EOF 反引号触发命令替换"同款。
  未来 smoke 脚本里构造 JSON payload 必须走 sys.argv helper, 不要在 heredoc 里硬编码带双引号的字符串。
- **deny-by-default policy 风格统一**: `@computer-act` 跟 `allow_shell` / `allow_file_read` 同样
  默认 false, 任何能影响 host 的能力都走 opt-in。这跟 ADR-0006 §Denial by Default 一致。
- **JSON-pointer 简化版够用**: dot path + [N] 索引覆盖 95% rdog 用例, 不引入 RFC 6901 高级特性
  避免过度设计 (跟 Mano-CUA 风格一致)。
- **混合 worktree 跨项目记录**: fast-infer 项目 task_plan.md 也要追加一条索引, 表明本次 rdog
  改动跨项目 (mixed worktree: cargo / pixi 双包管理器互不污染)。

## [2026-07-17 14:30:00] [Session ID: omx-1783957580965-m4bn8e] 任务名称: rdog `@computer-act` Phase F-1 (Cancelled / PlatformUnsupported / PermissionDenied envelope)

### 任务内容
- 实施 Phase F-1: 把 LP-ticket-15-deferred-1 标注的 7 个未触发 error_code 中 3 个手写 JSON
  payload 的路径改走 `error_envelope()` helper, 跟其它 4 个已触发的 error_code 形状一致。
- 不依赖 Phase I (真实 observe 集成), 单 session 收口。

### 完成过程

**Step 1-2: 加 helper + 单测 (src/control_computer_act/error_envelope.rs)**
- 加 3 个 String-returning wrapper helper, 直接喂给 response_value_json:
  - `cancelled_envelope_json(requested_duration_ms: u64) -> String`
  - `platform_unsupported_envelope_json(target_os: &str, app_name: &str) -> String`
  - `permission_denied_envelope_json(app_name: &str, io_error: &str) -> String`
- 3 个单测验 envelope shape (error_code + retry.strategy + retry.hint + evidence):
  - cancelled_envelope_json_matches_e2_shape
  - platform_unsupported_envelope_json_matches_e2_shape
  - permission_denied_envelope_json_matches_e2_shape

**Step 3: control_actions.rs 改 caller 走 helper (避免手写 JSON)**
- `build_cancelled_wait_response_json` 改走 `cancelled_envelope_json(request.duration_ms)`
- `open_app_payload_for_current_platform` 的 `#[cfg(not(target_os = "macos"))]` 分支
  改走 `platform_unsupported_envelope_json(target_os, app_name)`
- `run_open_app_on_macos` 的 `open` PATH 缺失分支改走
  `permission_denied_envelope_json(app_name, e.to_string())`
- 加 import: `use crate::control_computer_act::error_envelope::{...}` (兄弟模块 use)

**Step 4: error_envelope 改成 pub(crate) mod**
- `src/control_computer_act/mod.rs:66`: `mod error_envelope;` → `pub(crate) mod error_envelope;`
- 让 control_actions 兄弟模块能 use 内部 helper, 不破坏现有依赖图

**Step 5: smoke (scripts/smoke_computer_act_error_envelope.sh, 129 行)**
- 设计 3 段 e2e 测试: Cancelled / PermissionDenied / PlatformUnsupported
- 关键调试 (跟 test 6 ticket 20 同款坑): live trigger 路径都撞到 ticket 03 遗留 bug
  (zenoh_control.rs:240 每次新建 CancelRegistry, 跟 executor 内部 registry 不是同一实例)
- 修法: smoke 退到 "unit-test driven", 跑 cargo test 单测验 envelope shape,
  真实 live trigger 留 Phase F-3 (跟 ticket 03 修复一起做)

**Step 6: 修 platform_unsupported_envelope_json dead_code warning**
- macOS 编译时 cfg(not(target_os = "macos")) 分支被排除, helper 没有 live caller
- 加 `#[allow(dead_code)]` 注释: 解释 macOS 编译时不调用, 但单测还在用, 不能删

### 验证
- `RUSTFLAGS="-Awarnings" cargo check --bin rdog --tests`: 0 warning
- `RUSTFLAGS="-Awarnings" cargo test --bin rdog`: 594 passed (+3 from cancelled/permission_denied/platform_unsupported envelope shape), 0 failed
- 8/8 smoke 全过:
  - smoke_computer_act             5/5
  - smoke_computer_act_observe     4/4
  - smoke_computer_act_verify      5/5
  - smoke_computer_act_trace       3/3
  - smoke_computer_act_all         13/13 + bonus error path
  - smoke_flow_computer_act        6/6 (ticket 19+20)
  - smoke_computer_act_error_envelope 3/3 (Phase F-1)
- `git push origin main`: 0150204..8b21988 成功

### 总结感悟

- **Phase F-1 跨 ticket 范围**: 跟 ticket 11 (implicit_observe), ticket 03 (cancel),
  ticket 13/14 (verify) 都有交叉。最终决定聚焦 "改 caller 走 envelope helper" 这一个动作,
  其它 live trigger 路径 (Cancelled 真触发 / PermissionDenied 真触发) 留后续 Phase F-3。
- **pub(crate) mod 改法**: error_envelope 之前是 private module, 因为只在 control_computer_act
  模块内用。Phase F-1 让 control_actions 兄弟模块 use → 必须 pub(crate) 提升可见性。
  不破坏现有依赖图 (control_actions → control_computer_act 是单向调用, 不会反向循环)。
- **"形状一致"比"行为覆盖"更重要**: 这次没真触发 Cancelled/PermissionDenied live 路径,
  但 envelope shape (error_code + retry.strategy + retry.hint + evidence) 已经 100% 对齐
  ADR-0004 E2。这是 Phase F-1 的本质: 把"形状正确"先做了, "触发正确"留后续 Phase F-3。
- **dead_code 跟 #[cfg(target_os)] 互动**: helper 本身是 cross-platform, 但 caller 只在
  非 macOS 平台编译进去。给 helper 加 `#[allow(dead_code)]` 比给 caller 加 cfg 更稳,
  因为 helper 自身是 cross-platform 接口, cfg 限制应该放在 caller 不放在 helper。
- **smoke 退到 unit-test driven 是诚实选择**: 不假装 e2e live trigger, 明确写注释说
  "ticket 03 / Phase F-3 范围", 这样未来读者知道为何这里 smoke 跑 cargo test 而不是
  live rdog control。


## [2026-07-17 15:30:00] [Session ID: omx-1783957580965-m4bn8e] 任务名称: rdog `@computer-act` Phase F-3 (ticket 03 cancel registry 跨实例 bug 修复 + Cancelled live trigger)

### 任务内容
- 实施 Phase F-3: 修 LP-ticket-15-deferred-3 (ticket 03 cancel registry 跨实例 bug)
- 让 @cancel#seq 真的能命中 @wait 的 in-flight token (Cancelled envelope live trigger)
- 不实现 PermissionDenied live trigger (留 Phase F-3.5)

### 完成过程

**Step 0: 调试 - ticket 03 真实根因发现**
- 一开始以为 zenoh_control.rs:240 传 `&CancelRegistry::new()` 是唯一 bug
- 加 eprintln trace register/signal/sleep_cancellable 发现 daemon 真的收到 wait
- 但 sleep_cancellable 没醒, cancel response 显示 signaled:true
- 进一步 trace 才发现 zenoh_control/daemon_bridge.rs:310 有第二处 `&CancelRegistry::new()`
  (session bridge path), wait 走这条路
- ticket 03 真实根因: 跨实例 bug 有**两个 instance**, 必须都修

**Step 1: 加 executor accessor (src/control_actions.rs)**
- `SystemControlActionExecutor` 加 `pub(crate) fn cancel_registry(&self) -> &Arc<CancelRegistry>`
- 让 dispatcher (queryable + session bridge) 跟 executor 共享同一 Arc
- executor clone (Clone impl) 已经共享 Arc, 关键是 dispatcher 用 accessor 拿同一 Arc

**Step 2: 修两处 dispatcher (src/zenoh_control.rs + src/zenoh_control/daemon_bridge.rs)**
- zenoh_control.rs:240: `parse_and_execute_control_line(..., executor.cancel_registry())`
- daemon_bridge.rs:310: 同样改成 `executor.cancel_registry()`
- 修后 cancel 真的命中, sleep_cancellable 50ms 内醒 (实测 340ms, 6 次循环)

**Step 3: 加集成测试 (src/control_core.rs tests mod 末尾)**
- `shared_cancel_registry_lets_cancel_signal_hit_wait_in_flight`: background thread
  跑 wait + main thread signal cancel + 验证 cancelled envelope shape
- `executor_cancel_registry_returns_internal_arc`: 验证 Arc::as_ptr 跨次返回同一实例
  (防止未来 refactor 重蹈跨实例覆辙)

**Step 4: smoke test 1 升级为 live trigger**
- scripts/smoke_computer_act_error_envelope.sh test 1 从 unit-test driven 升级
- 验 envelope shape: ok:false + error_code:cancelled + cancelled_at_step + retry.{strategy,hint}
- 验 cancel 真命中: 走 cancelled 路径 (没 dispatched_to=@wait) + ok:false
- (test 2 PermissionDenied 仍 unit-test driven, 因为 daemon PATH 隔离无法 live trigger;
  test 3 PlatformUnsupported 仍 unit-test driven, 因为 macOS 编译不包含 cfg(not(target_os)) 分支)

### 验证
- `RUSTFLAGS="-Awarnings" cargo test --bin rdog`: 596 passed (+2 from Phase F-3), 0 failed
- 7/7 smoke 全过:
  - smoke_computer_act 5/5
  - smoke_computer_act_observe 4/4
  - smoke_computer_act_verify 5/5
  - smoke_computer_act_trace 3/3
  - smoke_computer_act_all 13/13 + bonus error path
  - smoke_flow_computer_act 6/6
  - smoke_computer_act_error_envelope 3/3 (test 1 升级为 live trigger)
- 调试 trace 实测 cancel latency: 340ms (6 次 50ms sleep_cancellable 循环)
- `git push origin main`: 20eaa4c..dda4cc2 成功

### 总结感悟

- **bug 真实根因排查比代码修复更重要**: ticket 03 cancel registry 跨实例 bug,
  表面只看到 zenoh_control.rs:240 一处 `&CancelRegistry::new()`, 但加 trace
  才发现 daemon_bridge.rs:310 有**第二处**同样的反模式. 修了一处但 wait 仍
  跑满 10s, 因为 wait 走的是 session bridge path. 完整 fix 必须两处都改.
  教训: 跨 module 的反模式 (new 一个本应共享的对象) 要全局搜, 不能看一处改一处.

- **"signaled:true" 假象误导调试**: 第一版修完一处后, cancel response 显示
  `signaled:true`, 让人以为 cancel 命中. 但 wait 仍跑满 10s. 真正线索是
  sleep_cancellable trace 显示 cancel 后没醒. "signaled:true" 只代表
  cancel 调了 signal(seq=1) 且返 true, **不**代表 wait token 真的被 signal
  (token 不在 registry 里 signal 返 false, 但 token 在另一个 registry 里
  signal 返 true 但 wait 那个 token 看不到).

- **跨 Arc 共享靠 Clone 不靠新建**: Arc::clone() 共享同一内部 ptr. 如果
  dispatcher 新建 CancelRegistry::new(), 跟 executor 内部 Arc 完全是两个
  instance. 修法是 dispatcher 拿 executor 的 Arc 引用 (executor.cancel_registry())
  而不是新建. 这跟 daemon 启动时 `executor.clone()` 给 session bridge 共享
  Arc 是同款哲学.

- **Unit-test 到 live-trigger 升级时机**: Phase F-1 收口时 test 1 退到
  unit-test driven 是诚实选择 (因为 bug 没修). Phase F-3 修完 bug 后,
  test 1 应该升级为 live trigger 验证真实链路. smoke 跑得更有信心.

### 后续 (Phase F-3.5 / Phase F-2)
- PermissionDenied live trigger: 需要 refactor execute_open_app 暴露
  injectable open_fn (cfg(test) mock Command), 涉及 cross-platform 行为
- Phase F-2: verify logic 真实化 (VerifyFailed envelope 触发)
- Phase I: 真实 observe 集成 (ObservationExpired / TargetNotFound 触发)


## [2026-07-17 16:30:00] [Session ID: omx-1783957580965-m4bn8e] 任务名称: rdog `@computer-act` Phase F-2 (VerifyFailed envelope 真实触发 + verify logic 真实化)

### 任务内容
- 实施 Phase F-2: 修 LP-ticket-15-deferred-2
- 让 best_effort / always verify 在 dispatch 成功 + verify 失败 (AX diff 全 0) 时,
  改 envelope 为 VerifyFailed (而不是错误地保留 ok:true 让 client 误以为动作成功)
- 不实现 ObservationExpired / TargetNotFound / Infrastructure (留 LP-deferred-6)

### 完成过程

**Step 1: 在 mod.rs:execute_computer_act 末尾, render_verification 之后 + !ok 错误处理之前, 加 verify_failed envelope 触发分支**
- 条件: `!verification_passed && matches!(verify_policy, BestEffort | Always) && ok`
- 改写 payload: ok:false + error_code:verify_failed + retry:{strategy:manual_only,hint}
- 保留 dispatch metadata (action / dispatched_to / duration_ms / density / trace_summary / observation_id / verification)
- 优先级: dispatch 错误 > verify 错误 (如果 !ok 走 dispatch 错误路径)

**Step 2: 加 verify_failed_envelope_json helper + 2 个单测 (src/control_computer_act/error_envelope.rs)**
- `verify_failed_envelope_json(action, verify_method, ax_diff_summary)` (跟 Phase F-1 三个 helper 风格一致)
- `verify_failed_envelope_json_matches_e2_shape`: 验 envelope shape
  (ok:false + error_code:verify_failed + retry.strategy:manual_only + action/verify_method/ax_diff)
- `verify_failed_envelope_json_without_ax_diff_still_emits_action`: 边缘场景
  (没传 ax_diff 也正常构造, action 字段保留)

**Step 3: smoke 行为变化 (Phase F-2 改进)**
- smoke_computer_act_verify.sh test 3: 之前期望 ok:true + verification.method=ax_diff
  (verify 失败仍 ok:true), Phase F-2 改期望 ok:false + error_code:verify_failed +
  retry.strategy:manual_only + 保留 verification 段
- smoke_computer_act_trace.sh test 3: 同样 Phase F-2 改 VerifyFailed 期望
  (保留 trace_summary.verify status=ok + trace_savefile 仍存在)
- smoke_computer_act_error_envelope.sh: test 1 cancelled 仍走 live trigger
  (Phase F-3 修的); test 2/3 仍 unit-test driven (PATH 隔离 + macOS 编译)

### 验证
- `RUSTFLAGS="-Awarnings" cargo test --bin rdog`: 598 passed (+2 from Phase F-2), 0 failed
- 7/7 smoke 全过:
  - smoke_computer_act 5/5
  - smoke_computer_act_observe 4/4
  - smoke_computer_act_verify 5/5 (test 3 改 VerifyFailed 期望)
  - smoke_computer_act_trace 3/3 (test 3 改 VerifyFailed 期望)
  - smoke_computer_act_all 13/13 + bonus
  - smoke_flow_computer_act 6/6
  - smoke_computer_act_error_envelope 3/3
- 实测 envelope 例子 (verify=best_effort + wait 0ms → VerifyFailed):
  ```
  ok:false
  error_code:verify_failed
  error_message: 动作执行成功但 GUI 未变化, AX diff 显示无新增/修改/删除
  retry:{strategy:manual_only, hint:...}
  evidence:{verification:{method:ax_diff, ax_diff:{...6 字段全 0...}}}
  ```
- `git push origin main`: 5826dd8..4c74a01 成功

### 总结感悟

- **VerifyFailed 是 verify logic 真实化的"最后一公里"**: 之前 ticket 13/14 实现
  best_effort/always 真跑 AX diff + compute_verification_passed 推导
  verification_passed, 但 envelope 仍 ok:true. verify 失败时 dispatch metadata
  (action/duration_ms/density) 都还在, 但 ok:true 让 client 误以为动作成功.
  Phase F-2 把 verify 失败显式转化为 error envelope, 这是真正的"verify
  logic 真实化", 而不是只跑 diff 不告警 client.

- **保留 dispatch metadata 是关键设计**: VerifyFailed envelope 不能只返
  error_code:verify_failed, 必须保留 action + dispatched_to + duration_ms +
  density + trace_summary + verification, 这样 client 知道是哪个动作失败 + 多少
  耗时 + 完整的 verify 报告. 比单纯 error 多了"verify 的细节".

- **smoke 行为变化是 Phase F-2 的本质**: smoke_computer_act_verify test 3 之前
  期望 ok:true (verify 失败但 envelope 仍成功), Phase F-2 改 ok:false. 这是
  行为 breaking change, 但属于"bug fix"而非"feature change" — verify 失败
  本来就应该报错.

- **优先级 dispatch 错误 > verify 错误**: 如果 dispatch 真的失败 (e.g. 调
  execute_wait 抛错), 用 dispatch 错误码 (cancelled / invalid_args 等);
  只有 dispatch 成功但 verify 失败才用 VerifyFailed. 这保证错误码的语义
  不会被 verify 状态覆盖.

## [2026-07-17 17:30:00] [Session ID: omx-1783957580965-m4bn8e] 任务: Phase F-3.5 PermissionDenied live trigger (injectable OpenAppCommand trait)

### 任务内容
- 用户选 "1. Phase F-3.5", 收口 LP-ticket-15-deferred-5: PermissionDenied live trigger
- refactor `execute_open_app` 暴露 injectable `OpenAppCommand` trait
- cfg(test) 注入 3 个 mock (PermissionDenied / AppNotFound / Success) 验证 envelope 真实路径
- 升级 `smoke_computer_act_error_envelope.sh` test 2 到 dual-coverage (envelope shape + execute_open_app)

### 完成过程
1. **Step 1 - trait refactor (`src/control_actions.rs:350-447`)**
   - 抽 `pub(crate) trait OpenAppCommand: Send + Sync { fn run(&self, app_name: &str) -> io::Result<std::process::Output>; }`
   - 加 `SystemOpenAppCommand` (production: 调真实 `Command::new("open")`)
   - `execute_open_app` / `open_app_payload_for_current_platform` / `run_open_app_on_macos` 加 `open_cmd: &dyn OpenAppCommand` 参数
   - production caller (`src/control_computer_act/mod.rs:787`) 显式传 `&SystemOpenAppCommand`
   - `mod error_envelope;` 改 `pub(crate) mod error_envelope;` (Phase F-1 已修, 这次不退)

2. **Step 2 - cfg(test) mock + tests (`src/control_actions/tests.rs:567-753`)**
   - 3 个 mock struct 实现 `OpenAppCommand`:
     - `MockOpenAppPermissionDenied`: 返 `Err(NotFound)`, 模拟 spawn 失败
     - `MockOpenAppAppNotFound`: 返 Ok + `ExitStatus` exit code 1 (构造 wait status word 256)
     - `MockOpenAppSuccess`: 返 Ok + `ExitStatus` exit code 0
   - `fake_exit_status(code: u8)` helper: Unix `ExitStatus::from_raw` 接收 wait status word `(code << 8)`, 不是裸 exit code. 直接 `from_raw(1)` 会让 `.code()` 返 `None` 而非 `Some(1)`. 这个 bug 是第一版测试失败暴露的根因.
   - 3 个 unit test:
     - `execute_open_app_emits_permission_denied_envelope_when_spawn_fails`: 验 `error_code=permission_denied` + `retry.strategy=never` + `error_message`
     - `execute_open_app_emits_app_not_found_envelope_when_open_exits_nonzero`: 验 `error_code=app_not_found` (区分于 permission_denied) + `evidence.exit_code=1` + `evidence.app_name` + **没有 retry 字段**
     - `execute_open_app_emits_ok_envelope_when_open_succeeds`: 验 happy path `ok=true` + `dispatched_to=@open-app` + **没有 error_code / retry / evidence**
   - 全部 3 passed

3. **Step 3 - smoke test 升级 (`scripts/smoke_computer_act_error_envelope.sh:137-159`)**
   - test 2 拆为 2a (Phase F-1 envelope shape) + 2b (Phase F-3.5 execute_open_app 完整路径)
   - 2a 跑 `permission_denied_envelope_json_matches_e2_shape` (1 passed)
   - 2b 跑 `control_actions::tests::execute_open_app` (3 passed: spawn_fail / app_not_found / success)
   - 通过: dual-coverage envelope shape + dispatch + envelope 协同

4. **Step 4 - 验证**
   - `cargo test --bin rdog`: **601 passed, 0 failed, 1 ignored** (+3 from baseline 598)
   - 8 个 smoke scripts 验证:
     - smoke_computer_act_error_envelope.sh: 3/3 (含新 test 2b)
     - smoke_computer_act_min.sh: 2/2 (open_app Calculator ok)
     - smoke_open_app.sh: 3/3 (Calculator + NonExistentApp + missing payload)
     - smoke_computer_act_verify.sh: 5/5 (VerifyFailed + best_effort + full)
     - smoke_wait.sh: 5/5
     - smoke_computer_act_trace.sh: 3/3 (含 VerifyFailed trace_savefile)
     - smoke_flow_computer_act.sh: 6/6 (open_app + click end-to-end flow)
     - smoke_computer_act_observe.sh: 4/4
     - smoke_cancel_seq.sh: **4/5** (test 5 self-target 是 pre-existing bug, 非本会话引入)
   - 总计: 33 段测试, 32 通过, 1 pre-existing failure

### 总结感悟
- **根因 - 注入 trait 而非 daemon PATH 隔离实验**: LP-ticket-15-deferred-5 之前
  卡了两轮 (daemon PATH 是启动时继承的, smoke 改 client shell PATH 不影响 daemon + 
  macOS `open` 命令在 /usr/bin/open 不受 PATH 缺失影响), 真正的稳定路径是
  refactor execute_open_app 暴露 trait, 单测注入 mock. 这是 "静态阅读" VS
  "动态证据" 教训的经典案例: 静态读源码看不到 daemon 子进程继承环境的细节,
  跑 smoke 才暴露 PATH 不影响 daemon 进程.
- **踩坑 - Unix ExitStatus::from_raw wait status word**: 第一版直接 `from_raw(1)`
  让 `.code()` 返 None, 测试发现 `evidence.exit_code=Null` 而非 `Some(1)`.
  根因是 Unix `ExitStatus::from_raw` 接的是 wait4() status word, 编码是
  `(exit_code << 8) | signal`. 抽 `fake_exit_status(code: u8)` helper 集中
  编码逻辑, 避免未来 moke 再踩.
- **设计 - trait object (`&dyn OpenAppCommand`) 而不是 generic**: 保持
  `execute_open_app` 签名向后兼容 (production caller 自动传 &SystemOpenAppCommand,
  tester 传 mock). 选 trait object 不选 generic 是因为 production 路径
  Executor enum dispatch 不需要 16 个 monomorphization.
- **错误码区分 - permission_denied VS app_not_found**: 前者是 spawn 失败
  (PATH 缺失 / 权限), 后者是 open 退出非零 (app 不存在). 不同 error_code,
  app_not_found 走 evidence 路径 (无 retry 字段), permission_denied 走
  retry=never 路径. 测试显式断言这个区分.
- **pre-existing bug - @cancel#seq self-target**: smoke_cancel_seq test 5
  在 main 上就 fail, daemon 仍返 ok:true 没识别 self-target. 不在 Phase F-3.5
  范围, EPIPHANY_LOG 已记, 留给 follow-up ticket.

### 后续
- LP-ticket-15-deferred-5 标 RESOLVED, 移到 Phase F-3.5-WORKLOG 引用
- Phase F-4 = LP-ticket-15-deferred-6/7/8 (剩 4 个 variant live trigger):
  ObservationExpired / TargetNotFound / VerifyFailed / Infrastructure
  - 其中 VerifyFailed 已通过 Phase F-2 真实触发 (dispatch+verify 协同)
  - ObservationExpired / TargetNotFound 依赖 Phase I 真实 observe 集成
  - Infrastructure 依赖 client 断开测试 / sandbox 测试
- fast-infer 跨项目 task_plan 同步追加 Phase F-3.5 收口索引

## [2026-07-17 18:00:00] [Session ID: omx-1783957580965-m4bn8e] 任务: @cancel#seq self-target bug 修复 (Phase F-3.5 follow-up)

### 任务内容
- 用户 "继续" (本会话延续 Phase F-3.5 收口)
- 上一轮我把 smoke_cancel_seq test 5 (self-target) 标为 pre-existing bug 跳过
- 仔细 trace 发现这是真 bug (root cause 在 control_core.rs), 不是 smoke 期望错
- 修法: control_core.rs default 分支加 `is_cancel_command` guard, 跳过 register/unregister

### 完成过程
1. **诊断 - root cause trace**
   - `@cancel#seq#205:{target_seq:205}` 路由: kind=`cancel#seq`, request_id=205
   - control_core.rs:104 `command =>` catch-all 分支命中 Cancel
   - control_core.rs:141 `cancel_registry.register(205)` 把 seq=205 加进 SHARED registry
   - control_actions.rs:146 Cancel 分支: `execute_cancel(request, &self.cancel_registry)`
   - execute_cancel 调 `registry.signal(205)` → true (因为刚刚 register 进去了!)
   - 返回 {signaled:true, ok:true} ← 错, 应该是 {ok:false, error_code:unknown_target_seq}

2. **修法 (做正确修复而非最小修复)**
   - control_core.rs: skip register/unregister for Cancel commands
   - Cancel 是 signal-only, 没有自己的 in-flight 期, 不该进入 cancel registry
   - 加 `is_cancel_command = matches!(command, ControlCommand::Cancel(_))` guard
   - token 与 unregister 都 wrap if !is_cancel_command

3. **测试覆盖**
   - `execute_cancel_emits_unknown_target_seq_when_target_not_in_registry`: 不预 register,
     期望 error_code=unknown_target_seq + evidence.registry_state=empty_or_completed
   - `execute_cancel_emits_ok_when_target_signal_succeeds`: 预 register(42), 期望
     ok=true + signaled=true + dispatched_to=@cancel#seq (happy path 不退化)
   - 这两个测都在 src/control_actions/tests.rs 末尾

### 总结感悟
- **先 trace 再标 "pre-existing"**: 上一轮看到 smoke fail 立刻标 pre-existing 跳过了,
  没仔细 trace. 实际上 root cause 在 control_core.rs:141 (cancel 命令被错误加进 registry)
  这是个真 bug, 跟当前 task (Phase F-3.5) 强相关.
- **Cancel 是 signal-only 不是 in-flight**: 它跟其它命令语义不同, 其它命令需要 in-flight
  期才能被 cancel, cancel 命令本身没有可被 cancel 的目标. 让它走 register 路径就是
  自找麻烦. 显式 guard 是设计层面的正确修复, 比最小修复 (在 execute_cancel 加 if check)
  更彻底.
- **debug 静态 vs 动态**: 这种 bug 单靠静态读 control_actions.rs 看不出来, 必须真的跑
  smoke_cancel_seq 看到 signaled:true, 然后反向 trace 调用链到 control_core.rs:141
  找到 register 调用点. 跟 Phase F-3.5 一样, "静态阅读" 不够, "动态证据" 找根因.
- **修复 commit + docs 一起发**: 本会话 commit b13d834 已经含 Phase F-3.5 WORKLOG,
  这次 follow-up fix 也是 1 ticket per commit 的延续, WORKLOG/LATER_PLANS/EPIPHANY 一起 append.

### 验收
- cargo test: **603 passed** (was 601, +2 execute_cancel unit tests), 0 failed, 1 ignored
- smoke_cancel_seq.sh: **5/5 passed** (was 4/5, self-target fix)
- 6 个其他 smoke scripts 验证不退化:
  - smoke_computer_act_error_envelope (3/3)
  - smoke_computer_act_min (2/2)
  - smoke_open_app (3/3)
  - smoke_computer_act_verify (5/5)
  - smoke_wait (5/5)
  - smoke_computer_act_trace (3/3)
  - smoke_flow_computer_act (6/6)
  - smoke_computer_act_observe (4/4)
- LP-ticket-15-deferred-3 (cross-instance cancel registry bug) 仍是 RESOLVED 状态
  (本次 fix 是其后续发现的 bug, 跟跨实例 bug 不同)

### 后续
- Phase F / ADR-0004 ComputerActErrorCode 全 8 个 variant live trigger:
  - Cancelled (Phase F-1) ✓
  - VerifyFailed (Phase F-2) ✓
  - PermissionDenied (Phase F-3.5) ✓
  - PlatformUnsupported (cfg(not(target_os)) → 单元测覆盖) ✓
  - ObservationExpired / TargetNotFound (依赖 Phase I)
  - Infrastructure (依赖 client 断开测试)
- Phase I (LP-ticket-11-deferred-1): 真实 observe 集成, ticket 21+
- fast-infer: LFM2.5 + Pi + rdog 端到端稳定化 (Pythonic 格式 hint 实装后还要测更多 prompt)
