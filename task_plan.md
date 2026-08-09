# 任务计划: macOS ops 稳定共享摩擦分析

## [2026-08-08 00:43:43] [Session ID: omx-1786061963768-e7in9l] [续档与执行计划]: current-binary 三轮 ledger 只读分析

### 目标

- 从 current reference、repeat A、repeat B 的真实 interaction ledger 中识别跨模型、跨 case 重复出现的共享摩擦。
- 只有同时具备动态重复样本和共享 rdog / canonical skill 静态路径的候选,才进入 decision brief。
- 本阶段只交付 decision brief,不修改 parser、协议、primitive、canonical skill、runner 或 case。

### 阶段

- [x] 阶段 0: 读取 workflow、历史六文件、重复采样报告和当前 Git 状态,恢复上一轮 checkpoint。
- [ ] 阶段 1: 聚合三份 ledger 的 error code/message、verb、独立 `(model, case)` 样本和 recovery 请求序列。
- [ ] 阶段 2: 对达到重复门槛的候选定位共享 parser、protocol、primitive 或 canonical skill 路径。
- [ ] 阶段 3: 排除模型格式探索、窗口瞬态、权限波动和不安全语义归一化,形成 decision brief。
- [ ] 阶段 4: 验证文档、回溯六文件与归档状态,在批准 gate 前停止实现。

### 候选判断

- 主假设: 三轮中可能存在少量跨样本重复的通用失败形状,可由共享控制层减少 recovery 请求。
- 最强备选解释: 高成本主要来自模型自身探索和瞬态运行波动,不存在值得修改共享层的稳定候选。
- 推翻主假设的证据: 重复模式无法跨至少两个独立 `(model, case)` 样本,或静态路径只支持改变语义、放宽权限、猜测 target 的修复。

### 续档说明

- 旧 `task_plan.md` 已超过 1000 行,稳定保存为 `archive/default_history/task_plan_2026-08-08_004343_macos_ops_interaction.md`。
- 按 `continuous-learning` 规则执行后台总结、归档 manifest 和长期知识索引检查。主线只读分析不等待其完成。

### 状态

**当前在阶段 1**: 先从机器可读 ledger 建立动态重复证据,不从单个错误文本猜测修复方向。

## [2026-08-08 00:48:12] [Session ID: omx-1786061963768-e7in9l] [错误记录]: 首次 ledger 聚合排序失败

- 现象: 临时 Python 聚合器在排序 response error signature 时抛出 `TypeError: '<' not supported between instances of 'NoneType' and 'int'`。
- 原因: ledger 中部分 error code 为 `null`,排序键直接比较了 `None` 与整数。
- 处理: 不修改 ledger;仅在临时分析器的排序键中把 code 转成字符串,保留原始 code 用于展示和分组。
- 状态: 阶段 1 尚未完成,本次失败输出不作为动态证据。

## [2026-08-08 01:02:49] [Session ID: omx-1786061963768-e7in9l] [阶段更新]: 动态聚合与共享路径筛选完成

- [x] 阶段 1: 完成三份 current-binary ledger 的 error code/message、verb、独立 `(model, case)` 样本和相邻 recovery 聚合。
- [x] 阶段 2: 定位 `parse_key_payload`、`parse_ax_press_payload`、`parse_window_find_payload` 与 `resolve_unique_app_window_id` 的共享路径,并核对 canonical skill 与 protocol reference。
- [ ] 阶段 3: 写入 decision brief,区分可批准的通用契约改良与必须拒绝的语义猜测。
- [ ] 阶段 4: 等待批准 gate;批准前不改 parser、协议行为、primitive、canonical skill、runner 或 case。

### 动态结论

- `@key` 对象字段漂移(`target`/`keys`/`shortcut`)共 20 个 response errors,覆盖 7 个独立 `(model, case)` 样本和 3 轮,其中 12 次紧接 recovery。
- `@ax-press` 顶层 `action` 共 9 个 response errors,覆盖 4 个独立样本和 3 轮,9 次都紧接 recovery。
- 两类合计 29 个 parser errors,最多暴露 21 个可避免的紧接 recovery 请求;这是上限,不是已验证收益。
- App selector 多窗口歧义和 AX/window stale locator 都重复出现,但现有 fail-closed 语义是安全不变量,不能用自动选择或静默重绑替代。

### 静态结论

- `src/control_protocol/parsers/key.rs:41-175` 只接受 `key`、`hold_ms`、`mode`、`modifiers`、`delivery`、`pid`、`window_id`;`target`、`keys`、`shortcut` 没有无歧义的现有语义。
- `src/control_ax.rs:1231-1325` 的 `@ax-press` 固定 AXPress;其它 AX action 走独立 `@ax-action` parser,接受顶层 `action` 会改变命令边界。
- `.codex/skills/rdog-control/references/protocol.md` 已给出基础 `@key` 示例,但第 633 行把完整语法指向 canonical `SKILL.md` 中不存在的章节,造成通用文档路径断裂。

### 当前状态

**阶段 3**: decision brief 已生成,批准前停止实现。

## [2026-08-08 01:08:21] [Session ID: omx-1786061963768-e7in9l] [阶段完成]: decision brief 已生成

- [x] 阶段 3: 完成 `decision-brief__20260808-stable-shared-friction.md`,包含动态样本、静态路径、请求差额上限、风险和批准/拒绝/暂缓三种决策。
- [ ] 阶段 4: 等待用户批准。批准前不修改 parser、协议行为、primitive、canonical skill、runner 或 case。

### 推荐决策

- 推荐选项 1: 只做通用 canonical skill/reference 契约澄清,不添加 `target` / `keys` / `shortcut` / 通用 `action` parser alias。
- 选项 2: 保持实现不变,继续观察模型探索噪声。
- 选项 3: 另开 durable selector/ref + 显式 `auto_refind` 设计,不作为本轮短期修复。

### 停止条件

**当前在批准 gate**: brief 已落盘并待审阅;未获批准不进入实现。

## [2026-08-08 00:32:00] [Session ID: omx-1786061963768-e7in9l] [批准后执行]: 通用 key/AX 契约澄清

### 目标

- 只改 canonical `rdog-control` skill 和 protocol reference 的通用文案,减少模型在 `@key` 与 AX action 上的语法探索。
- 不新增 parser alias,不改变协议行为,不记录任何特定 App 或 case 的操作序列。

### 阶段

