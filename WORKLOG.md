## [2026-08-05 23:38:00] [Session ID: omx-1785926019233-oohizd] 任务名称: Worklog 续档

### 任务内容
- 旧 `WORKLOG.md` 达到 1001 行,已完成持续学习后续档。

### 完成过程
- 旧记录归档为 `archive/default_history/WORKLOG_2026-08-05_233200_app_menu_window_screenshot.md`。
- 归档范围、学习结论和验证见 `archive/manifests/ARCHIVE_MANIFEST__2026-08-05_worklog_rollover_app_menu_window_screenshot.md`。

### 总结感悟
- App 菜单 capture selector 与 screenshot backend gate 已同步到长期经验;本轮没有新增待办或重大风险。

## [2026-08-05 23:15:33] [Session ID: omx-1785926019233-oohizd] 任务名称: Native capture tracing 诊断

### 任务内容
- 为 macOS SCK / xcap screenshot 链路新增结构化 tracing event,区分 timeout、fallback、权限拒绝和非权限终态失败。
- 保持已有 timeout、单 worker gate、SCK -> xcap fallback 与 control error code 行为不变。

### 完成过程
- 新增 `tracing 0.1.44` 和关闭 `tracing-log` feature 的 `tracing-subscriber 0.3.23`;它与既有 `fern` 并行使用同一 `RDOG_LOG_LEVEL` 和 stderr/hidden-file 目标。
- 在 timeout helper 记录 `screenshot_capture_timeout`;在共享 policy 记录 `screenshot_capture_fallback`、`screenshot_capture_permission_denied` 或 `screenshot_capture_failed`。
- 新增事件捕获测试,覆盖 SCK timeout 后 xcap 成功、两个 backend 都失败、权限拒绝不 fallback。
- 已同步 `specs/zenoh-screenshot-control-plan.md` 和 canonical `rdog-control` skill,并完成 `notes.md` 阈值续档。

### 验证
- `cargo fmt -- --check`: 通过。
- `cargo nextest run --package rustdog --bin rdog screenshot::tests`: 30 passed。
- `cargo nextest run --package rustdog --bin rdog`: 683 passed,1 skipped。
- `cargo build --package rustdog --bin rdog`: 成功;17 条现有 warning 位于本任务未触碰模块。
- `RDOG_LOG_LEVEL=info target/debug/rdog --version`: 输出 `rustdog 3.0.0`,确认 logger 与 tracing subscriber 可共同初始化。
- `git diff --check`: 通过。

### 总结感悟
- 对无法取消的 native API,控制面 timeout 只说明等待结束,不代表底层 worker 已停止。日志必须分别记录 deadline、fallback 和单一终态,现场才能避免错误重试。

## [2026-08-06 15:06:56] [Session ID: omx-1785926019233-oohizd] 任务名称: 全模型 macOS ops 评测与 DeepSeek 隔离重跑

### 任务内容
- 重新评测 DeepSeek、MiniMax-M3、qwen3.7、qwen3.6、MiniMax-M2.7-highspeed 的完整 8-case macOS ops suite。
- 废弃受到人工 GUI 干扰的 DeepSeek 首轮结果,用新 artifact 取代。
- 核对 native tracing 引入后 logger 初始化与现有 fern logger 的真实 daemon 兼容性。

### 完成过程
- 新 DeepSeek suite 位于 `/tmp/pi-rdog-macos-ops-deepseek-20260806-145902`;runner 正常 exit 0,汇总为 8/8,每 case 都在首次尝试成功。
- 4 个先前完成的无干扰 suite 也均为 8/8。全矩阵为 40/40;两次 Safari 新标签的 case-level retry 均在第二次成功。
- 通过 `@ping`、`@capabilities` 与完整 live suite 验证 current daemon。Accessibility、Screen Recording、keyboard、screenshot 与 type-text 均为 available。
- 审计 JSONL 后没有发现需要本轮新增协议兼容代码的失败路径。低风险的 `@window-find:APP` 候选已记录为后续项。

### 总结感悟
- 满分 case 不等于零协议错误。评测报告必须同时给出 final success 与 recoverable error 分布,否则会掩盖模型可自愈但有成本的写法偏差。

### 提交与审阅
- 代码审阅为 `APPROVE`,架构审阅为 `WATCH`;watch 项是 fern / tracing 两条输出管线不提供跨 facade 严格排序。
- 运行时代码已提交为 `dbbf7b9 fix(logging): avoid tracing log tracer conflict`。
- 文档记录将在独立 commit 中提交,与运行时代码边界分离。

