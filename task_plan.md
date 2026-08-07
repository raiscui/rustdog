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