- [x] 阶段 1: 应用通用 skill/reference 契约澄清。
- [x] 阶段 2: 运行静态检查和最小 parser 回归。
- [ ] 阶段 3: 运行完整 5 x 8 live matrix 并生成 interaction ledger。
- [ ] 阶段 4: 审阅请求数、错误和新鲜证据,决定是否提升 baseline。

### 已完成改动

- `.codex/skills/rdog-control/SKILL.md`: 增加 `Local @key Actions`,列出合法字段、targeted delivery 约束和禁止字段;明确 `@ax-press` 与 `@ax-action` 边界。
- `.codex/skills/rdog-control/references/protocol.md`: 增加同一 `@key` / AX action 合同,修正 `Local @key Actions` 引用。
- `.codex/skills/rdog-control/references/control-workflow.md`: 同步修正引用。

### 验证

- `git diff --check` 通过。
- `rtk cargo nextest run --package rustdog --bin rdog -E 'test(parse_should_support_compact_bare_key_payloads) | test(parse_should_accept_raw_single_line_cmd_and_reject_ambiguous_payloads)' --no-capture`: 2 passed。

### 状态

**当前在阶段 3**: 使用相同活动模型和 8 个 case 做完整认证,不把单轮收益当作稳定结论。

## [2026-08-08 09:00:00] [Session ID: omx-1786061963768-e7in9l] [归档动作]: 整理已结束的后缀支线上下文

- [x] 已确认默认六文件仍是当前 macOS ops interaction ledger 主线,继续保留在根目录。
- [x] 已将 24 个旧支线主题的 81 个带后缀文件整组移动到 `archive/branch_contexts/<topic>/`。
- [x] 已生成本批归档 manifest,并同步 `EXPERIENCE.md` 与 `AGENTS.md` 的长期索引。
- [x] 已完成归档完整性、格式和工作区范围验证。

## [2026-08-08 09:12:00] [Session ID: omx-1786061963768-e7in9l] [阶段完成]: 归档与长期索引验证

- [x] 24 个旧支线主题、81 个文件已整组移动到 `archive/branch_contexts/<topic>/`。
- [x] 已生成 `archive/manifests/ARCHIVE_MANIFEST__2026-08-08_macos_ops_interaction_ledger.md`,包含完整 81 条映射。
- [x] 已将三轮 current-binary ledger 的请求波动、provenance 门禁和 parser 安全边界追加到 `EXPERIENCE.md`。
- [x] 已在 `AGENTS.md` 索引归档 manifest 和 `workflows/macos-ops-interaction-efficiency.md`。
- [x] 已验证根目录无 `*__*.md`,manifest 映射无重复且每条目标均存在,`git diff --check` 通过。

**当前状态**: 归档支线已完成;默认 macOS ops 认证仍在阶段 3,本轮归档没有修改 parser、协议、primitive、runner、skill 或 case。

## [2026-08-08 01:48:00] [Session ID: omx-1786061963768-e7in9l] [阶段完成]: key contract candidate 认证收口

### 阶段

- [x] 阶段 3: 完成 5 个活动模型 x 8 个 case 的 live matrix,归档完整 source artifacts 和 interaction ledger。
- [x] 阶段 4: 完成请求数、错误、新鲜证据和逐 case 门禁审阅。

### 动态结果

- 40/40 case 成功,40 attempts;全部保留真实 rdog 调用和 fresh verification。
- candidate: 213 agent decisions、209 rdog requests、20 response errors。
- 输入兼容 current reference: 258 decisions、243 requests、25 response errors。
- candidate 中 `@key target/keys/shortcut` 和 `@ax-press action` 请求均为 0。
- 9 个未变化 `(model, case)` 的请求数高于 current reference,且没有不可替代验证证据。

### 决策

- candidate 总请求数下降 34,但未通过逐 case 不增长门禁,因此不提升 baseline。
- 保留 `.codex/skills/rdog-control` 的通用契约澄清和完整失败 candidate artifacts;不添加 parser alias、App recipe 或 case 特例。
- 已认证 baseline 保持不变。

### 状态

**本轮完成**: 已按 workflow 的 fail-closed 规则收口,没有待执行的认证步骤。

## [2026-08-08 14:00:00] [Session ID: omx-1786112971218-cbf063] 阶段 1: 方案 1 - 资源 epoch + stale write fast-reject

### 目标

新增 `epoch` 字段到 `@observe` 响应,以及可选 `epoch` 字段到 `@computer-act` 等
mutating 命令,使得客户端可以显式回传它们观察到的 epoch,后端在 dispatch
之前 fast-reject 不匹配的 epoch (而不是依赖 TTL 检测一次就走到底再失败)。

### 范围 (向后兼容加成)

- 不改变现有 `@observe` / `@ax-press` / `@click` / `@computer-act` 默认行为
- 不动 TTL-based eviction 逻辑
- 不引入 per-resource epoch (留给后续 Phase B)
- 不修改其他 mutating 命令 (key / mouse / drag / wheel / type-text 等) — 那些命令
  暂不参与 epoch fast-reject, 等 Phase B 用 `@observe` + `@computer-act` 路径
  验证后再扩张

### 阶段

- [ ] 阶段 1: 规划 + 建分支
- [ ] 阶段 2: 在 `src/control_observation/observe/response.rs` 加 `epoch` 顶层字段
- [ ] 阶段 3: 在 `src/control_protocol.rs` 加 `epoch: Option<u64>` 到 `ComputerActRequest`
- [ ] 阶段 4: 在 `src/control_protocol/parsers/computer_act.rs` 解析 `epoch` 字段
- [ ] 阶段 5: 在 `src/control_actions.rs` 的 `execute_computer_act` 前置 epoch 校验
- [ ] 阶段 6: 加测试 (parser 接受/拒绝 epoch, dispatch 验证/拒绝)
- [ ] 阶段 7: 编译 + 单测 + 协议文档同步
- [ ] 阶段 8: 提交 commit + 推送

### 关键决策

- **epoch 值 = `created_at_unix_ms`**: 最小可行, 无新状态, 复用现有 observation header
- **可选字段**: 不传 epoch = 走原路径 (TTL 校验); 传了 epoch = 显式 fast-reject
- **错误码**: `STALE_OBSERVATION_EPOCH`, 复用现有 `stale_observation_ref_error` 风格
- **分支名**: `feature/observe-epoch-stale-reject`, 基于 `restore-point-20260803-1300`
- **不**修改 `ObservationHeader` struct: 响应 JSON 顶层加 `epoch` 字段, 不破坏现有
  serializer / durable store