### 最终提交
- 根仓库文档已提交为 `a267afb docs(context): record macos ops evaluation`。
- 外部评测 runner 已提交为 `7502c1c fix(macos-ops): cover setup capture and cleanup`;其 24 项单测通过,并经独立 `APPROVE/CLEAR` 审阅。
- root 工作树已清理。外部仓库的未跟踪历史上下文文件保持未改,没有混入代码提交。

## [2026-08-07 00:19:24] [Session ID: omx-1786061963768-e7in9l] 任务名称: macOS ops 交互效率工作流规格

### 任务内容
- 新建 `workflows/macos-ops-interaction-efficiency.md`,作为优化 macOS ops agent 交互步数的唯一 workflow 规格。
- 将用户确认的通用性边界、ledger 口径、baseline、brief 和全矩阵认证规则写入 workflow 与原始 notes。

### 完成过程
- 读取当前 runner、canonical skill 和历史评测记录,确认 agent JSONL、summary、suite result 与 5 模型 × 8 case 入口。
- 通过逐项确认固定: 统计全部 agent `rdog control` 请求,禁止 app/case 特例,优先修共享层,全矩阵认证,fail-closed 计量和版本化 baseline。
- baseline 归档固定在外部评测仓库 `results/macos-ops-interaction/<baseline-id>/`,不依赖 `/tmp`。

### 总结感悟
- 最终 pass 只能证明任务完成,不能证明控制成本低。ledger 必须保留重试、协议错误和动作后证据,才能把通用兼容性优化与局部脚本技巧区分开。

## [2026-08-07 09:40:21] [Session ID: omx-1786061963768-e7in9l] 任务名称: macOS ops interaction ledger baseline 收口

### 任务内容
- 更新 `workflows/macos-ops-interaction-efficiency.md`,让计量规则与真实 Pi bash artifact 一致。
- 在外部评测仓库完成真实 5 模型 x 8 case 的 immutable interaction baseline 归档。

### 完成过程
- 新 ledger 定向回归 7 项和与既有 macOS ops runner 的联合回归 31 项均通过,`ruff check` 无问题。
- `macos-ops-20260807-live-5x8` 已保存 5 份 source suite、输入指纹、ledger、manifest 和摘要。
- 保留 1 个零成本失败 retry,没有修改 rdog 协议、canonical skill 或任何 app 特例。

### 总结感悟
- agent 决策和 rdog 请求是两个不同指标。将 shell 预备步骤直接拒绝会扭曲成本,而将其算作请求又会高估控制面交互。

## [2026-08-07 10:00:19] [Session ID: omx-1786061963768-e7in9l] 任务名称: macOS ops parser 兼容性候选 brief

### 任务内容
- 从 immutable interaction ledger 提取跨模型、跨 case 的共享 parser 摩擦,并在不改协议行为的前提下形成决策 brief。

### 完成过程
- 聚合 30 个 recovery 请求,确认 `@cmd` raw payload 和 `@window-find:APP` 都满足动态样本阈值并拥有明确共享 parser 入口。
- 将 `@key.target` 和 `@ax-press.action` 排除,因为它们分别会改变显式 targeted delivery 与 AX action 语义。
- 生成 external evaluation artifact brief,等待用户批准后才实施。

### 总结感悟
- 反复出现的 parse error 不必然可自动兼容。只有能保持既有 target、权限和 action 不变量的语法归一化才是合格优化。

## [2026-08-07 11:25:39] [Session ID: omx-1786061963768-e7in9l] 任务名称: macOS ops 共享 parser 兼容与 current-binary 认证

### 任务内容
- 实现批准的通用 parser 兼容: `@cmd` raw 单行 payload 与 `@window-find:APP`。
- 为外部 interaction archive 增加二进制 provenance 门禁,并以 current `target/debug/rdog` 完成 5 模型 x 8 case 认证。

### 完成过程
- Rust 格式化、685 项 binary nextest 和 binary build 已通过。评测 runner 以 `rdogBinary` 固定 runner 与 Pi bash 的同一可执行文件。
- archive 从 config 重新计算 `rdogBinary` path/SHA-256,要求每个 source `run-plan.rdog` 完全匹配;10 项 ledger 回归、27 项 runner 回归和 `ruff check runner` 全部通过。
- 归档 40/40 成功的 5 个 suite,得到 258 agent decisions、243 rdog requests、40 attempts。相对 immutable baseline 的 260 / 252 / 41,请求减少 9 次,满足已批准升级门槛。

