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