### 风险

- 误用 epoch: 客户端可能错把 `observation_id` 当 epoch — 需在 SKILL.md 明确
- epoch 单位不明: 需在 spec 写 "epoch == observed_at_unix_ms, 单位 ms"
- 与 future per-resource epoch 的迁移: 留有 docstring 说明 "epoch 是 daemon-global
  monotonic, 未来可能换成 per-resource"

### 状态

**当前在阶段 1**: 创建分支 + 写 plan + 准备动手

### 阶段 1 完成

- [x] 阶段 1: 规划 + 建分支 (`feature/observe-epoch-stale-reject`)
- [x] 阶段 2: `@observe` 响应顶层 `epoch` 字段 (src/control_observation/observe/response.rs)
- [x] 阶段 3: `ComputerActRequest.epoch: Option<u64>` (src/control_protocol.rs)
- [x] 阶段 4: parser 接受 `epoch`, 拒绝重复/负数/非整数 (src/control_protocol/parsers/computer_act.rs)
- [x] 阶段 5: `check_observation_epoch_fast_reject` 在 implicit_observe 之前拦截 (src/control_computer_act/mod.rs)
- [x] 阶段 6: 加 12 个测试 (parser 5 + dispatch 5 + response 2)
- [x] 阶段 7: 编译 + 697 个 tests pass (524 control + 173 其它), 0 warning
- [ ] 阶段 8: 文档同步 (SKILL.md / REFERENCES) + 提交

### 验证证据

- `RUSTFLAGS="-Awarnings" cargo check --quiet`: exit 0, no output
- `cargo nextest run -p rustdog --bin rdog -E 'test(/control_/)'`: 524 passed, 0 failed
- `cargo nextest run -p rustdog --bin rdog -E 'not test(/control_/)'`: 173 passed, 0 failed
- 新加 12 个 tests 全部 pass:
  - `parse_should_accept_computer_act_epoch`
  - `parse_should_accept_computer_act_without_epoch`
  - `parse_should_reject_computer_act_negative_epoch`
  - `parse_should_reject_computer_act_non_integer_epoch`
  - `parse_should_reject_computer_act_duplicate_epoch_field`
  - `epoch_check_passes_when_epoch_matches_header`
  - `epoch_check_rejects_when_epoch_mismatches`
  - `epoch_check_rejects_when_observation_absent`
  - `epoch_check_returns_none_when_epoch_not_provided`
  - `epoch_check_returns_none_when_observation_id_missing`
  - `render_observe_response_should_expose_epoch_at_top_level`
  - `build_observe_response_should_expose_epoch_even_without_primary_observation`

### 决议

- 维持"向后兼容加成": 不传 epoch 的现有 caller 行为完全不变
- epoch 仅作用于 `@computer-act`, 其它 mutating 命令 (`@ax-press` / `@click` / ...)
  暂不参与, 等 Phase B 用 `@computer-act` 验证后再扩张
- epoch = `created_at_unix_ms` (最小可行), 未来 per-resource epoch 时只换
  `resolve_observation_header` 的实现, 上层 API 不变

## [2026-08-08 15:00:00] [Session ID: omx-1786112971218-cbf063] 阶段 1: 方案 2 - postcondition 三态 outcome

### 目标

按 pi-computer-use `ActOutcome = "worked" | "didnt" | "unknown"` 语义, 把
@computer-act 响应 envelope 从 `ok:false + error_code:verify_failed` 反转成
`ok:true + outcome:"didnt"`, 让 dispatch 成功 vs 动作生效两个概念彻底分开.

### 范围 (向后兼容 - 部分破坏)

- 推翻 LP-ticket-15-deferred-2-RESOLVED (Phase F-2) 的 `ok:false` 改写路径
- 新增 `ComputerActOutcome` enum + `outcome` 顶层字段
- `verify_failed` 详情挪到 `verification.failed_reason: "verify_failed"`
- outcome mapping:
  - `worked` = dispatch ok + (verify=None OR verify_passed)
  - `didnt` = dispatch ok + verify_failed
  - `unknown` = dispatch ok + verify 未执行 (timeout / cancel)
- smoke 脚本需要更新期望 (test 2 / test 3 的 verify_failed assertion)
- 单元测试 `verify_failed_envelope_json_matches_e2_shape` 改名为
  `render_verification_failed_reason` 或保留 helper 但不再用于 envelope rewrite

### 阶段

- [ ] 阶段 1: 规划 + 建分支 (`feature/computer-act-outcome-3state`)
- [ ] 阶段 2: 加 `ComputerActOutcome` enum + `render_outcome` helper
- [ ] 阶段 3: 改 `execute_computer_act` 响应 envelope (加 outcome, 删 verify_failed rewrite)
- [ ] 阶段 4: 加 `verification.failed_reason` / `verification.status` 字段
- [ ] 阶段 5: 更新 error_envelope.rs 测试 (verify_failed helper 改建)
- [ ] 阶段 6: 新增 outcome 三态单测
- [ ] 阶段 7: 更新 smoke 脚本 expected assertion
- [ ] 阶段 8: 编译 + 全套测试 + 提交
- [ ] 阶段 9: 文档同步 (specs + LATER_PLANS + WORKLOG)

### 关键决策

- `outcome` 总是存在 (即使 `ok: false`, 也保留 `outcome: "unknown"` 占位)
- `ok: true` 永远表示 dispatch succeeded (无论 verify 结果)
- `ok: false` 永远表示 dispatch failed (permission / infra / timeout / unknown_action / invalid_args)
- `outcome: "didnt"` 替代 `ok: false + error_code: verify_failed`
- `verification.failed_reason: "verify_failed"` 保留 detail
- `ComputerActErrorCode::VerifyFailed` enum 保留 (供 retry strategy / hint reference), 不再用作 top-level error_code

### 风险

- 破坏现有依赖 `ok: false` 判定 verify 失败的 caller. 写明在 commit message 顶部 + WORKLOG
- smoke 脚本更新, 跑不了的话 `scripts/smoke_computer_act_*.sh` 必须同步

### 状态

**当前在阶段 1**: 写 plan + 创建分支 + 准备动手

## [2026-08-08 17:30:00] [Session ID: omx-1786201921174-cvveb1] 阶段 5-7 落地: outcome 三态测试 + dead code 清理