### 总结感悟
- LLM eval 的版本号不足以证明 parser 改动被实际执行。只有 binary path 与内容 hash 同时封存,ledger 才能作为 shared protocol 优化的证据。
- 这轮没有向 skill 写入 App 或操作序列。改善来自共享 parser 兼容和可验证的评测路径,不是局部脚本化。

## [2026-08-07 12:54:47] [Session ID: omx-1786061963768-e7in9l] 任务名称: rdog parser 兼容性提交

### 任务内容
- 将本轮共享 parser 兼容、协议文档、canonical skill reference 和 workflow 固化为独立提交。

### 完成过程
- 已创建 `feat(control): accept raw cmd and positional window lookup` scoped commit。
- 提交只包含本轮审阅的 13 个文件,没有加入 App/case 特例操作序列。

### 总结感悟
- 评测改善应来自共享控制面兼容性,并通过全矩阵 artifact 验证,不能依赖脚本化的局部路径。

## [2026-08-07 16:39:13] [Session ID: omx-1786061963768-e7in9l] 任务名称: parser compatibility 独立重复采样

### 任务内容
- 对已认证的共享 parser compatibility 执行两次独立的 5 模型 x 8 case macOS ops matrix。
- 归档完整 source artifacts,用 existing binary provenance 门禁比较稳定性,不改 canonical skill、runner、case 或 app-specific 操作序列。

### 完成过程
- repeat A 与 repeat B 都完成 40/40,并通过 current binary path/SHA-256 校验。
- 外部评测仓库归档了两个 immutable ledger,另写 `repeat-sampling__20260807-parser-compatibility.md` 汇总历史 baseline、current reference、A/B 的计量与 provider 分布。
- B 的高成本集中在 MiniMax-M3 和 MiniMax-M2.7-highspeed;没有将可恢复错误或运行波动伪装成 parser 根因。

### 总结感悟
- 一次全矩阵请求数下降只能构成候选认证,不能单独证明稳定改善。重复采样必须保留 attempt、recovery 和 response error,否则最终成功会掩盖控制成本波动。
- 不稳定时的正确动作是停止扩展兼容语法,保留可审计样本,而不是追加 App 或 case 特例让某次评测更低。

## [2026-08-08 00:17:56] [Session ID: omx-1786061963768-e7in9l] 任务名称: repeat sampling 提交收口

### 任务内容
- 将两轮 immutable artifacts、比较报告和项目上下文提交并推送到各自远端分支。

### 完成过程
- root `e508132` 已推送到 `origin/restore-point-20260803-1300`。
- external `9a03d1b` 已推送到 `origin/master`;首次 SSH ref 更新被远端关闭,已上传的 84 个 LFS 对象被复用,第二次 push 成功。

### 总结感悟
- 大型 LFS 提交必须分别确认对象上传和 Git ref 更新;只有远端 ref 指向本地 HEAD 才算推送完成。

## [2026-08-08 09:12:00] [Session ID: omx-1786061963768-e7in9l] 任务名称: macOS ops ledger 前旧支线上下文归档

### 任务内容
- 整理 24 个已结束的后缀支线主题,共 81 个上下文文件。
- 保留当前默认六文件主线,同步归档 manifest、长期经验和 AGENTS 索引。

### 完成过程
- 按主题将文件移动到 `archive/branch_contexts/<topic>/`,不改正文和原文件名。
- 创建 `archive/manifests/ARCHIVE_MANIFEST__2026-08-08_macos_ops_interaction_ledger.md`,记录 81 条源路径到目标路径映射。
- 在 `EXPERIENCE.md` 记录 current reference `243`、repeat A `252`、repeat B `340` 的重复采样边界,以及 binary provenance 和 parser 语义安全门。
- 在 `AGENTS.md` 增加 manifest 和 macOS ops workflow 的检索入口。

### 总结感悟
- 40/40 成功不能替代交互成本和可恢复错误的独立统计。
- 旧上下文必须按主题整组归档,否则后续检索会把历史支线误当作当前任务状态。

## [2026-08-08 01:48:00] [Session ID: omx-1786061963768-e7in9l] 任务名称: macOS ops 通用 key/AX 契约候选

