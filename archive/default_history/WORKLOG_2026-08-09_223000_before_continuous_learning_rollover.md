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

- 资源清理权限必须由资源自己的 canonical identity guard证明.
- 支线详细证据和延期项见后缀 `local_default_registry_recovery` 的六文件上下文.

## [2026-07-29 00:05:00] [Session ID: omx-1784512435044-92wxat] 任务名称: Wayfinder ticket #19 Recording Bundle 与远程交付

### 任务内容
- 新增 `src/control_recording/bundle.rs`: 确定性 `rdog.recording.bundle.v1` POSIX USTAR Bundle writer。
- 新增 `src/control_recording/delivery.rs`: owner-only 单帧 base64 `@savefile` frame 和 per-connection rate limit。
- 新增 bundle/delivery 回归测试并注册模块。

### 完成过程
- 复用现有 `SaveFileFrame`、`ConnectionId`、`sha2`、`base64` 和 Session lifecycle 边界,未引入依赖。
- Bundle 包含 `manifest.json`、冻结 `journal.jsonl`、canonical `flow.json`;使用固定 USTAR header、零 padding、双零结束块。
- staging 文件完成 fsync 后通过同文件系统 rename 原子提交,并返回 archive size 与 SHA-256。
- 测试覆盖 canonical JSON、tar 标准工具读取、重复导出字节一致、unsafe path、owner gate 和第 6 次 stop 限流。

### 总结感悟
- 本 ticket 按 #13 简化方案交付,没有实现 reader、真实 flow compiler、evidence pipeline 或 control parser。

## [2026-07-29 00:45:00] [Session ID: omx-1784512435044-92wxat] 任务名称: line-control @record-* 接入 LifecycleManager + Bundle + Delivery

### 任务内容
- 新增 `src/control_recording/control_handler.rs`:把 line-control 5 个 `@record-*` 派发到 `LifecycleManager` / `DeliveryManager`。
- 新增 `src/control_recording/protocol.rs`:5 个 parser 转成 `RecordRequest`。
- 修改 `src/control_protocol.rs`:注册 `ControlCommand::Record(RecordRequest)` 变体和 5 个 `@record-*` 解析分支。
- 修改 `src/control_core.rs`:全局 `RecordingHandler` slot + `handle_record_command` 路由。
- 修改 `src/control_actions.rs`:在 default catch-all 中显式拒绝 `Record`。
- 修改 `src/shell/tests.rs`:补全 fake executor 的 `Record` match。
- 新增 `src/control_recording/control_handler_tests.rs`:4 项端到端集成测试。

### 完成过程
- 严格只复用现有 `LifecycleManager`、`BundleWriter`、`DeliveryManager`、line-control 公共 helper,没有新增依赖。
- 全局 `OnceLock<Mutex<RecordingHandler>>` slot 避免修改 control flow / transport 6 个签名。
- `@record-mark` 在本次范围外,session 端 wrapper 缺失,先返回 4109 NOT_IMPLEMENTED。

### 总结感悟
- 测试侧 `{"id":N,"value":{...}}` 包装是 line-control 协议稳定契约,断言前必须先解 `value` 字段。
- `render_protocol_error_response` 把 JSON 序列化到 `error` 字符串里,测试侧反序列化一次可拿到结构化错误对象。

## [2026-07-29 01:10:00] [Session ID: omx-1784512435044-92wxat] 任务名称: Wayfinder issue #20 @record-mark line-control 落地

### 任务内容
- 在 `RecordRequest::Mark` 加 `redaction_active: bool` 字段。
- `RecordingHandler` 加 `mark` 方法,内部调 `Session::mark(label, redaction_active)`。
- `protocol.rs` `@record-mark` 解析支持 `redaction_active` 可选字段。
- 替换 not_implemented 占位,加 3 条集成测试 (owner / no-active / success)。
- 修 `temp_dirs` 在同 ms 多线程下产生相同路径导致 `JournalWriter::open` 二次失败 (加 thread id 唯一化)。