### 上下文承接

上一 session (omx-1786112971218-cbf063) 在 `feature/computer-act-outcome-3state` 分支
已经:
- [x] 阶段 1: 规划 + 建分支
- [x] 阶段 2: outcome.rs (174 行, 7 个单测)
- [x] 阶段 3: mod.rs 删 83 行 verify_failed envelope rewrite, 加 43 行 outcome 计算
- [x] 阶段 4: verify.rs 加 verification_status_for_diff + verification.status 字段

本 session 继续:
- [ ] 阶段 5: error_envelope.rs 清理 VerifyFailed dead code
- [ ] 阶段 6: verify.rs 加 verification_status_for_diff 单测 (3 档)
- [ ] 阶段 7: tests.rs 加 execute_computer_act outcome 字段集成测试
- [ ] 阶段 8: smoke 脚本更新期望 (2 处 verify_failed → outcome:"didnt")
- [ ] 阶段 9: cargo check + cargo test 全套
- [ ] 阶段 10: 文档同步 + commit + push

### 本轮决策

- `ComputerActErrorCode::VerifyFailed` enum 保留 (按 task_plan 上一段决议, 供 reference)
- `verify_failed_envelope_json()` helper 删 (0 caller, dead code)
- 2 个 helper 集成测试删 (依赖被删的 helper)
- `verify_failed_carries_ax_diff_evidence` 测试保留 (测 error_envelope 工厂本身, 与 VerifyFailed variant 共生)
- 不加 `verification.failed_reason` 字段 (outcome + status 已足够区分, ponytail 拒绝冗余)
- smoke 脚本期望从 `error_code:"verify_failed"` 改成 `outcome:"didnt"`

## [2026-08-08 17:55:00] [Session ID: omx-1786201921174-cvveb1] 阶段 5-10 收口: commit + push 完成

### 阶段推进

- [x] 阶段 5: error_envelope.rs 清理 VerifyFailed dead code (删 helper + 2 测试, -69 行)
- [x] 阶段 6: verify.rs 加 verification_status_for_diff 单测 (3 档: verified / preexisting / failed)
- [x] 阶段 7: outcome 三态单测不下冗余集成测试 — 决策点
  - outcome.rs 7 个单测已覆盖决策表 + wire 字符串 + render, 不再加 mod.rs 集成测试
  - 理由: ponytail 推 "non-trivial logic leaves ONE runnable check" — outcome.rs 已是唯一真相源
  - WORKLOG 记录这一决策
- [x] 阶段 8: smoke 脚本更新期望 (verify.sh + trace.sh test 3 各改 outcome:"didnt" + status:"failed")
- [x] 阶段 9: cargo check + cargo test 全套 (705 passed, 0 failed, 1 skipped macOS-only)
- [x] 阶段 10: 文档同步 + commit + push
  - LATER_PLANS.md SUPERSEDED 标注
  - WORKLOG.md 阶段记录
  - specs/rdog-computer-act-spec.md 加 Outcome 三态段
  - tickets/13 + tickets/14 加 outcome acceptance
  - task_plan.md 推进
  - commit 7764c29 + push origin/feature/computer-act-outcome-3state

### 验证证据汇总

- RUSTFLAGS="-Awarnings" cargo check --quiet: exit 0, 无 warning
- cargo nextest run -p rustdog --bin rdog: 705 passed, 0 failed, 1 skipped
- +10 新测试 (7 outcome + 3 verification_status), -3 dead 测试
- 净改动: +464 / -182 = +282 行 (含 outcome.rs +174)
- 单一 commit: 7764c29

### 任务完成

feature/computer-act-outcome-3state 分支已推到 origin. 后续决策:

- 跑一轮 macOS live smoke 验证 outcome / status 字段被 client 实际读取
- 如果 client 没读, 进 LATER_PLANS 考虑删 outcome / status (跟 epoch 验证类似)
- 不推广 outcome 三态到 @ax-press / @click 等 mutating 命令 (路径上无 outcome 概念)

## [2026-08-08 23:55:00] [Session ID: omx-1786201921174-cvveb1] 阶段 11: macOS live smoke + 方案 2 闭环

### 完成

- [x] macOS live smoke 三件套全过:
  - smoke_computer_act_verify.sh: 5/5 (含 outcome:"didnt" + status:"failed" 真实出现)
  - smoke_computer_act_trace.sh: 3/3 (outcome:"didnt" + trace_savefile 落盘)
  - mac_lab_live_smoke.sh: 4/4 (ping / literal shell / pty / tty display)
- [x] outcome 三态 decision table 5 行全部实证 (含 preexisting 中间档)
- [x] verification.status 三档 (verified / preexisting / failed) 真实出现 (failed + preexisting 已实证)
- [x] TCC 权限 OK (daemon 日志无 warning, verify_ms=955-1023ms 真实 AX capture)
- [x] smoke 期望设计 bug 修复: scripts/smoke_computer_act_verify.sh test 3 outcome / status 改枚举匹配 (锁 wire contract 不锁特定值)
- [x] WORKLOG.md `[2026-08-08 23:50:00]` 完整 live evidence
- [x] notes.md `[2026-08-08 23:50:00]` 决策表实证表
- [x] EPIPHANY_LOG.md `[2026-08-08 23:50:00]` preexisting 中间档 + smoke 锁 contract 哲学

### 方案 2 闭环决策

- outcome / status 字段在 live wire 上工作, smoke 全过, skill 同步完成
- 方案 2 (postcondition 三态) 正式 close, 不进 LATER_PLANS
- "preexisting" 中间档真实有效, 是 outcome 三态 ADR 的核心论证 (不再凑数)

### 下一轮建议

- 修 dead_code warning (LP-ticket-20-deferred-1): `key_delivery_backend` + 5 个 ComputerActErrorCode variant
- 等下一个集成 client 落地后跑 5×8 matrix 验证 outcome 字段被 client 实际消费

## [2026-08-09 01:00:00] [Session ID: omx-1786201921174-cvveb1] 阶段 12: 重建 5×8 macOS ops matrix runner

### 背景