### 任务内容
- 在 canonical skill/reference 中明确 `@key` 合法字段、targeted delivery 和 AXPress/其它 AX action 的边界。
- 完成 5 x 8 live matrix 与 interaction ledger 认证,不使用 App/case 固定操作序列。

### 完成过程
- 只修改 `.codex/skills/rdog-control` 的三份通用文档,没有改变 parser、protocol runtime、runner 或 case。
- 最小 parser 回归 2 项通过;完整 matrix 为 40/40,并归档 5 份 source suite。
- candidate 总请求从输入兼容 reference 的 243 降到 209,但 9 个未变化 case 增长,因此按门禁拒绝 baseline promotion。

### 总结感悟
- 清零目标语法错误只能证明文案被本轮模型采用,不能证明整体收益稳定。
- 逐 case 门禁能阻止用少数大幅下降掩盖其它任务的控制难度回退。

## [2026-08-08 14:48:00] [Session ID: omx-1786112971218-cbf063] 任务名称: feature/observe-epoch-stale-reject 方案 1 落地

### 任务内容

- 在 `feature/observe-epoch-stale-reject` 分支实现 pi-computer-use 借鉴方案 1。
- 给 `@observe` 响应顶层暴露 `epoch` 字段 + `ComputerActRequest` 加 `epoch: Option<u64>`,
  在 implicit_observe / routing / dispatch 之前做 fast-reject, 阻止 stale write
  落到过期 observation 上。

### 完成过程

- 在 `src/control_observation/observe/response.rs` 响应顶层加 `epoch = primary_observation.created_at_unix_ms` (无 primary 时退化到 observed_at_unix_ms)。
- 在 `src/control_observation.rs` 加 `pub fn resolve_observation_header(observation_id)` 及 store::{resolve_header} 只解析 header 的 API, 不要求 ref_id。
- 在 `src/control_protocol.rs` 给 `ComputerActRequest` 加 `epoch: Option<u64>` 字段, 文档说明 epoch 与 observation_id 配对使用。
- 在 `src/control_protocol/parsers/computer_act.rs` 解析 `epoch` 字段, 对重复/负数/非整数报错, 与 `timeout_ms` 共用同一套风格。
- 在 `src/control_computer_act/error_envelope.rs` 加 `StaleObservationEpoch` error_code, retry.strategy = `re_observe_then_retry`, hint 引导重新 @observe。
- 在 `src/control_computer_act/mod.rs` 加 `check_observation_epoch_fast_reject` + `stale_observation_epoch_envelope`, 在 `execute_computer_act` 入口前验证; 不传 epoch 或 epoch 单独没 observation_id 时 no-op, 保持原行为。
- 在 `src/control_protocol/tests/computer_act.rs` 加 5 个 parser test。
- 在 `src/control_computer_act/tests.rs` 加 5 个 dispatch test (no_epoch / no_obs_id / match / mismatch / absent)。
- 在 `src/control_observation/observe_tests.rs` 加 2 个 response shape test。
- 在 `specs/rdog-computer-act-spec.md` 补 epoch 字段说明, 标注 feature/observe-epoch-stale-reject。

### 验证证据

- `RUSTFLAGS="-Awarnings" cargo check --quiet`: exit 0, 无 warning。
- `cargo nextest run -p rustdog --bin rdog -E 'test(/control_/)'`: 524 passed, 0 failed。
- `cargo nextest run -p rustdog --bin rdog -E 'not test(/control_/)'`: 173 passed, 0 failed。
- 新增 12 个测试全部 pass: parse 5 + dispatch 5 + response 2。

### 总结感悟

- "向后兼容加成" 的实现路径比较安全: 不传 epoch 走原路径, 传了 epoch 才 fast-reject, 没有改变任何现有 caller 行为。
- 把 epoch 字段挂在响应顶层而不是 `observation.observation_id` 嵌套对象里, 客户端一次 JSON 路径就能取到, 大幅降低集成成本。
- `check_observation_epoch_fast_reject` 作为独立 pub(crate) 函数暴露, 方便单测覆盖各分支 (no_epoch / no_obs_id / match / mismatch / absent), 不会污染 dispatch 主流程。
- epoch 用 `created_at_unix_ms` 而不是新建 monotonic counter, 最小可行, 未来切 per-resource epoch 时只换 `resolve_observation_header` 实现, 上层 API 不变。

## [2026-08-08 17:45:00] [Session ID: omx-1786201921174-cvveb1] 任务名称: feature/computer-act-outcome-3state 方案 2 落地

### 任务内容