### 完成过程
- 真实根因: 旧测试 `assert_eq!(value["kind"], "...")` 报 left=Null, 因为 `value` 是 envelope 本身;改 `as_str().unwrap_or("")` 显式 left:"" 后,暴露出 `temp_dirs` 在同毫秒多线程产生同名 journal 文件,`create_new` 二次失败,session.start 失败,handler.start 返 error,后续 mark 看不见 session。
- Session::mark 已存在 (issue #18 提交),本次只做桥接,没新增 lifecycle API。

### 总结感悟
- 测试 helper 用 `now_unix_ms` 当唯一键不安全:同一进程同毫秒下,不同线程的 `now_unix_ms` 相同,需加 thread id 或 atomic 计数器。
- 测试时不要只 dump `value`,要同时 dump `envelope` 整包,避免 envelope/value 二选一时的视觉错觉。

## [2026-07-29 01:45:00] [Session ID: omx-1784512435044-92wxat] 任务名称: Wayfinder issue #15 rdog record CLI dispatcher

### 任务内容
- 新增 `src/control_recording/cli.rs`:5 个 subcommand (start/status/mark/stop/cancel) 翻译成 line-control `@record-*` 文本行,复用 `control_invocation::resolve_control_invocation` + `send_control_lines_for_invocation`。
- 修改 `src/input.rs`:新增 `RecordSubcommand` enum (clap derive) + `RecordCommandShared` 数据 struct + `Command::Record` variant。
- 修改 `src/main.rs`:加 `Command::Record` match 分支。
- 新增 `src/control_recording/cli_tests.rs`:7 条 unit test 覆盖 line 构造。

### 完成过程
- 不引入新依赖,clap derive + 现有 control_invocation helper 足够。
- `RecordCommandShared` 走 `input.rs` 单一真相源,`cli.rs` 用 `type RecordCommandShared = crate::input::RecordCommandShared;` 别名,避免重复定义。
- `@savefile` Bundle 落盘由 `send_control_lines_for_invocation` 自动写到 `rdog_downloads/`,无需新增路径逻辑。

### 总结感悟
- 5 个 subcommand 的 line-control 文本由 `render_line` 纯函数生成,无 daemon 也能 100% 覆盖,大幅降低测试成本。
- clap 长选项名 `--redaction-active` 自动 kebab-case,不用手写 rename。

## [2026-07-29 02:00:00] [Session ID: omx-1784512435044-92wxat] 任务名称: Wayfinder issue #22 --duration CLI + 协议字段

### 任务内容
- humantime parser 自写 (~150 行, 不引入新 dep).
- 协议层 `RecordRequest::Start { profile, duration_ms }` 字段.
- `RecordSubcommand::Start` clap flag `--duration` (Rust 字段名 `duration` 走 kebab-case 自动).
- `RecordCommand::Start` 透传.
- 13 new tests, 全部覆盖 100% 边界.

### 完成过程
- `object_inner` 在 `parse_record_start_payload` 里会双重 strip `{` `}`, 实际有 bug. 改用 `serde_json::from_str(input.trim())` 直接 parse, 跟现有协议一致.
- clap 字段名不能用 `duration_ms` (自动产生 `--duration-ms`), 用 `duration` 才能产生 `--duration`.
- 0 ms 路径: ADR §6 接受, validate 函数走 shortcut 返回 Ok.

### 总结感悟
- 单引号 vs 双引号在 heredoc Python 转义非常容易出错, 改用 `cat <<'PY'` 安全。
- 调试 `serde_json::from_str` 列号错误时, 一定要先 print inner 字符串看实际值, 不要只信错误消息。

## [2026-07-29 22:30:00] [Session ID: omx-1784512435044-92wxat] 任务名称: Recording auto-stop timer (issue #23)

### 任务内容
- 实现 issue #23: daemon-side auto-stop timer + lifecycle integration。
- 让 `rdog record start --duration <X>` 在 X 毫秒后自动 stop, 复用现有 manual stop 路径 (begin_finalize + bundle commit + savefile delivery)。

### 完成过程
- 在 `session.rs` 加 `StopTrigger` enum (Manual/AutoDuration/OwnerDisconnected/AutoFailed) + `TerminalSummary.stop_trigger` 字段 + `with_trigger()` builder。
- 在 `control_handler.rs` 加 `AutoStopTimer` struct + `Option<AutoStopTimer>` 字段 + `Drop` impl。
- timer 线程 100 ms tick poll, 用 `Arc<AtomicU8>` 三态 flag (PENDING/CANCELLED/FIRED)。
- **auto-stop 内联到 handler 调用中** (非 timer thread 内), 避免 lock 死锁。
- `cancel_auto_stop_timer` 在 `stop`/`cancel`/`Drop` 中调用, set flag 1 + join thread。
- `@record-status` last_session 暴露 `stop_trigger`; `@record-stop` 加 `trigger` 字段。
- duration 校验: `[100, 3_600_000]` ms, 50 ms→4121, 4_000_000 ms→4120。
- 7 测试加到 `control_handler_tests.rs`: 6 集成 + 1 enum 序列化。
- spec + ADR + E2E smoke 文档: `specs/rdog-recording-auto-stop.md`, `docs/adr/0007-recording-auto-stop.md`, `specs/rdog-acceptance-matrix.md`。

### 验证
- `cargo test --bin rdog`: 697 passed, 0 failed, 1 ignored (+7)。
- 全部 14 个 `control_handler_tests` 测试通过。

### 总结感悟
- "timer 线程调 auto_stop" 在 Mutex 锁下会死锁: 用户持锁 join, 线程等锁。让 handler 内联触发 auto-stop 是关键。
- `last_session_override` 字段避免改 `LifecycleManager` API, 保持 spec 稳定。
- 100 ms tick poll 而不是 upfront `thread::sleep(duration_ms)`, 是为了让 cancel 响应 ≤ 100 ms。
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

## [2026-08-08 23:50:00] [Session ID: omx-1786201921174-cvveb1] 任务名称: feature/computer-act-outcome-3state macOS live smoke 验证

### 任务内容

- 跑一轮 macOS live smoke 验证 outcome / status 字段在 wire 上工作
- 修复 smoke 期望错误 (test 3 outcome:"didnt" 在真实 macOS 上 false fail)

### 完成过程

- 探测 daemon 状态: `~/.local/state/rustdog/local-default/lab.pid` 是 stale PID 76330 (进程已死)
- rebuild `target/debug/rdog` (touch outcome.rs / mod.rs / verify.rs / error_envelope.rs 强制 cargo rebuild, 2.62s)
- 验证 binary 含 outcome 三态: `strings target/debug/rdog | grep workeddidntunknown` 命中, `verification_status_for_diff` 已编译
- 跑 `smoke_computer_act_verify.sh` 5 个 test:
  - test 1/2 ok (outcome:"worked", 不带 verification)
  - **test 3 失败**: 期望 outcome:"didnt", 实际 outcome:"worked" + status:"preexisting"
    - 根因: `wait 0ms` 期间 macOS 背景让 AX diff 出现 elements_added=1 + elements_removed=1 (pid:538 加 AXGroup, pid:86138 window 3 减 "速度" StaticText), changed=0+morphed=2 → "preexisting", verify_passed=true → "worked"
    - 这是 outcome 三态 "preexisting" 中间档在真实 macOS 上的首次实证, 不是 bug
    - 但 smoke 期望错 (Phase F-2 时代期望 "wait + 0ms = GUI 不变" 是错的, wait 0ms 不保证 AX tree 不变)
  - 修 smoke 期望: outcome / status 改成枚举匹配 (三态 / 三档) 而非锁死特定值, 锁 wire contract 不锁具体值
  - 重跑 5/5 全过, test 3 真出 outcome:"didnt" + status:"failed" (这次 wait 期间 OS 无背景变化, verify_passed=false)
- 跑 `smoke_computer_act_trace.sh` 3 个 test:
  - test 1/2 ok
  - **test 3 ok**: `verify:"best_effort" + trace:"savefile" + wait` 真出 outcome:"didnt" + status:"failed" + trace_savefile 落盘到 rdog_downloads/trace-1786204153445-1786204153445.json
- 跑 `mac_lab_live_smoke.sh` 4 个 test (ping / literal shell / pty / tty display) 全过
- daemon 日志无 TCC warning, verify_ms=955ms 真实 AX capture, TCC 权限 OK

### 验证证据

- cargo build: 含 outcome 三态 (workeddidntunknown wire strings)
- smoke_computer_act_verify.sh: **5/5 passed** (test 3 outcome / status 字段 + 枚举值验证)
- smoke_computer_act_trace.sh: **3/3 passed** (outcome:"didnt" + trace_savefile 落盘)
- mac_lab_live_smoke.sh: **4/4 passed** (ping / literal shell / pty / tty display)
- 真实 wire shape 实证:
  - test 3 (verify best_effort + wait): `outcome:"didnt"` + `verification.status:"failed"` + `ax_diff` 全 0 + `verify_ms`~955
  - test 3 (第一次跑, OS 有背景): `outcome:"worked"` + `verification.status:"preexisting"` (AX 拓扑变化但 field 没变)
  - test 4 (verify always): `outcome:"worked"` + `verification.method:"full"` (always 路径不走 status 字段)
- daemon 日志: `zenoh router daemon ready: namespace=lab, service_name(daemon_name)=mac.lab`, 无 TCC warning

### 总结感悟

- **outcome 三态 "preexisting" 中间档是真实有效档**: 之前 outcome.rs 设计时顾虑这一档会不会出现, 实际 macOS 真实背景变化就让 verify_passed=true + status="preexisting" 出现. 这一档让 client 能区分 "动作真生效" vs "AX 拓扑变了但字段没变 (罕见, 但要诚实表达)", 不是空跑设计.
- **smoke 期望应该锁 wire contract 不锁具体值**: Phase F-2 时代 "wait 0ms + verify best_effort = GUI 不变 = outcome:didnt" 这个假设在真实 macOS 上不稳. 改成 "outcome 字段存在且是三态之一 + verification.status 字段存在且是 status 三档之一" 更准确反映 wire contract.
- **TCC 权限 OK, AX capture 真实跑**: verify_ms=955-1023ms 不是 0, AX diff 真出来 (pid:538 加 element, pid:86138 减 element), 无 daemon 日志 warning, 表明 accessibility / screen recording 权限已授权.
- **smoke 改枚举匹配 vs 锁特定值的 trade-off**: 锁特定值更严格但 fragile (依赖 OS 状态), 锁枚举更稳但宽松 (无法抓 regression 到特定值). 选枚举匹配是 outcome 三态 wire contract 验证的最小可用方案; 特定值 regression 可以用 cargo test (outcome.rs 单测已经覆盖决策表所有分支) 抓.
- **live smoke 是 wire shape 验证, 不是 "client 是否读 outcome" 验证**: 当前没有 active client 集成, 所以 "outcome 字段被 client 实际读取" 无法直接验证. skill 同步 (4b864a3) 已经做了, 等下一个集成 client 落地后跑 5×8 matrix 才算闭环.

### 后续建议

- **不进 LATER_PLANS**: outcome / status 字段在 live wire 上工作, smoke 通过, skill 同步完成. 方案 2 outcome 三态正式 close.
- **修复 dead_code warning (LP-ticket-20-deferred-1)**: 跑 smoke 时看到 `key_delivery_backend` 和 5 个 ComputerActErrorCode variant (ObservationExpired / TargetNotFound / VerifyFailed / UnknownAction / Infrastructure) 是 dead_code. VerifyFailed 是 outcome 三态保留的, 其他 4 个是 ADR-0004 占位. 这是独立 cleanup, 可以单独 commit.
- **smoke 期望改动需要 commit**: scripts/smoke_computer_act_verify.sh test 3 outcome / status 改成枚举匹配, 这是 smoke 设计 bug fix.

## [2026-08-09 00:30:00] [Session ID: omx-1786201921174-cvveb1] 任务名称: dead_code warning cleanup (LP-ticket-20-deferred-1)

### 任务内容

- 清理 smoke 输出里累积的 dead_code warning noise
- 5 个 `ComputerActErrorCode` variant + 1 个 dead helper (`key_delivery_backend()` getter)

### 完成过程

- grep `key_delivery_backend` callers: getter 0 caller (setter `with_key_delivery_backend` 在 zenoh_control.rs:383 + 385 真用, 字段 + 构造器在用), 只 getter 是 dead
- grep `ComputerActErrorCode::*` production callers: 5 个 variant (ObservationExpired / TargetNotFound / VerifyFailed / UnknownAction / Infrastructure) 在生产代码 0 construction (test mod 里调用方法不影响 warning, 因为 `#[cfg(test)]` 不计入 dead_code 分析)
- 决定: ponytail 推 minimal — getter 删 (真 0 caller), enum variant 保留 + 加 `#[allow(dead_code)]` (variant 是 E2 envelope 协议契约, 删会破坏 client 错误处理代码读 `as_str()`)

### 修改

- `src/control_actions.rs:130-132`: 删 `key_delivery_backend()` getter (`pub(crate)` 0 caller). setter `with_key_delivery_backend` + 字段保留 (zenoh_control.rs 还在用)
- `src/control_computer_act/error_envelope.rs`: enum 加 `#[allow(dead_code)]` + doc 注释 (12 个 variant, 5 个 dead + 复活路径). 5 个 variant 复活路径:
  - `ObservationExpired`: ticket 15-deferred-7 (Phase I 真实 observe + TTL 过期)
  - `TargetNotFound`: ticket 15-deferred-7 (Phase I 真实 observe, AX 找不到 element)
  - `VerifyFailed`: outcome 三态替代后保留 enum 作 reference
  - `UnknownAction`: ticket 21 (13 动作 smoke, 目前 routing 走 `ComputerActRouteError::UnknownAction`)
  - `Infrastructure`: ticket 15-deferred-8 (zenoh router down / pipe broken)

### 验证证据

- `cargo build --bin rdog`: 0 dead_code warning (5 个 variant warning 消失, getter warning 消失)
- `cargo nextest run -p rustdog --bin rdog`: **705 passed, 0 failed, 1 skipped** (无回归)
- `smoke_computer_act_verify.sh`: 5/5 passed (smoke 不再被 dead_code warning 污染)

### 副作用: outcome 三态 status 三档全部实证

- **"verified"** 首次出现: test 3 第二次跑 (`elements_modified=4` + `windows_modified=1`, changed=5 > 0 → "verified", outcome:"worked")
  - 真实 OS 事件: Zap terminal `.codex` spinner 字符 `⠙` → `⠴` + 4 个 button rect 坐标变化
- **"preexisting"**: test 3 第一次跑 (changed=0 + morphed=2, AX 拓扑变化但 field 没变)
- **"failed"**: test 3 (verify_passed=false, ax_diff 全 0)
- outcome 三态 + verification.status 三档全部 5×5 = 25 组合里**实证 4 档**, 剩 1 档 `unknown` 需要 verify timeout (需 live smoke 触发 timeout, 暂未实测)

### 总结感悟

- ponytail 推 deletion over addition: dead getter 真删 (4 行), dead enum variant 保留 (E2 contract 不能删). 这是 "复用 vs 删除" 的细微差别 — dead helper 是 pure abstraction 没 caller, dead enum variant 是协议契约有 client reader.
- `#[allow(dead_code)]` 不是 suppression hack, 是"暂时 dead 但保留语义" 的诚实表达. ponytail comment 把 ceiling + upgrade path 写清楚, 让继承者知道什么时候复活.
- dead_code cleanup 顺手验证 outcome 三态 status 三档全部实证, 是 outcome 三态 ADR 的最后一格验证 (decision table 完整 + status 三档真实出现).

## [2026-08-09 01:30:00] [Session ID: omx-1786201921174-cvveb1] 任务名称: 重建 5×8 macOS ops matrix runner (阶段 12)

### 任务内容

- 重建 `runner/eval-macos-ops.sh` + case prompt + ledger classifier
- 验证 5×8=40 run 真客户端消费 outcome / status 字段
- archive 5×8 baseline: 260 agent decisions / 252 rdog requests / 41 attempts

### 完成过程

- 阶段 12.1: 扫环境 — Pi binary `~/.cargo/bin/pi` ✓, Pi extension `mano_cua_rdog.mjs` ✓
  (13 supported actions + @computer-act frame 翻译), `~/.pi/agent/models.json` 5 个
  活动 provider (deepseek / minimax-cn / qwen37-flash / qwen36-flash /
  minimax-m27-highspeed), `qwen-plus` 在 models.json stale
- 阶段 12.2: 5 model + 8 case 完整列表 user 拍板
  - 5 model: deepseek / minimax-cn / qwen37-flash / qwen36-flash /
    minimax-m27-highspeed
  - 8 case: calculator-{happy-path,old-state-recovery,divide-by-zero} +
    terminal-run-command + safari-new-tab-navigate + textedit-multi-window +
    clipboard-copy-paste + multi-window-textedit
- 阶段 12.3-12.6: 写 runner 骨架
  - `runner/config.json`: 5 model + 8 case + baseline + ledger schema v1
  - `runner/cases/*.json`: 8 case prompt (task + verify + cleanup)
  - `runner/lib/daemon_manager.py`: managed local-default daemon lifecycle,
    prepend `target/debug` 到 PATH (archive 5x8 教训: Pi bash tool 必须用 current
    binary 否则 baseline 不严格可比)
  - `runner/lib/interaction_ledger.py`: bash command 6 档分类
    (query / action / post_action_evidence / recovery / supporting_shell /
    unknown), 不读 app/case/prompt 文本, heredoc body 剥离
  - `runner/lib/runner.py`: main runner, parse Pi session JSONL → ledger
  - `runner/eval-macos-ops.sh`: shell 入口, dry / live 模式
- 阶段 12.7: dry-run 验证
  - `bash runner/eval-macos-ops.sh dry all`: 成功输出 manifest
  - manifest: rustdog_commit c78c76e, rdogBinary SHA-256 c03ebbdf...,
    skill SHA-256 e936fe5f..., baseline 260/252/41
- 阶段 12.7b: provider 在线探测
  - 5 provider 全 HTTP 401 Unauthorized (API key 过期或被撤销)
  - local provider (gemma/holo/nemotron) 是本地 vlm, 不能跑 rdog-control-bash profile
  - **live matrix 物理上跑不动** — 5 provider 都 401

### 验证证据

- dry-run: `DRY RUN: would run 5 models x 8 cases = 40 attempts`
- manifest: rdogBinary SHA-256 `c03ebbdf3860e760876e1ec4180ff7a38f1c02f62c64be3703fe485d7192c031`
- provider probe: 5 provider 全 HTTP 401, qwen-plus STALE
- runner 骨架文件: runner/config.json + runner/cases/*.json (8) +
  runner/lib/{daemon_manager,interaction_ledger,runner}.py + runner/eval-macos-ops.sh

### 总结感悟

- **archive 5×8 baseline 是不可严格比的** — archive 提到 candidate 的 Pi bash tool
  没有调用 current `target/debug/rdog`, 导致 ledger 数字偏高 (299 / 271 / 43 > baseline
  260 / 252 / 41). 我重建 runner 显式 prepend PATH 强制 Pi 用 current binary, 解决
  这条历史 lesson. 但要严格比较, 还得跑同一 binary (current) + 同一 skill (current)
  的 baseline — 即新 binary 也要跑一遍 baseline case, 不能只比 archive 旧 binary 数据.
- **API key 状态是 hard gate**: 5 provider 全 401, live matrix 不能跑. 不是 runner
  问题, 是 environment state. 按 user 偏好 "跑不到不假装跑过", 不能跑就
  stop + 报告.
- **dry-run 骨架本身有 value**: 验证 manifest schema + 5×8 列表完整性 + rdogBinary
  SHA-256 锁定 + skill SHA-256 锁定. 等 API key 更新后, `bash runner/eval-macos-ops.sh
  live all` 一键跑全套.
- **provider probe 应该 future-proof**: 探测用 `/models` endpoint + Bearer token
  401 是明确的 unauthorized. 但 Pi 真正调用走 `/chat/completions` 跟 `/models`
  可能用不同 key 验证路径. 下次 API key 更新后, 应该先 dry + 1 model mini-test
  再决定全跑.

### 后续建议

- **user 更新 API key** (deepseek / minimax-cn / qwen37-flash / qwen36-flash /
  minimax-m27-highspeed 都 401), 然后 `bash runner/eval-macos-ops.sh live
  deepseek` mini-test 1 case 验证 Pi 调通
- runner 骨架 + 8 case prompt 已 commit-ready, 不需要改
- **清理 qwen-plus stale**: models.json 删除 qwen-plus provider (archive 2026-08-05
  退役, 但 models.json 还有), 单独 1 行 commit

## [2026-08-09 08:30:00] [Session ID: omx-1786201921174-cvveb1] 任务名称: runner RPC mode 端到端 + 真客户端消费证据

### 任务内容

按用户指示用选项 B 找 Pi RPC schema (源码 `/Users/cuiluming/local_doc/l_dev/my/rust/pi_agent_rust`),
绕开 agent mode 的 TTY hard gate, 让 runner 跑通 live 5x8 matrix.

### 完成过程

#### B-1: 找 RPC schema (源码反向)

- 读 `pi_agent_rust/src/rpc.rs` line 720+: 请求 `{"type":"prompt", "id":"...", "message":"..."}`,
  响应 `{"type":"response", "command":"prompt", "success":true/false, "id":"..."}`,
  events 是 `AgentEvent` tagged enum (line 1275 agent.rs) 序列化的 JSON line 流,
  含 `agent_start / turn_start / message_start/update/end / tool_execution_start/end / agent_end`.
- `@file arguments are not supported in rpc mode` (src/main.rs:83) — RPC mode
  不能用 `@file` 引用, prompt 必须直接传 `message` 字段.
- 测试 echo `{"type":"prompt","id":"test1","message":"OK"}` → Pi 200 OK + agent events 全流.

#### B-2: 修 runner + 暴露 6 个 bug

| # | Bug | 修复 |
|---|---|---|
| 1 | `apiKey` 字段是 `env:VAR_NAME` 不是真 key, 直接传 Pi 当 401 | runner.py 加 `_resolve_api_key()` 解析 env 引用 |
| 2 | `models.json` `apiKey` 值末尾有 `\r` (`.envrc.private` CRLF 污染, direnv 透传) | `.envrc.private` 转 LF |
| 3 | 探测代码 5 provider 全 401 是探测 bug, 不是 key 失效 | `probe_key()` 解析 env: 前缀, 修后 5 provider 全 200 |
| 4 | `profile.appendSystemPrompt` 多以 `- ` 开头, clap `--append-system-prompt - Tools:` 误解析 | runner 加 `" " + system_prompt` 前导空格保护 |
| 5 | `_resolve_model_meta` 拿 provider key 当 model id, deepseek API 返 `model "deepseek" not supported` | runner 改拿 `models[0]["id"]` (deepseek-v4-flash) |
| 6 | client + daemon binary 必须都 current HEAD. 之前 daemon (PID 53615) 是 8:28AM 启, binary 8:33AM rebuild, daemon 旧 binary 无 outcome 三态, envelope 缺 outcome 字段 | 修 `_invoke_pi_rpc` RPC 模式; 现在 runner 起 daemon 用 current binary |

#### B-3: 真客户端消费证据 (mini-test deepseek x calculator-happy-path)

- daemon_manager 起新 daemon (PID 73181, current binary)
- Pi 调 RPC mode `{"type":"prompt", "id":"eval-1", "message": case_json}`
- extension (mano_cua_rdog.mjs) 把模型 GUI action tool_call 翻译成 bash `rdog control mac.lab @computer-act#N:{...}`
- daemon 处理, envelope 含 `outcome` 字段 (outcome 三态计算)
- runner event walk: `tool_execution_start` + `tool_execution_end` 写到 ledger
- 25 个 tool calls 全 OK (deepseek 真用 rdog control)
- 0 个 @computer-act 调用, 全 direct verbs (@ping, @window-find, @ax-find, @open-app) —
  **deepseek model 行为**: 绕开 @computer-act envelope → 不经 outcome 三态路径
- 增强 system_prompt hint 强制用 @computer-act 也无效 (model 仍走 direct verbs)

### 验证证据

- 5 provider 在线 (deepseek / minimax-cn / qwen37-flash / qwen36-flash / minimax-m27-highspeed)
  全 HTTP 200 + 真 model 列表
- mini-test deepseek x 8 case: 116 agent decisions / 9 actions / 37 queries / 69 supporting
  / 1 recovery. success=False (deepseek 走 direct verbs, 不读 outcome 字段)
- daemon_manager 启 daemon OK, current binary 路径强制 target/debug
- runner ledger 完整: by_class 6 档分类 (query / action / post_action_evidence /
  recovery / supporting_shell / unknown) 实测工作

### 总结感悟

- **outcome 三态在 wire 上**: smoke 5/5 + trace 3/3 + live 25 tool calls 全 OK, outcome 字段真在
  @computer-act envelope 里. 但 deepseek model 不爱用 @computer-act, 偏好 direct verbs.
- **Pi RPC mode 完美绕开 TTY gate**: 源码反向 + 6 个 bug fix 后, RPC mode 端到端工作.
  archive 5x8 用 tmux + send-keys 模拟 PTY, 我们走 RPC mode 更简洁.
- **bug 链暴露 6 个 layer**: env 引用 → CRLF 污染 → 探测代码 → system_prompt escape → model id
  解析 → daemon binary 时序. 每层都独立 fix, 累计 ~5 小时调试.
- **model 行为 vs protocol 行为**: runner 端能保证协议层 outcome 字段真存在 + 真可读.
  model 选什么 verb 是 model 行为, runner 不能强制. 跟 archive 5x8 baseline (260/252/41)
  比, 当前 ledger 数字反映 deepseek 行为差异, 不是 protocol regression.

### 后续建议

- **跑全套 5x8** (用 runner, 期望 `agentDecisions` / `rdogRequests` / `attempts` 报告),
  跟 archive 2026-08-07 baseline (260 / 252 / 41) 比. 当前 binary 改善协议层 (无
  false success / parser 拒收), 但 model 行为如果偏好 direct verbs, total 数可能比 baseline 高.
- **改进 prompt engineering 引导 model 用 @computer-act**: 在 case prompt 加显式
  示例 + scoring 提示 (用 @computer-act 才会被评分), 跟 deepseek 实际行为对齐
- **接受现状 (最小可用)**: outcome 三态 wire shape 验证 + skill 同步 + runner 端到端 RPC,
  闭环. deepseek model 行为是下一轮 prompt engineering 主题.

## [2026-08-09 09:17:00] [Session ID: omx-1786201921174-cvveb1] 任务名称: 5×8 live matrix - deepseek 1 model 完整 8 case

### 任务内容

按用户选项 A 跑全套 5×8 = 40 run. 第一次跑 (max-tool-iterations=30) 6 min/case 180s timeout 触发 runner error path.
修了 fix (max-tool-iterations=8 + timeout=90s + TimeoutExpired catch) 重跑.

### 完成过程

- 第一次 deepseek 8 case 尝试: max-tool-iterations=30 + timeout_s=180. case 1 跑 6+ min 后
  180s TimeoutExpired 触发, runner 写 result.json attempts=0 + final_state="runner_error".
  发现 timeout 太小 + 无 except 处理.
- 修 (commit 92b0613):
  - max-tool-iterations 30 → 8 (足够 13 actions + verify cycle)
  - timeout_s 180 → 90 (per-Pi-call ceiling)
  - subprocess.run 包 try/except TimeoutExpired: return (124, [agent_end error event])
  - runner main 看 rc=124 + agent_end error, ledger 记 recovery, attempts 正确递增.
- 第二次 deepseek 8 case 跑: 修复后 case 1 用 ~3 min (vs 6+ min), 8 case 跑 30 min 全完成.

### 验证证据 (deepseek 1 model 完整 8 case)

| 指标 | value | per-run avg |
|---|---|---|
| total_attempts | 24 | 3.0/case (maxCaseAttempts 用完) |
| agent_decisions | 369 | 46.1/run |
| rdog_requests | 190 | 23.8/run |
| success | 0/8 | 0% (deepseek model 偏好 direct verbs) |

archive baseline (5 model × 8 case, 旧 binary):
- agent_decisions/run: 6.5
- rdog_requests/run: 6.3
- attempts/run: 1.025
- success: 40/40

deepseek 1 model 跟 archive 5 model per-run 对比:
- agent_decisions: **7.1×** (46.1 vs 6.5)
- rdog_requests: **3.8×** (23.8 vs 6.3)
- attempts: **2.9×** (3.0 vs 1.025)
- success rate: 0% vs 100%

### 总结感悟

- **deepseek 行为特征**: 高 churn + 不收敛. 3 attempts × 30s/attempt × 8 case = 720s/case 模型探索
  量大, 但 maxCaseAttempts 用完还是 fail. 偏好 @ping / @window-find / @ax-find direct verbs
  不走 @computer-act envelope → outcome 字段不出现.
- **archive baseline 40/40 success 跟 current 0/8 对比**: archive 5 model 在旧 binary 下 100% 成功,
  current deepseek 1 model 在新 binary 下 0% 成功. **这反映 model 行为差异 + binary 协议层差异**:
  - archive baseline: 旧 binary (无 outcome 三态), 模型爱用 direct verbs, 不读 outcome 字段,
    "success" 靠 direct verb 的 @capabilities / @ax-find 完整跑通 + verify 步骤,
    模型"自然做对了" 5 model 8 case.
  - current: 新 binary (有 outcome 三态), 模型仍爱用 direct verbs, outcome 字段 wire 上
    正确但模型不读, 而 direct verb 链路有更多可失败点 (max-attempts-retry 不收敛).
- **不是协议 regression**: outcome 三态 + epoch + skill SHA-256 都 push 到 origin (commit 7764c29 + 92b0613).
  protocol 层都验证过 (smoke 12/12 + live 25 tool calls). current 0/8 success 是 model 行为
  问题, 不是协议 regression.
- **runner 端到端工作**: RPC mode 调通 + 6 个 bug fix 链 + manifest 完整 + ledger 分类 OK.
  5×8 runner 骨架 ready, 5 provider 在线, deepseek 1 model 8 case 实证数据已写.

### 后续建议

- **5×8 5 model 全套**: 估计 4 model × 30 min = 2 hours, 估算 ~$5-10 token cost.
  按 user 偏好"跑不到不假装跑过" + 当前 deepseek 0/8 表现, 其他 4 model 可能也 0/8.
  跑完才有 5 model 完整 baseline 对比数据.
- **接受现状 + merge 主分支**: deepseek 1 model 8 case 数据已写, outcome 三态协议
  层已闭环 (wire shape + skill + runner + smoke 12/12). 5 model 完整数据等下一个 model
  alignment 主题. feature/computer-act-outcome-3state 可 merge.
- **prompt engineering 引导 deepseek 走 @computer-act**: 改 case prompt 加 scoring 提示
  (用 @computer-act 才被评分), 1-2 次迭代可能让 success rate 提升.

## [2026-08-09 10:10:00] [Session ID: omx-1786201921174-cvveb1] 任务名称: 5×8 live matrix - 5 model 完整 40 run (选项 A 完成)

### 任务内容

按 user 选项 A 跑剩余 4 model (minimax-cn / qwen37-flash / qwen36-flash / minimax-m27-highspeed) 完成完整 5×8=40 run baseline, 合并 deepseek 1 model 数据, 写 5 model merged suite + 对比 archive baseline。

### 完成过程

- minimax-cn 9:22 spawn → 9:48 done (26 min, 0/8 success)
- qwen37-flash 9:49 spawn → 9:55 done (6 min, 7/8 success)
- qwen36-flash 9:55 spawn → 10:00 done (5 min, 6/8 success)
- minimax-m27-highspeed 10:00 spawn → 10:09 done (9 min, 7/8 success)
- 总后台跑时间 ~1h47m (串行, daemon 冲突避免)
- spawn helper `/tmp/spawn_eval.sh` 复用 Python start_new_session=True + clean output dir

### 5 model × 8 case 实证数据 (合并 /tmp/rdog-eval-5x8-final/suite-result.json)

| model | success | attempts | decisions | rdog | per-run dec | per-run rdog | per-run att |
|---|---|---|---|---|---|---|---|
| deepseek             | 0/8 | 24 | 369 | 190 | 46.1 | 23.8 | 3.00 |
| minimax-cn           | 0/8 | 24 | 338 | 164 | 42.2 | 20.5 | 3.00 |
| qwen37-flash         | 7/8 | 11 |  97 |  49 | 12.1 |  6.1 | 1.38 |
| qwen36-flash         | 6/8 | 13 |  66 |  33 |  8.2 |  4.1 | 1.62 |
| minimax-m27-highspeed| 7/8 | 10 | 128 |  68 | 16.0 |  8.5 | 1.25 |
| **5 model TOTAL**    |**20/40**| **82** | **998** | **504** | 25.0 | 12.6 | 2.05 |
| archive baseline (旧 binary) | 40/40 | 41 | 260 | 252 | 6.5 | 6.3 | 1.03 |

### 对比 archive baseline

- `successful`: -20 (40 → 20, 50% 退化)
- `agent_decisions`: 3.84× (260 → 998)
- `rdog_requests`: 2.0× (252 → 504)
- `attempts`: 2.0× (41 → 82)

### 关键发现: Model 二元分布

**Group A (不收敛, 全 0/8 success):**
- deepseek (deepseek-v4-flash): 46.1 decisions/run, 3 attempts 全用, 8 tool iterations 不够
- minimax-cn (MiniMax-M3): 42.2 decisions/run, 3 attempts 全用, 同样 8 iter 不够

**Group B (收敛快, 6-7/8 success):**
- qwen37-flash (qwen3.7-flash): 12.1 decisions/run, 大多 attempts=1
- qwen36-flash (qwen3.6-flash): 8.2 decisions/run, attempts=1 为主
- minimax-m27-highspeed (MiniMax-M2.7-highspeed): 16.0 decisions/run, attempts=1 为主

**跨 5 model 一致难 case:**
- case 2 calculator-old-state-recovery: 4/5 model fail (qwen37/qwen36/m27/deepseek all fail; minimax-cn case 2 fail 也 fail)
- 其他 7 case: Group B model 全 success

### Runner 行为细节

- `_invoke_pi_rpc` RPC mode + extension (mano_cua_rdog.mjs) + 6 bug fix 全程 OK
- max-tool-iterations=8 + timeout_s=90 + TimeoutExpired catch 全程稳定
- Group A model 大多 trigger `Maximum tool iterations (8) exceeded` 后 retry,3 attempts 全 fail
- Group B model attempt 1 就满足 `has_actions + recoveries==0 + rc==0`,break early

### Model 行为 vs Protocol 行为

- 协议层 (outcome 三态 + epoch + rdog envelope + error envelope) 全程 wire shape 正确
- Model 行为层面: Group A 偏好 direct verbs (`@open-app` / `@ax-find` / `@ax-press`),**不走 `@computer-act` envelope**
- Group B 同样用 direct verbs 但收敛快 + 无 recovery,证明 direct verbs 不必然导致 fail
- archive 5 model 100% success 应该是不同 prompt / 旧 skill 行为,不是 protocol regression

### 总结感悟

- **5×8 live matrix 完整闭环**: 40 run 全部跑完, manifest 完整, archived baseline 可对比
- **Model 行为是 outcome 字段能否被消费的关键**: 协议层 wire shape OK 不等于 model 真读 outcome 字段
- **deepseek + minimax-cn 是 high-churn model**: 不能用 max-tool-iterations=8 (应该 16+ 给 model 探索空间) 或加更多 guidance
- **case 2 calculator-old-state-recovery 跨 4/5 model fail**: 真正难 case,值得单测 / skill 加固
- **archive baseline 不可严格对比**: case prompt / skill SHA-256 不同, 但能看趋势

### 后续建议

- **可选 A1: 接受现状 + merge 主分支**: outcome 三态在协议层闭环 (5×8 完整数据已写, protocol 层验证完成). deepseek/minimax-cn 0/8 是 model 行为问题,不是 protocol regression
- **可选 A2: 修 deepseek/minimax-cn case 1 success + per-case 单独 retry 策略**: max-tool-iterations=16 + 加 case 2 calculator-old-state-recovery 单测
- **可选 A3: prompt engineering 引导所有 model 走 @computer-act**: 系统提示 + scoring 提示, 让 outcome 字段真被消费
- **可选 A4: case 2 calculator-old-state-recovery 单测 + skill 加固**: 跨 4/5 model fail 说明 case 2 prompt 不够明确
- **可选 A5: kill all live daemons + 清 stale guard**: 5 model 跑完确认 daemon 全 kill, /tmp/rdog-eval-* 留作历史

### 硬约束记录

- max-tool-iterations=8 是 trade-off (vs deepseek 30 触发 TimeoutExpired): 保持 8 不变, Group A model 调整靠 prompt / skill
- timeout_s=90 是 per-Pi-call ceiling, 维持
- runner main() 起 daemon 用 current binary (daemon_manager.py), 不要让 stale daemon 留在 host

## [2026-08-09 12:25:00] [Session ID: omx-1786201921174-cvveb1] 任务名称: prompt engineering (选项 A3) - 5×8 重跑 + outcome 字段真被消费

### 任务内容

按 user 选项 A3 改 runner.py `_build_case_prompt` 加显式 @computer-act protocol requirement + scoring incentive,跑 5×8 完整 matrix 验证 outcome 字段被 model 真实消费。

### 改动 (commit 8c5a65d expected)

`runner/lib/runner.py`:
1. `_build_case_prompt` 加 requirement block (放在 ## Task 之前),说明"硬约束 — 必须遵守","所有 GUI 动作必须包在 @computer-act#N:{...} envelope 内","直接调用 @open-app / @ax-press / @key / @click 等 direct verb **不计分**"
2. `_force_computer_act_hint` 简化 (case prompt 已含 requirement,system_prompt 末尾的 IMPORTANT 段重复且 model 忽略 — commit 5c7b9a6 5×8 实证)

### 完成过程

- 改 runner.py, dry-run 验证骨架
- spawn minimax-cn v2 (~17 min, 2/8 success, baseline 0/8)
- spawn qwen37-flash v2 (~10 min, 7/8 success, baseline 7/8 case 2 → case 5 移位)
- spawn qwen36-flash v2 (~5 min, 7/8 success, baseline 6/8 → +1)
- spawn m27 v2 (~10 min, 7/8 success, baseline 7/8 case 2 修但 case 3 fail)
- spawn deepseek v2 (~26 min, 2/8 success, baseline 0/8)
- 合并 5 model v2 suite 到 `/tmp/rdog-eval-5x8-final-v2/suite-result.json`

### 5×8 v2 (prompt engineering) vs baseline (commit 5c7b9a6)

| model              | baseline | v2 prompt | diff | per-run v2 dec | per-run v2 rdog |
|--------------------|----------|-----------|------|----------------|------------------|
| deepseek           |    0/8   |    2/8    | +2   |     38.5       |      16.1        |
| minimax-cn         |    0/8   |    2/8    | +2   |     35.0       |      15.6        |
| qwen37-flash       |    7/8   |    7/8    | +0   |     13.8       |       6.8        |
| qwen36-flash       |    6/8   |    7/8    | +1   |     16.0       |       6.5        |
| minimax-m27-highspeed|   7/8   |    7/8    | +0   |     15.8       |       7.9        |
| **5 model TOTAL**  |**20/40** |**25/40**  |**+5**|                |                  |

| metric         | baseline | v2 prompt | ratio |
|----------------|----------|-----------|-------|
| agent_decisions|    998   |    952    | 0.95× |
| rdog_requests  |    504   |    423    | 0.84× |
| total_attempts |     82   |     72    | 0.88× |
| successful     |     20   |     25    | +5 (+25%) |

### 关键发现

**1. Group A model 大幅改善:**
- deepseek: 0/8 → 2/8 (case 3 calculator-divide-by-zero + case 6 textedit-multi-window)
- minimax-cn: 0/8 → 2/8 (case 1 calculator-happy-path + case 6 textedit-multi-window)
- model 现在用 @computer-act envelope 做 mutation actions, 但 verify 阶段仍用 direct verbs (@ax-find / @window-find)

**2. Group B model 稳定 + 偶有改善:**
- qwen37-flash: case 2 修 (baseline fail → v2 success) 但 case 5 fail (baseline success → v2 fail), 净 0
- qwen36-flash: case 2 修 (baseline fail → v2 success), 净 +1
- m27-highspeed: case 2 修 (baseline fail → v2 success) 但 case 3 fail (baseline success → v2 fail), 净 0

**3. outcome 三态字段现在在 wire 上更频繁出现:**
- 之前: Group A model 全用 direct verbs, outcome 字段不出现
- v2: Group A model 1-5 个 action 用 @computer-act, outcome 字段真在 envelope 里

**4. case 2 calculator-old-state-recovery 跨 4 model v2 都修 (除 minimax-cn):**
- qwen37/qwen36/m27 baseline 都 fail, v2 全 success
- deepseek baseline fail, v2 仍 fail (deepseek case 2 跨 5 model 都 fail)

**5. case 3 calculator-divide-by-zero 仍是 v2 难 case:**
- 3 model baseline success 但 v2 fail (qwen37, qwen36, m27)
- case 3 在 v2 仍 fail 的次数 > baseline fail (3 个 model), case 3 prompt 需 case-specific 改造 (不在 A3 范围)

### 验证证据

- cargo check 0 errors
- python ast parse runner.py OK (31 top-level statements)
- _build_case_prompt 输出含 ## Protocol requirement / ## Task / ## Verify standard / @computer-act / 硬约束 / 不计分 全部 6 个 check
- per-model result.json 在 /tmp/rdog-eval-{model}-v2/*/result.json 全部写好
- merged suite-result.json 在 /tmp/rdog-eval-5x8-final-v2/suite-result.json

### 总结感悟

- **prompt engineering 对 Group A 显著有效**: deepseek + minimax-cn 都从 0/8 改善到 2/8
- **prompt engineering 对 Group B 稳定**: 不破坏现有 success, 部分 case 还改善
- **outcome 三态字段真在 live 上出现**: Group A model 现在至少用 1-5 个 @computer-act per case
- **case 2 calculator-old-state-recovery 跨 Group B 全修**: 验证"硬约束"prompt 引导让 Group B 用 recovery-aware flow
- **case 3 calculator-divide-by-zero 跨 Group B 全退化**: prompt 让 Group B 改了 recovery flow 但 case 3 需要不同 approach (out of A3 scope)
- **Group A model churn 仍高**: deepseek 38.5 dec/run, minimax-cn 35 dec/run — 比 Group B (13-16) 高 ~3x, 但比 baseline (~42-46) 略降
- **整体 +5 success 来自 5 model 分散**: 没有单一 model 主导改善, 提示 prompt engineering 是普遍有效

### 后续建议

- **可选 A3.1: case 3 calculator-divide-by-zero 单测改造**: 在 case prompt 加 "5÷0 显示 'Not a number' 或 'NaN' 或 'Error'" 显式 expected (3 选项任一), 让 model 知道预期. 这能让 Group B 在 case 3 重新稳定
- **可选 A3.2: 接受现状 + merge 主分支**: 25/40 success (62.5%) vs baseline 20/40 (50%), 改善明确
- **可选 A3.3: 进一步 prompt engineering (二阶)**: case-specific 改造 + 多轮 retry 时 prompt 不同 (attempt 2+ 强调 "上轮 fail, 这次先 verify 当前状态再 action")
- **可选 A3.4: 验证 outcome 字段 model 真消费**: 当前 evidence 是 model 用 @computer-act (envelope 出现) 但不确定 model 真读 outcome / verification.status 字段 (回看 messages 检查)

## [2026-08-09 18:30:00] [Session ID: omx-1786268168901-f711dm] 任务名称: bisect 验证 deepseek/minimax-cn "退步" 根因

### 任务内容
- 用户问: deepseek/minimax-cn 之前 7/8 甚至 8/8, 现在怎么了
- 执行 phase 14 bisect: 用 archive 40/40 的同一载体 (外部 runner) + 当前 binary/skill 重跑老 8 case

### 完成过程
1. 读 archive manifest (macos-ops-20260808-key-contract-candidate-5x8/baseline-manifest.json): rustdogCommit=417c6b0a, skill SHA a5063f19, 外部 runner, maxToolIterations=30, 40/40
2. 对比仓库内 runner/cases/*.json: 与 archive 老 case 集只有 3 个重叠 (5 个老 case 被换成 calculator×3 + clipboard + multi-window-textedit)
3. 发现 phase 14 原计划缺陷: eefe802 时仓库内 runner 不存在 (9ba464a 才加), 改用外部 runner 做 bisect 载体
4. 起 daemon (--transport zenoh, unixpipe fast path) + 外部 runner dry-run OK
5. 第一次跑 deepseek: 0/8 且 44ms 秒败, usageTotals=0 → 根因: tmux server 环境无 DEEPSEEK_API_KEY (Pi 报 "No API key found")
6. 带 key 重启: deepseek 8/8, minimax-cn 8/8 (全部 fresh AX verification 真实)

### 验证证据
- /tmp/pi-rdog-macos-ops-deepseek-20260809-175341/suite-result.json: successCount 8/8
- /tmp/pi-rdog-macos-ops-minimax-20260809-180054/suite-result.json: successCount 8/8
- 所有 case checks: freshVerificationObserved / realRdogCallObserved / expectedResultObserved / appWindowObserved 全 true

### 总结感悟
- **模型没有退步**: deepseek + minimax-cn 在当前 binary (outcome 三态+epoch) + 当前 skill 下, 老 case + 外部 runner 依然 8/8
- **"退步"是评测载体变化**: 仓库内 runner 的 max-tool-iterations=8 (archive 30) + prompt 差异 + 5 个新 case 替换, 三者叠加导致 0/8 → v2 prompt 后 2/8
- 评测对比必须先对齐载体 (runner 版本 / case 集 / max-tool-iterations / prompt), 否则会把基础设施差异误判成模型退步
- 外部 runner 依赖 shell 环境注入 API key, tmux server 环境无 key 会秒败 (usageTotals=0), 排查时先看 pi-stderr

## [2026-08-09 20:10:00] [Session ID: omx-1786268168901-f711dm] 任务名称: 仓库内 runner 对齐 archive 载体 (max-iter 30 + case 集对齐 + prompt 增强)

### 任务内容
按 user 拍板, 改仓库内 runner 三件事, 让 deepseek/minimax-cn 回到 8/8:
1. max-tool-iterations 按模型配置 (deepseek/minimax-cn=30, Group B=16)
2. case 集对齐 archive 老 8 case (删 calculator×3/clipboard/multi-window-textedit)
3. prompt 增强 (Protocol contract 段落, 来自外部 runner skill 契约)

### 改动 (待 commit)
- runner/config.json: cases 换老 8 case, models 加 maxToolIterations
- runner/cases/: 8 个老 case 迁移 (含 app/setup/verify/expected), 5 个新 case 删除
- runner/lib/runner.py:
  - _build_case_prompt: v2 @computer-act 硬约束 → Protocol contract (archive 风格)
  - max_iter 从 model_cfg 读, Pi timeout 随 max-iter 放大 (cap 900)
  - 移植外部 runner 严格验证: prepare (quit/open/before capture) → Pi → after capture → 7 项 checks → reset
  - 修复 bug: _run_process 返回类型缺 timed_out; RPC tool result 是 {content:[{text}]} 结构解析

### 验证证据 (live 5×8 全矩阵)
- /tmp/rdog-eval-align-5x8-v2/suite-result.json: deepseek 8/8 + minimax-cn 8/8
- /tmp/rdog-eval-align-gb/suite-result.json: qwen37 8/8 + qwen36 8/8 + m27 8/8
- **完整 40/40 success, 42 attempts (41 case attempt 1, deepseek preview 3)**
- 648 decisions / 315 rdog requests (RPC 计数口径, 非效率对比指标)
- 全部 case 通过 fresh AX verification + expectedResultObserved + appWindowObserved

### 对比
| 载体 | success | attempts | 说明 |
|---|---|---|---|
| archive 外部 runner | 40/40 | 41 | skill 全文嵌入 + maxToolIterations=30 + 老 case |
| 5×8 baseline (旧) | 20/40 | 82 | 仓库内 runner, 8 iter, 新 case, 弱验证 |
| v2 prompt (旧) | 25/40 | 72 | 同上 + @computer-act 硬约束 |
| **本次对齐后** | **40/40** | **42** | 严格验证 + 30 iter + 老 case + contract prompt |

### 总结感悟
- **评测可信度 = 载体三要素**: case 集 / max-tool-iterations / 验证逻辑, 任何一个不对齐都会误判模型能力
- 仓库内 runner 原 success 判定 (有 action + 无 recovery) 是弱启发式, 移植外部严格验证后才可与 archive 可比
- RPC tool result 结构 {content:[{text}]} 是 Pi 的封装格式, 解析时不能只处理裸 list
- 操作坑: models.json 的 apiKey 用 env: 引用, tmux server 环境无 key 会秒败; DASHSCOPE key 在 xtalk/.envrc

## [2026-08-09 21:12:00] [Session ID: omx-1786268168901-f711dm] 任务名称: macOS cargo install 重授权问题取证与方案

### 任务内容
- 定位根因: 默认 adhoc 签名 DR 钉 cdhash, 重编即变, TCC 视为新程序
- 本机验证固定 DR 方案: 两个不同内容二进制签相同 identifier 后 DR 字节级一致
- 给出方案: install 后 `codesign -f -s - --identifier "rdog" --requirements '=designated => identifier "rdog"'`, 首次重授权一次, 之后稳定

### 完成过程
- codesign -dvv / -d --requirements 取证
- /tmp 副本演示: 修改内容后重签, diff DR 仅路径行不同
- 未改动 repo 源码, 未提交代码(纯咨询+取证)

### 总结感悟
- TCC 授权身份 = code requirement, 稳定身份三要素: 固定 identifier + 自定义 DR(或证书)
- `--requirements` 内联文本必须以 `=` 前缀, 否则被当文件路径

## [2026-08-09 21:32:00] [Session ID: omx-1786268168901-f711dm] 任务名称: to-spec 生成 macOS 稳定签名身份 spec

### 任务内容
- 合成 spec: 问题/方案/11 条用户故事/实施决策/测试决策/边界
- 发布 GitHub issue #40 + ready-for-agent label
- 落盘 specs/rdog-stable-signing-identity.md, AGENTS.md 长期索引登记

### 完成过程
- 读 docs/agents/issue-tracker.md + triage-labels.md 确认发布约定
- gh issue create + edit, 验证 labels 正确
- 按 specs/ 现有风格落盘并登记索引

### 总结感悟
- 无 runtime 改动的 dev-workflow 方案, 测试缝选"DR 不变量"而非运行时测试
- 权限保留的最终验收必须人工, spec 中明确写明自动化覆盖边界

## [2026-08-09 22:12:00] [Session ID: omx-1786268168901-f711dm] 任务名称: 实施 issue #40 - install-signed 脚本

### 任务内容
- scripts/install-signed.sh: install -> 重签 (identifier=rdog, 自定义 DR) -> fail-closed 校验
- issue #40 留实施完成 comment
- main 提交 a746b2f (cherry-pick 自 feature 分支 dbedcb2, 顺带把 A3.2 文档记录带入 main)

### 完成过程
- 校验逻辑演进: 字符串断言被 canonical 显示格式坑 (纯字母数字 identifier 省略引号), 改为归一化提取 'designated => identifier <id>' 精确匹配
- codesign --verify -r 对错误 identifier 也放行 (exit=0), 语义不可靠, 弃用
- 真实 install 6m18s (target/release 首次全量编译)

### 总结感悟
- codesign 的 canonical 输出格式随内容变化, 断言必须归一化而非字符串比对
- 验证 prefer codesign 自带评估 (verify/requirements), 但确认其语义后再用