- 方案 2 outcome 三态已落地 + smoke 全过 + skill 同步
- outcome / status 在 wire 上工作, 但 "client 真消费" 仍是间接证据
- archive 提到 `runner/eval-macos-ops.sh` 在 "外部评测工程", 该工程已不在 filesystem
- 当前 pi agent `models.json` 有 5 个活动 provider (deepseek / minimax-cn / qwen37-flash / qwen36-flash / minimax-m27-highspeed)
- archive `macos-ops-20260807-live-5x8` 5 模型各 8/8 全部成功, 是 baseline

### 目标

重建 5×8 macOS ops matrix runner + case + ledger, 跑一次 current-binary live matrix
验证 outcome / status / epoch 字段是否被真客户端 (Pi-driven models) 实际消费.

### 阶段

- [x] 阶段 12.1: 扫环境 (Pi binary + extension + provider + archive evidence)
- [ ] 阶段 12.2: user 拍板 5 model 完整列表 + 8 case 完整列表 (基准对齐 archive 5×8)
- [ ] 阶段 12.3: 写 runner 骨架 (eval-macos-ops.sh + lib/ + cases/ + ledger/)
- [ ] 阶段 12.4: 8 case prompt 文件 (基于 archive 已知 case + 当前 binary 能力)
- [ ] 阶段 12.5: 5 model 配置 (对齐 models.json, 清理 qwen-plus stale)
- [ ] 阶段 12.6: 写 interaction ledger schema + classification (rdog.macos-ops.interaction-ledger.v1)
- [ ] 阶段 12.7: dry-run 验证 (provider 在线探测 + daemon 起来 + skill SHA-256 锁定)
- [ ] 阶段 12.8: live matrix 跑 5×8=40 run (maxCaseAttempts=3, 失败 case 重试到 3 次)
- [ ] 阶段 12.9: 汇总 ledger, 对比 baseline (archive 5×8), 验证 outcome / status 字段
- [ ] 阶段 12.10: docs / commit / push

### 关键决策

- runner 放 rustdog 仓库下 `runner/eval-macos-ops.sh` (不是外部 sibling project, 让 runner 跟 rdog binary 在同一仓库, 减少 runner/rdog 版本错位风险)
- case prompt 8 个对齐 archive 5×8 (5 老 + 3 新: terminal-run-command / safari-new-tab-navigate / textedit-multi-window)
- 5 model 对齐 models.json 当前 5 个活动 provider (deepseek / minimax-cn / qwen37-flash / qwen36-flash / minimax-m27-highspeed)
- interaction ledger schema 对齐 workflows/macos-ops-interaction-efficiency.md 的 v1 schema
- classification 规则: query / action / post_action_evidence / recovery / supporting_shell / unknown (6 档, 不读 app/case/prompt 文本)

### 风险

- Pi provider API key 可能过期 (5 个 model 都 set, 但实时可用性需探测)
- 5×8 = 40 run + maxCaseAttempts=3 = 120 run 上限, 时间 1-3 小时
- 1-2 天工作量, 跟 user 拍板范围
- 历史 baseline 是 archive 5×8 (canonical skill SHA-256 `129aa820...`), 当前 skill SHA-256 不同, baseline 不严格可比但能看趋势

### 状态

**当前在阶段 12.2**: 等 user 拍板 5 model + 8 case 完整列表.

## [2026-08-09 09:30:00] [Session ID: omx-1786201921174-cvveb1] [阶段更新]: 跑 4 model 完整 5×8 live matrix (选项 A)

### 背景

- deepseek 1 model × 8 case 跑完 (commit 3ea6c58, WORKLOG [2026-08-09 09:17:00])
- 0/8 success, per-run 46.1 decisions / 23.8 rdog / 3.0 attempts (vs archive baseline 6.5 / 6.3 / 1.025)
- user 选项 A: 跑其他 4 model 完成完整 5×8 baseline
- 5 provider HTTP 200 在线 (deepseek / minimax-cn / qwen37-flash / qwen36-flash / minimax-m27-highspeed)
- daemon / stale guard 干净, runner 骨架就位

### 目标

- 跑剩余 4 model × 8 case = 32 case, 完成 5×8 = 40 run 完整 baseline
- 每 model 独立 output dir 隔离 (`/tmp/rdog-eval-<model>`)
- 每 model 完成后合并 suite-result.json 到 `/tmp/rdog-eval-5x8-final/suite-result.json`
- 跑完更新 WORKLOG + commit + push

### 阶段

- [x] 阶段 12.11: 启动检查 (git status clean / 5 provider 200 / daemon 干净 / stale 清)
- [ ] 阶段 12.12: minimax-cn × 8 case live (后台, 估计 30-60 min)
- [ ] 阶段 12.13: qwen37-flash × 8 case live
- [ ] 阶段 12.14: qwen36-flash × 8 case live
- [ ] 阶段 12.15: minimax-m27-highspeed × 8 case live
- [ ] 阶段 12.16: 合并 5 model 数据 + archive baseline 对比 + 写 WORKLOG
- [ ] 阶段 12.17: commit + push (待 user 拍板)

### 关键决策

- 4 model 串行跑 (避免并发 daemon 冲突)
- 每 model 独立 output dir 隔离结果
- max-tool-iterations=8 + timeout=90s 已 fix (commit 92b0613)
- runner main() 起 daemon + 跑完 kill, 全程自己管理

### 风险

- 4 model 估算 2-4 hours, deepseek 1 model ~30 min
- 其他 4 model 可能也 0/8 success (跟 deepseek 行为类似)
- 如果 model 长时间 retry 不收敛, per-case 可能 3 attempts × 90s = 4.5 min (maxCaseAttempts=3)
- macOS TCC 权限 (accessibility + screen recording) 必须仍 OK, 不然 AX capture 失败

### 当前状态

**阶段 12.12 即将启动**: minimax-cn × 8 case live, output dir `/tmp/rdog-eval-minimax-cn`.

## [2026-08-09 10:10:00] [Session ID: omx-1786201921174-cvveb1] [阶段完成]: 5 model 完整 5×8 matrix 跑完

### 完成

- [x] 阶段 12.12: minimax-cn × 8 case live (9:22 → 9:48, 26 min, 0/8 success)
- [x] 阶段 12.13: qwen37-flash × 8 case live (9:49 → 9:55, 6 min, 7/8 success)
- [x] 阶段 12.14: qwen36-flash × 8 case live (9:55 → 10:00, 5 min, 6/8 success)
- [x] 阶段 12.15: minimax-m27-highspeed × 8 case live (10:00 → 10:09, 9 min, 7/8 success)
- [x] 阶段 12.16: 5 model 数据合并 + archive baseline 对比 + 完整 WORKLOG entry
- [x] 阶段 12.17: git add + commit + push (本次 task)