- 在 `feature/computer-act-outcome-3state` 分支把 Phase F-2 的
  `ok:false + error_code:verify_failed` 改写路径替换成 outcome 三态
  (`worked` / `didnt` / `unknown`).
- 借鉴 pi-computer-use `ActOutcome`, 让 dispatch 成功 vs postcondition 满足
  两个概念彻底分开, 消除 false success.

### 完成过程

- 上一 session (omx-1786112971218-cbf063) 在 outcome 三态方案下:
  - 新建 `src/control_computer_act/outcome.rs` (174 行): `ComputerActOutcome` enum
    + `compute_outcome` (4 维决策表) + `render_outcome` (serde snake_case wire) + 7 个单测.
  - 改 `src/control_computer_act/mod.rs`: 删 83 行 Phase F-2 verify_failed envelope rewrite,
    加 43 行 outcome 三态计算 + 写入顶层 `outcome` 字段.
  - 改 `src/control_computer_act/verify.rs`: 加 `verification_status_for_diff` 函数
    + 在 `render_verification` BestEffort 分支写 `verification.status` 字段
    (verified / preexisting / failed, 与 pi-computer-use 同构).
- 本 session (omx-1786201921174-cvveb1) 继续:
  - 清理 `src/control_computer_act/error_envelope.rs` 中 dead code:
    删 `verify_failed_envelope_json()` helper (0 caller, -23 行)
    + 2 个 helper 集成测试 (-46 行), 保留 `ComputerActErrorCode::VerifyFailed`
    enum variant (供 retry_strategy reference).
  - 加 `src/control_computer_act/verify.rs` 3 个 status 单测
    (verified / preexisting / failed), 锁定三档 wire 字符串.
  - 更新 `scripts/smoke_computer_act_verify.sh` test 3 期望: 删 retry.strategy /
    retry.hint 断言, 改成 `ok:true` + `outcome:"didnt"` + `verification.status:"failed"`.
  - 更新 `scripts/smoke_computer_act_trace.sh` test 3 期望: 同样改 outcome 三态.

### 验证证据

- `RUSTFLAGS="-Awarnings" cargo check --quiet`: exit 0, 无 warning.
- `cargo nextest run -p rustdog --bin rdog`: **705 passed, 0 failed, 1 skipped**
  (原 baseline 698, +7 from outcome.rs + +0 from 决定不下冗余集成测试).
- 新增 10 个测试全部 pass:
  - outcome.rs 7 个 (decision table + wire strings + render value)
  - verify.rs 3 个 (verification_status_for_diff verified/preexisting/failed)
- 删除 3 个 dead 测试 (verify_failed_envelope_json_matches_e2_shape /
  verify_failed_envelope_json_without_ax_diff_still_emits_action
  + helper 函数 verify_failed_envelope_json).

### 总结感悟

- "Phase F-2 verify_failed envelope rewrite" 这块原本是修 false success 的过度设计
  (用 ok:false 表达 postcondition 失败混淆了 dispatch 与 postcondition).
  outcome 三态是正确的语义: `ok` 只表示 dispatch, `outcome` 表示 postcondition,
  客户端可以各自独立 retry 决策.
- pi-computer-use `ActOutcome` 已经沉淀了这个教训; rustdog 同步这套语义让跨产品
  integration 更顺 (compatible with pi).
- "verification.status" 字段同时在 best_effort (ax_diff) 和 always (full_observe) 走
  `verification_status_for_diff` 决策, single source of truth, 避免 caller 各自判断.
- "删 dead code" 比 "加新字段" 更该优先做. error_envelope.rs 的 helper + 2 测试
  没有 caller 了就该删. 减少 ~69 行, 跟 outcome 三态的 +174 行配合, 净改动仍然很轻.
- smoke 脚本的期望更新是必做的: Phase F-2 的 `ok:false + error_code:verify_failed`
  已经变成 `ok:true + outcome:"didnt"`, 不更新的话 smoke 跑就 false fail.

### 后续建议

- 不要把 outcome 三态推广到 @ax-press / @click / @key 等 mutating 命令, 路径上
  没有 outcome 概念. 只在 @computer-act (高层 action 封装) 这层做 postcondition 判断.
- smoke 跑完后 (下一轮 macOS live smoke) 验证 outcome 字段是否真被 client 读到;
  如果读到, 这一轮方案 2 就 close; 如果没读到, 进 LATER_PLANS 考虑删 outcome 字段
  (跟 epoch 验证逻辑类似).