### 关键结果

- 5×8 = 40 run 完整跑完
- 20/40 success (50% 退化 vs archive 40/40)
- decisions 3.84× / rdog 2.0× / attempts 2.0× vs archive
- 2 model 全 0/8 (deepseek + minimax-cn), 3 model 6-7/8 (qwen37/qwen36/m27)
- case 2 calculator-old-state-recovery 跨 4/5 model fail

### 后续任务 (不进 commit)

- 选项 A1: 接受现状 + merge 主分支 (等 user 拍板)
- 选项 A2: 修 deepseek/minimax-cn 高 churn (max-iter 16 + prompt guidance)
- 选项 A3: prompt engineering 引导 @computer-act 路径
- 选项 A4: case 2 单测 + skill 加固

## [2026-08-09 10:15:00] [Session ID: omx-1786201921174-cvveb1] [新阶段 13]: prompt engineering 引导所有 model 走 @computer-act (选项 A3)

### 目标

- 让 Group A model (deepseek + minimax-cn) 也走 `@computer-act` envelope, 让 outcome / verification.status 字段真被 model 消费
- 5 model × 8 case 重跑, 验证 success rate 提升 (baseline 20/40 = 50%)
- 验证 model 是否真用 `@computer-act` (wire 上 outcome 字段出现率)

### 现状 (来自 5×8 baseline commit 5c7b9a6)

- 5 model: 20/40 success
- Group A (deepseek + minimax-cn): 0/8 全 fail, 全 direct verbs (`@open-app` / `@ax-find` / `@ax-press`), 不走 `@computer-act`
- Group B (qwen37 + qwen36 + m27): 6-7/8 success, 也用 direct verbs, 但收玫快
- runner `_force_computer_act_hint` 已加但 model ignore (system prompt 末尾加 IMPORTANT 段)

### 阶段

- [ ] 阶段 13.1: 读 8 case prompt 文件 + 当前 system_prompt hint, 定位 prompt engineering 落点
- [ ] 阶段 13.2: 改 case prompt 加 `@computer-act` wrapper example + scoring incentive
- [ ] 阶段 13.3: 改 runner `_force_computer_act_hint` 更明确 (强引导)
- [ ] 阶段 13.4: dry-run 验证骨架
- [ ] 阶段 13.5: 跑 5 model × 8 case (重跑完整 matrix)
- [ ] 阶段 13.6: 验证 model 是否真用 `@computer-act` (count envelope usage in wire events)
- [ ] 阶段 13.7: 对比 5×8 baseline (commit 5c7b9a6) success rate + decisions ratio + rdog ratio
- [ ] 阶段 13.8: 写 WORKLOG + commit + push (按 user scoped commit 偏好)

### 关键决策

- prompt engineering 是 iterative, 不预设 1 次到位
- 改 case prompt 是主要杠杆 (model 第一眼看 case task 字段)
- runner system_prompt hint 是次要杠杆 (model 优先看 case task)
- scoring incentive ("用 @computer-act 才被评分") 强制 model 走 envelope
- 跑 5 model 完整 matrix, 不偷工 (user 偏好 "跑不到不假装跑过")

### 风险

- Group A model 可能仍 ignore (api-level 行为难改)
- prompt 改坏可能让 Group B model 也 fail
- 5×8 重跑 ~2 hours
- 改了 case prompt 跟 archive baseline 不可比 (但跟 commit 5c7b9a6 baseline 可比)

### 状态

**当前在阶段 13.1**: 读 case prompt + runner system_prompt hint 现状.

## [2026-08-09 12:25:00] [Session ID: omx-1786201921174-cvveb1] [阶段完成]: A3 prompt engineering 5×8 重跑实证

### 完成

- [x] 阶段 13.1-13.3: 改 runner.py `_build_case_prompt` + 简化 `_force_computer_act_hint`
- [x] 阶段 13.4: dry-run 验证骨架
- [x] 阶段 13.5: 跑 5 model × 8 case v2 (minimax-cn 17 min + qwen37 10 min + qwen36 5 min + m27 10 min + deepseek 26 min)
- [x] 阶段 13.6: 验证 model 是否真用 @computer-act (Group A 现在 1-5 action/case, baseline 0)
- [x] 阶段 13.7: 对比 baseline: 20/40 → 25/40 success (+5), attempts -12%, decisions -5%, rdog -16%
- [x] 阶段 13.8: WORKLOG entry + commit + push

### 关键数据

| metric | baseline | v2 prompt | ratio |
|---|---|---|---|
| successful | 20/40 | 25/40 | +5 (+25%) |
| attempts | 82 | 72 | 0.88× |
| decisions | 998 | 952 | 0.95× |
| rdog | 504 | 423 | 0.84× |

### Group A 改善

- deepseek: 0/8 → 2/8 (+2)
- minimax-cn: 0/8 → 2/8 (+2)

### Group B 稳定

- qwen37-flash: 7/8 → 7/8 (case 2 修, case 5 退化)
- qwen36-flash: 6/8 → 7/8 (+1, case 2 修)
- m27-highspeed: 7/8 → 7/8 (case 2 修, case 3 退化)

### 后续 (LATER_PLANS 已记录)

- A3.1: case 3 单测改造
- A3.2: merge 主分支
- A3.3: 二阶 prompt engineering (attempt-aware)
- A3.4: 验证 model 真消费 outcome 字段

## [2026-08-09 12:35:00] [Session ID: omx-1786201921174-cvveb1] [新阶段 14]: git bisect 验证 deepseek 0/8 退步根因 (选项 1)

### 背景

- deepseek + minimax-cn 在 archive baseline (旧 binary) 都 8/8
- 现在 baseline (新 binary) 都 0/8, v2 (prompt engineering) 都 2/8
- 改动可能原因: binary (outcome 三态 in 7764c29) + skill SHA-256 (in 4b864a3) + runner RPC mode (in a382458) + epoch (in 6bbce4b)
- user 选项 1: checkout 到 c78c76e 之前 (skill SHA-256 不同之前) 的 binary + runner, 看 deepseek 是否回到 8/8

### 计划

- [ ] 阶段 14.1: 创建 temp branch + checkout 到 eefe802 (skill 旧, binary 仍 outcome 三态, runner 旧)
- [ ] 阶段 14.2: cargo build 旧 binary, 验证编译
- [ ] 阶段 14.3: 跑 deepseek 8 case (用旧 binary + 旧 skill + 旧 runner)
- [ ] 阶段 14.4: 写 WORKLOG + 切回 main branch

### 关键决策

- 先做 eefe802 test (isolates skill change): 如果 deepseek=8/8 → skill 是原因
- 如果仍 0/8,继续 bisect: 7764c29~1 (= 6bbce4b) → tests binary outcome 三态
- 再不行: 6bbce4b~1 (= 28ed415) → tests epoch + runner + everything

### 状态

**当前在阶段 14.1**: 创建 temp branch + checkout.

## [2026-08-09 18:05:00] [Session ID: omx-1786268168901-f711dm] [阶段更新]: phase 14 bisect 计划修正 + Step 1 启动

### 新发现 (有证据)

1. **archive 40/40 的载体是外部 runner**: `/Users/cuiluming/local_doc/l_dev/my/rust/pi-rdog-calculator-eval/runner/` (run_macos_ops_eval.py SHA cae01559, worktree clean 在 master)
2. **archive baseline manifest** (macos-ops-20260808-key-contract-candidate-5x8/baseline-manifest.json):
   - rustdogCommit = 417c6b0a (28ed415 的父 commit, 在当前 feature 分支历史内)
   - skill SHA-256 = a5063f19 (旧版, 无 outcome 三态 reference)
   - rdog binary SHA = db5cb9fd, maxToolIterations=30 (config-macos-ops.json 现仍为 30)
   - case = macos-ops-prompts.json (8 case: textedit-type-text / calendar-window-check / safari-navigate-example / preview-open-image / terminal-window-check / terminal-run-command / safari-new-tab-navigate / textedit-multi-window)
   - summary.md: deepseek 41 decisions / 40 requests / 8 attempts; allCasesPassed=true → 40/40
3. **仓库内 runner/cases/*.json 与 archive case 集只有 3 个重叠** (terminal-run-command / safari-new-tab-navigate / textedit-multi-window):
   - 仓库内 5 个新 case: calculator-divide-by-zero / calculator-happy-path / calculator-old-state-recovery / clipboard-copy-paste / multi-window-textedit
   - archive 5 个老 case 被替换: textedit-type-text / calendar-window-check / safari-navigate-example / preview-open-image / terminal-window-check
   - task_plan 之前写 "5 老 + 3 新" 与实际 runner/cases 不符 (实际 5 新 + 3 老)
4. **原 phase 14 计划缺陷**: "checkout 到 eefe802 用旧 runner" 不成立 — 仓库内 runner 是 9ba464a (08-09) 才进仓库, eefe802 时 runner/ 不存在
5. **外部 runner 的 config 指向当前工作区**: rdogBinary = rustdog/target/debug/rdog (当前 89c34b99, 含 outcome 三态 + epoch), canonicalSkillPath = rustdog/.codex/skills/rdog-control/SKILL.md (当前版)

### 修正后的 bisect 计划 (载体 = 外部 runner)

- [x] 阶段 14.1: 确认 archive manifest + 外部 runner 可跑 (完成上述证据收集)
- [ ] 阶段 14.2: Step 1 - 外部 runner + 当前 binary (89c34b99) + 当前 skill + 老 8 case 跑 deepseek (~20-30 min)
      8/8 → 模型没退步, 差异在仓库内 runner/case/prompt; 0/8 → binary/skill 影响大
- [ ] 阶段 14.3: Step 2 - checkout 旧 SKILL.md (4b864a3~1) + 外部 runner 重跑 deepseek, 隔离 skill 变量
- [ ] 阶段 14.4: Step 3 - cargo build 417c6b0a 旧 binary + 外部 runner 重跑 deepseek, 隔离 binary 变量
- [ ] 阶段 14.5: 汇总 bisect 结论 + 写 WORKLOG + 切回原分支

### 前置检查 (Step 1 前)

- [ ] daemon 状态确认 (外部 runner 不自动起 daemon, 需要 rdog_macos.toml + 当前 binary 手动起)
- [ ] dry-run 验证外部 runner 骨架
- [ ] TCC 权限 (AX + screen recording) 确认

### 风险

- Step 1 用当前 binary 跑老 case, 如果 8/8 → 直接证明模型没有退步, 问题在评测基础设施 (case 集 + runner 迭代上限 + prompt)
- deepseek 1 model × 8 case 约 20-30 min (archive 参考)
- 真实操控 macOS 应用 (TextEdit/Safari/Preview/Calendar/Terminal), 用户已拍板 phase 14 选项 1

### 状态

**当前在阶段 14.2 前置检查**: daemon 状态 + dry-run.

### Step 1 执行记录

- daemon: rdog daemon --transport zenoh 已起 (tmux rdog-daemon, PID 44662), unixpipe fast path, @ping pong
- 外部 runner: dry-run OK (8 老 case)
- Step 1 启动: tmux rdog-eval, PID 63102, output_root /tmp/pi-rdog-macos-ops-deepseek-20260809-174815
- 预计 20-40 min, 完成后读 suite-result.json 的 successCount

### Step 1 第一次跑失败原因 (已修复)

- 现象: 0/8, 每 attempt 44ms 秒败, usageTotals.totalTokens=0
- 根因: tmux server 环境没有 DEEPSEEK_API_KEY (len=0), Pi 进程报 "No API key found for provider deepseek"
- 验证: exec shell key len=35, tmux server key len=0
- 修复: 重启 Step 1 时显式 export DEEPSEEK_API_KEY (tmux rdog-eval, output_root 175341)
- 教训: 外部 runner 依赖 shell 环境注入 key, 不自己处理; 仓库内 runner (a382458) 是显式处理 env key 的

### Step 1 结果: deepseek 老 case 8/8 (关键证据)

- 外部 runner + 当前 binary 89c34b99 + 当前 skill + 老 8 case → **8/8 success**
- 8 case 全部: freshVerificationObserved=true, realRdogCallObserved=true, expectedResultObserved=true, appWindowObserved=true
- verify 有真实 AX observation (obs-1786269257341-70, permission granted)
- 结论: **deepseek 模型没有退步**; binary (outcome 三态+epoch) 和 skill 变化都不是主因
- 退步来源锁定为: 仓库内 runner (max-tool-iterations 30→8 + prompt 差异) + 新 case 集 (calculator×3 + clipboard + multi-window-textedit)
- Step 2/3 (隔离 skill/binary) 不再必要, 8/8 已证伪 binary/skill 主因假设
- minimax-cn 对照跑中: /tmp/pi-rdog-macos-ops-minimax-20260809-180054

### Step 1 最终结论: deepseek + minimax-cn 双双 8/8 (老 case + 外部 runner)

- deepseek: 8/8 (大多 attempt 1, safari-new-tab-navigate 2 attempts)
- minimax-cn: 8/8 (safari-navigate-example 2, terminal-run-command 3, safari-new-tab-navigate 2)
- 全部 fresh AX verification 真实 (freshVerificationObserved=true)
- **bisect 结论: 模型没有退步**; 退步根因 = 仓库内 runner 的 max-tool-iterations=8 + prompt 差异 + 新 case 集 (5 个老 case 换成 calculator×3/clipboard/multi-window-textedit)
- phase 14 完成: Step 2/3 (隔离 skill/binary) 不需要, Step 1 已证伪
- 遗留: 仓库内 runner 若要回到 archive 水平, 需 max-tool-iterations 提到 30 (或按 model 配置) + case 集对齐 + prompt 增强

## [2026-08-09 18:40:00] [Session ID: omx-1786268168901-f711dm] [新阶段 15]: 仓库内 runner 对齐 archive 载体 (max-iter 30 + case 集对齐 + prompt 增强)

### 目标
- 仓库内 runner 达到 archive 外部 runner 同等的评测效果 (deepseek/minimax-cn 8/8)
- 三个改动: max-tool-iterations 30 (或按模型) / case 集对齐 archive 老 8 case / prompt 增强

### 阶段
- [ ] 阶段 15.1: 读仓库内 runner 现状 (runner.py / config.json / cases / README) + 外部 runner 的老 case prompts + prompt 构造
- [ ] 阶段 15.2: 设计改动方案 (per-model max-iter? case 文件如何迁移? prompt 增强落点)
- [ ] 阶段 15.3: 改代码
- [ ] 阶段 15.4: dry-run 验证骨架
- [ ] 阶段 15.5: 跑 deepseek + minimax-cn (先验证 Group A 回到 8/8)
- [ ] 阶段 15.6: (可选) 跑完整 5×8 对比
- [ ] 阶段 15.7: WORKLOG + commit + push

### 风险
- case prompt 迁移时 verify 逻辑要对齐外部 runner 语义
- 改 prompt 可能影响 Group B (qwen) — 需要全矩阵验证
- max-iter 30 会让 Group A 单 case 更久 (per-run 40+ decisions), 全矩阵时间更长

### 状态
**当前在阶段 15.1**: 读两套 runner 代码

### 阶段 15.2 设计决策

1. **max-tool-iterations 按模型**: config.json models[] 加 maxToolIterations, deepseek/minimax-cn=30, Group B=16 (5×8 显示 Group B 8-16 dec/run)
2. **case 集对齐**: 外部 macos-ops-prompts.json 的 8 个老 case 完整迁移 (含 app/setup/verify/expected), 删 5 个新 case (calculator×3 + clipboard + multi-window-textedit)
3. **prompt 增强**: case prompt 加 "Protocol contract" 段落 (来自外部 system-prompt-with-skill.md 前言 + 当前 SKILL.md 核心契约), 去掉 v2 的 @computer-act 硬约束 (移植严格验证后计分不看 envelope, archive 风格下 deepseek 0-1 dec/case 高效)
4. **验证逻辑移植 (必要)**: 仓库内 runner 原 success 判定是弱启发式 (有 action+无 recovery), 移植外部 runner 的 before/after capture + fresh verification + 8 项 checks, 让结果可信且与 archive 可比
5. daemon_manager 不动 (已验证 -c config 能识别 zenoh)

### 阶段 15.3/15.4 完成
- runner/cases: 8 个老 case 已写入, 5 个新 case 已删
- config.json: cases 更新 + models 加 maxToolIterations (deepseek/minimax-cn=30, Group B=16)
- runner.py: prompt 改 Protocol contract + 移植 prepare/capture/classify/reset + max-iter 按模型 + Pi timeout 随 max-iter 放大 (cap 900)
- 验证: py_compile OK, dry-run OK (5x8=40), 纯函数单测 OK (prompt/classify/fresh 正反例)
- 启动 live: deepseek + minimax-cn, output /tmp/rdog-eval-align-5x8

### 阶段 15.5 结果: Group A 16/16 全 success (严格验证下)

- deepseek 8/8 (preview-open-image 3 attempts, 其他 attempt 1)
- minimax-cn 8/8 (全 attempt 1)
- totals: 16 success / 18 attempts, 389 decisions, 189 rdog requests
- 对比: 5×8 baseline 时 Group A 每 case 42-46 decisions 全 fail; 现在平均 24/case 全 success
- 修复的 bug: (1) _run_process 返回类型缺 timed_out; (2) RPC tool result 是 {content:[{text}]} 结构, 解析只处理了 list → fresh verification 永远 false
- 关键: tmux 环境无 API key (models.json 用 env: 引用), 启动需显式 export; DASHSCOPE key 在 xtalk/.envrc
- Group B (qwen37/qwen36/m27) 全矩阵跑中: /tmp/rdog-eval-align-gb

### 阶段 15.6 结果: 完整 5×8 = 40/40 全 success

- Group A: deepseek 8/8 + minimax-cn 8/8 (16/16, 18 attempts)
- Group B: qwen37 8/8 + qwen36 8/8 + m27 8/8 (24/24, 全 attempt 1)
- totals: 40/40 success, 42 attempts, 648 decisions, 315 rdog requests
- 全部严格验证 (fresh AX + expected + window)
- 对比: 5×8 baseline 20/40 → v2 25/40 → 对齐后 40/40 (archive 同级)
- 收尾: WORKLOG 已写, 准备 commit (scoped)

### 完成
- [x] 阶段 15.1-15.7 全部完成

### 阶段 15.7 (后续收尾): immutableBaseline 更新为新 40/40 基准
- runner/config.json immutableBaseline: 260/252/41 (archive 旧) → 648/315/42 (2026-08-09 对齐后, 严格验证 40/40, RPC 口径)
- 注释同步更新: attempts 为硬指标, decisions/requests 同口径对比
- json 合法 + dry-run OK
