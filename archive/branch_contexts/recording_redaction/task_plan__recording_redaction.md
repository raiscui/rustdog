# 任务计划: 定义敏感输入脱敏与 Replay 参数模型

## [2026-07-22 12:29:40] [Session ID: omx-1784512435044-92wxat] [任务启动]: Recording redaction Wayfinder ticket

## 目标

形成可直接进入实施规划的安全规格,固定 Recorder 对文本、Secure Input、AX secure field、安全性未知目标、paste 和快捷键的分类,以及 Journal redaction、Replay 参数注入、缺参失败和审计边界。

## 阶段

- [x] 阶段 1: claim ticket,固定问题、依赖和停止条件。
- [x] 阶段 2: 读取 capture、lifecycle、Journal、semantic input、paste 与 flow 契约。
- [ ] 阶段 3: 逐项确认分类、redaction、参数声明、运行时注入、缺参和审计。
- [ ] 阶段 4: 编写并验证正式 redaction/parameter 规格。
- [ ] 阶段 5: 发布 resolution、关闭 ticket、更新 map/frontier并收口支线记录。

## 两个方向

1. 不惜代价的完整方案: 引入 secret vault、加密参数存储、字段级 taint propagation、策略 DSL 和多级审计。能力完整,但会把 Recorder 规格扩张成凭据管理系统。
2. 先把安全边界做对的简化方案: 基于可验证信号分类,未知即 redacted;Journal 只存 marker/parameter descriptor;Replay 启动时显式注入值,缺参 fail closed,运行时值不落盘。

## 做出的决定

- 采用方向 2: 用户已要求不要过度设计。
- 密码、token、Secure Input、AX secure field 和默认剪贴板内容绝不进入 Journal、manifest、Replay Script、trace 或日志。
- 本 ticket 不设计 secret storage、交互式密码管理器或通用模板语言。
- 本 ticket 是 Wayfinder grilling,每次只确认一个真正影响隐私或回放可执行性的决定点。

## 关键问题

1. 哪些信号足以判定 ordinary、sensitive 或 unknown?
2. Journal redaction marker 与 parameter descriptor 最少需要哪些字段?
3. Replay Script 如何引用参数而不携带真实值?
4. 参数值从哪里注入,如何避免进入 trace、error 和 artifact?
5. 缺参、类型不符、重复绑定和未使用参数如何失败?
6. paste、快捷键与输入法组合事件如何归类?

## 停止条件

- 正式规格覆盖 ticket body 的分类、Journal、manifest、Replay Script、注入、缺参和审计要求。
- Mermaid、JSON、Markdown、引用和 staged diff 验证通过。
- Resolution comment、ticket close、map pointer 和新 frontier 完成动态验证。

## 当前状态

**阶段 2 进行中**: ticket 已 assign 给 `raiscui`;下一动作是读取既有规格并整理不能被新模型破坏的协议边界。

## [2026-07-22 12:36:00] [Session ID: omx-1784512435044-92wxat] [阶段更新]: ticket claim 已验证

- `定义敏感输入脱敏与 Replay 参数模型` 保持 `OPEN`,label 为 `wayfinder:grilling`。
- GitHub assignee 已动态确认为 `raiscui`。
- 阶段 1 完成,进入既有契约核对。

## [2026-07-22 12:39:00] [Session ID: omx-1784512435044-92wxat] [遇到错误]: Memory 检索发生 shell 命令替换

- 现象: 双引号 grep pattern 中的反引号被 zsh 当作命令替换,输出 `command not found: rdog.recording.v1`。
- 影响: 只读检索的一段 pattern 失效,没有写文件或改变 tracker 状态。
- 处理: 后续包含反引号的 shell 参数统一使用单引号,并改用精确行段读取。

## [2026-07-22 12:43:00] [Session ID: omx-1784512435044-92wxat] [遇到错误]: 未匹配 glob 被 zsh 拒绝

- 现象: 只读检索参数 `src/macos*` 没有匹配文件,zsh 在命令执行前返回 `no matches found`。
- 影响: 该组中的一条补充检索未执行,其他并行证据不受影响,没有文件写入。
- 处理: 后续只传明确存在的路径,需要枚举时先用 `rg --files`。

## [2026-07-22 12:44:00] [Session ID: omx-1784512435044-92wxat] [阶段更新]: 既有契约核对完成

- [x] Secure Input 检测能力和不保存键值/键数边界已确认。
- [x] Journal redaction interval、cause 和禁止字段已确认。
- [x] `@type-text`、`@paste`、`@key` 的职责边界已确认。
- [x] `rdog.flow.v1` 当前没有通用参数机制已确认。
- [ ] 下一步: 一次只确认一个设计决定,先确认 `ordinary` / `sensitive` / `unknown` 三态分类。

**阶段 3 进行中**: 等待用户确认三态分类和 `unknown` fail-closed 规则。

## [2026-07-22 13:18:05] [Session ID: omx-1784512435044-92wxat] [决定确认]: 输入安全三态分类

- [x] `ordinary`: 只在完整、明确的非 secure 证据成立时允许保存文本语义。
- [x] `sensitive`: Secure Input、secure field 或显式 secret 声明。
- [x] `unknown`: 安全证据不足、焦点/目标歧义、权限不足或相关 gap。
- [x] `sensitive` 与 `unknown` 使用相同的 fail-closed 持久化规则。
- [x] canonical terms 已同步到 `CONTEXT.md`。
- [ ] 下一步: 确认 recorded paste 的参数化和回放语义。

**阶段 3 进行中**: 三态分类已固定,正在确认 paste 的录制与回放规则。

## [2026-07-22 13:37:55] [Session ID: omx-1784512435044-92wxat] [决定确认]: recorded paste 参数化

- [x] paste 只记录意图和目标上下文,不读取 clipboard content。
- [x] Replay 使用必填文本参数,不依赖环境 clipboard。
- [x] 实际投递复用 `@type-text mode:"clipboard"` 的显式 opt-in 路径。
- [x] 非文本 paste 不属于 v1 支持范围。
- [ ] 下一步: 确认快捷键、功能键和导航键分类。

**阶段 3 进行中**: paste 规则已固定,正在确认非文本键盘动作的记录边界。

## [2026-07-22 20:09:38] [Session ID: omx-1784512435044-92wxat] [决定确认]: 快捷键与非文本按键

- [x] 明确、完整且在 redaction 外的快捷键/功能键/导航键可编译为 `@key`。
- [x] 可打印字符和输入法相关按键不冒充快捷键。
- [x] redaction interval 内不保存任何键盘结构。
- [x] 歧义分类 fail closed。
- [ ] 下一步: 确认 IME/dead-key/composed-text 规则。

**阶段 3 进行中**: 快捷键边界已固定,正在确认组合文本的 canonical 语义。

## [2026-07-22 20:10:42] [Session ID: omx-1784512435044-92wxat] [决定确认]: IME 与 composed text

- [x] Replay 只保留已确认的最终 committed text。
- [x] 不重放 IME/dead-key/emoji 的 raw key 或候选过程。
- [x] intermediate marked text 不生成 Replay step。
- [x] sensitive/unknown 继续参数化。
- [ ] 下一步: 确认 ordinary 但 committed text 不可验证时的 compiler 行为。

**阶段 3 进行中**: 组合文本语义已固定,正在确认不可验证 ordinary 文本的处理方式。

## [2026-07-22 20:15:13] [Session ID: omx-1784512435044-92wxat] [决定确认]: ordinary unverified text 参数化

- [x] 不可靠的 ordinary committed text 自动转为 required Replay Parameter。
- [x] 安全分类保持 ordinary,不与 semantic confidence 混用。
- [x] Replay Parameter 统一使用不回显、不落盘的运行时边界。
- [x] `Replay Parameter` canonical term 已同步到 `CONTEXT.md`。
- [ ] 下一步: 确认 parameter descriptor 最小字段。

**阶段 3 进行中**: 输入分类与参数化触发条件已固定,正在收敛 descriptor schema。

## [2026-07-22 20:26:27] [Session ID: omx-1784512435044-92wxat] [决定确认]: parameter descriptor 最小字段

- [x] 单一身份字段使用 `parameter_id`。
- [x] v1 只支持 `value_type:"text"`。
- [x] descriptor 保存 classification、reason 和 origin journal pointer。
- [x] 所有参数隐式 required,禁止持久化 value/default 和扩展约束 DSL。
- [ ] 下一步: 确认 Journal、manifest、Replay Script 的 descriptor ownership。

**阶段 3 进行中**: descriptor 字段已固定,正在确认跨产物的单一真相源。

## [2026-07-22 20:30:24] [Session ID: omx-1784512435044-92wxat] [决定确认]: descriptor ownership

- [x] Recording Journal 唯一创建完整 descriptor。
- [x] manifest 和 Replay Script 只派生复制。
- [x] Replay step 只引用 `parameter_id`。
- [x] 缺少 canonical origin 时 compiler fail closed。
- [ ] 下一步: 确认 `rdog.flow.v1` 的参数引用 step 形状。

**阶段 3 进行中**: descriptor 单一真相源已固定,正在确认 Replay step 的安全引用形式。

## [2026-07-22 20:36:32] [Session ID: omx-1784512435044-92wxat] [决定确认]: typed `TypeText` flow step

- [x] literal/parameter text 统一使用 typed `TypeText`。
- [x] `text` 只允许 literal 或 parameter 二选一。
- [x] 复用现有 `@type-text` control core/backend。
- [x] 禁止所有通用字符串插值。
- [ ] 下一步: 确认运行时参数注入通道。

**阶段 3 进行中**: Replay step 形状已固定,正在确认值如何安全进入单次执行。

## [2026-07-22 20:37:28] [Session ID: omx-1784512435044-92wxat] [研究更新]: transport 尚无机密性契约

- [x] 当前文档只固定 trusted host/network/daemon 边界。
- [x] 未发现 control transport 的 TLS 或端到端 confidentiality 保证。
- [ ] 先确认 controller-side ephemeral binding 输入来源。
- [ ] 随后确认 sensitive/unknown parameter 的 transport gate。

**阶段 3 进行中**: 正在分别收敛参数的本地输入边界和传输边界。

## [2026-07-22 20:50:35] [Session ID: omx-1784512435044-92wxat] [决定确认]: runtime binding 输入源

- [x] CLI 仅从 stdin strict JSON 读取。
- [x] SDK 仅接收 in-memory map。
- [x] 禁止 argv、environment 和持久化 params file。
- [x] 所有 bindings 在连接前完成全量校验。
- [ ] 下一步: 确认 sensitive/unknown transport confidentiality gate。

**阶段 3 进行中**: 本地注入入口已固定,正在确认敏感值的传输条件。

## [2026-07-22 20:52:15] [Session ID: omx-1784512435044-92wxat] [决定确认]: transport confidentiality gate

- [x] ordinary parameter 复用 trusted-network 边界。
- [x] sensitive/unknown 只允许动态验证为 confidential 的 transport。
- [x] unixpipe 必须验证 actual endpoint、daemon identity、UID owner 和 `0600` mode。
- [x] sensitive/unknown 禁止透明 fallback 到非 confidential transport。
- [x] 不安全时在任何 binding 发送和 step 执行前失败。
- [ ] 下一步: 确认 parameter/reference/binding 集合完整性规则。

**阶段 3 进行中**: 参数输入与传输边界已固定,正在确认执行前全量校验。

## [2026-07-22 21:02:35] [Session ID: omx-1784512435044-92wxat] [决定确认]: parameter 集合严格校验

- [x] descriptor ID 唯一且每个只允许一个 step reference。
- [x] descriptor、reference 和 binding 集合必须完全相等。
- [x] missing、undeclared、duplicate、type mismatch 均有稳定错误码。
- [x] empty string 合法,不引入长度约束。
- [x] 全部校验和 transport gate 通过后才能执行。
- [ ] 下一步: 确认审计与不回显边界。

**阶段 3 进行中**: fail-closed 参数校验已固定,正在确认各类输出可记录的 metadata。

## [2026-07-22 22:35:24] [Session ID: omx-1784512435044-92wxat] [决定确认]: audit 与 runtime exposure

- [x] 所有日志/trace/response/error/artifact 禁止 value 及其派生特征。
- [x] ordinary literal text 也不在 runtime 输出中回显。
- [x] Debug redaction 与 best-effort zeroize 纳入实现要求。
- [x] paste clipboard exposure 必须在 manifest/preflight 显式披露。
- [x] 进程内存、可信 transport、目标 app 和系统 clipboard 的运行时边界已说明。
- [ ] 下一步: 确认一次性消费和 no-retry 语义。

**阶段 3 进行中**: 审计边界已固定,正在确认参数生命周期与不确定投递处理。

## [2026-07-22 22:39:30] [Session ID: omx-1784512435044-92wxat] [决定确认]: parameter lifecycle 与 no-retry

- [x] binding 单次消费,step 后立即清理。
- [x] 任一失败终止 Replay并清理剩余 bindings。
- [x] timeout/disconnect 后返回 uncertain,禁止自动重发。
- [x] 新 invocation 必须重新提供全部 bindings。
- [ ] 下一步: 确认 redaction segment 与 parameter cardinality。

**阶段 3 进行中**: 参数生命周期已固定,正在确认 redaction 到 parameter 的切分规则。

## [2026-07-22 22:40:59] [Session ID: omx-1784512435044-92wxat] [决定确认]: redaction segment cardinality

- [x] `cause + target identity` segment 一对一生成 Replay Parameter。
- [x] target/cause 变化触发 segment 切分。
- [x] paste 和 ordinary unverified input 各自按 input span 生成参数。
- [x] 不按时间、键数或值相等性猜测切分。
- [x] target unresolved 时保存 descriptor但禁止生成无目标 step。
- [ ] 下一步: 确认稳定 parameter ID 分配。

**阶段 3 进行中**: parameter cardinality 已固定,正在确认 ID 稳定性。

## [2026-07-23 00:19:47] [Session ID: omx-1784512435044-92wxat] [决定确认]: parameter ID 分配

- [x] Recording Session 内使用 `param-N` 顺序 ID。
- [x] 分配顺序跟随 descriptor entry 的 `journal_seq`。
- [x] ID 不复用、不重排、不编码任何输入特征。
- [x] 离线重编译只复制 canonical Journal ID。
- [ ] 下一步: 确认 bindings 的 wire request 位置。

**阶段 3 进行中**: ID 规则已固定,正在确认单次执行请求如何携带 bindings。

## [2026-07-23 15:52:06] [Session ID: omx-1784512435044-92wxat] [决定确认]: runtime-only root bindings

- [x] script root 只保存 `parameters[]` descriptors。
- [x] execution request 临时附加 root `bindings`。
- [x] Bundle/Replay serializer 禁止输出 bindings。
- [x] 不新增参数多帧协议或 argv inline 入口。
- [ ] 下一步: 确认 flow parameter capability gate 与旧 daemon 行为。

**阶段 3 进行中**: wire placement 已固定,正在确认版本兼容和禁止降级边界。

## [2026-07-23 16:24:00] [Session ID: omx-1784512435044-92wxat] [决定确认]: flow parameter capability gate

- [x] 保存格式继续使用 `rdog.flow.v1`。
- [x] `@capabilities` 新增 `flow_parameters` capability。
- [x] Bundle manifest 声明 `requires:["flow_parameters"]`。
- [x] capability 与 actual transport gate 在读取 stdin bindings 前完成。
- [x] 旧 daemon 返回 `FLOW_PARAMETERS_UNSUPPORTED`。
- [x] 禁止 literal、字符串插值或旧 `ControlLine` 降级。

**阶段 3 已完成**: 参数模型与安全执行边界均已确认。

## [2026-07-23 16:24:00] [Session ID: omx-1784512435044-92wxat] [阶段 4 开始]: 正式规格与 Wayfinder 收口

- [ ] 创建 `specs/rdog-recording-redaction-parameter-model.md`。
- [ ] 覆盖分类、segment、Journal/manifest/flow schema、capability preflight、transport、审计、错误、生命周期和验收矩阵。
- [ ] 添加 flowchart 与 sequenceDiagram,并用 `beautiful-mermaid-rs --ascii` 验证。
- [ ] 使用 `python3` 验证规格中的完整 JSON 示例。
- [ ] 更新 `AGENTS.md` 索引和相关规格的 planned-extension pointer。
- [ ] 记录本轮只读命令错误,检查 scoped diff。
- [ ] 发布 resolution comment、关闭当前 ticket,更新 Wayfinder map frontier。
- [ ] 写入支线 WORKLOG 并完成上下文收口。

**阶段 4 进行中**: 先读取既有 Journal、Flow、tracker 约束,再撰写正式规格。

## [2026-07-23 16:38:00] [Session ID: omx-1784512435044-92wxat] [阶段进展]: 正式规格初稿完成

- [x] 已创建 `specs/rdog-recording-redaction-parameter-model.md`。
- [x] 已覆盖分类、segment、Journal/manifest/flow schema、preflight、transport、审计、错误、生命周期和验收矩阵。
- [ ] 正在验证全部 JSON 示例与两类 Mermaid 图。

**阶段 4 进行中**: 规格内容已落盘,进入机器验证与关联文档同步。

## [2026-07-23 16:45:00] [Session ID: omx-1784512435044-92wxat] [阶段进展]: 规格机器验证与长期索引同步完成

- [x] 9 个完整 JSON 示例已由 `python3` 严格解析,并检查 duplicate key。
- [x] flowchart 与 sequenceDiagram 已由 `beautiful-mermaid-rs --ascii` 成功渲染。
- [x] `AGENTS.md` 已增加正式规格索引。
- [x] Journal 与 Flow 规格已增加 planned-extension pointer,且未误报为已实现。
- [x] 本轮两个只读命令错误已写入 `ERRORFIX__recording_redaction.md`。
- [ ] 下一步执行 scoped diff 与 tracker 收口。

**阶段 4 进行中**: 规格和验证已完成,正在检查改动边界并准备发布 resolution。

## [2026-07-23 16:52:00] [Session ID: omx-1784512435044-92wxat] [发布准备]: 提交正式规格资产

- [x] 已确认既有 Wayfinder 使用 commit permalink 发布 resolution asset。
- [x] 已确认关闭当前 ticket 后的新 frontier 为 `验证语义提升与坐标 fallback 的可行性` 和 `定义 Recording Bundle schema 与原子导出`。
- [x] GitHub dependency API transient EOF 已单独重试并记录。
- [ ] 仅暂存 `AGENTS.md`、`CONTEXT.md`、两份关联规格 pointer 和新正式规格。
- [ ] staged diff 验证通过后提交并推送 `main`。

**阶段 4 进行中**: 正在发布可由 Issue 固定引用的正式规格资产。

## [2026-07-23 17:03:00] [Session ID: omx-1784512435044-92wxat] [发布完成]: 正式规格已推送

- [x] Scoped commit: `28c21dc70338f2180a5a789ec7c10ec9eeb8b063`。
- [x] `origin/main` 已更新到该 commit。
- [ ] 按 Wayfinder 顺序发布 resolution comment、关闭 ticket、更新 map Decisions so far。

**阶段 4 进行中**: 正在执行 GitHub tracker 原子收口步骤。

## [2026-07-23 17:12:00] [Session ID: omx-1784512435044-92wxat] [任务完成]: 规格与 Wayfinder 收口

- [x] 创建并发布正式规格。
- [x] 覆盖全部已确认决策与验收矩阵。
- [x] 9 个 JSON 示例和 2 个 Mermaid 图通过机器验证。
- [x] 更新 `AGENTS.md` 与 Journal/Flow planned-extension pointer。
- [x] 记录全部已知命令错误及修复证据。
- [x] 发布 resolution comment并关闭 `定义敏感输入脱敏与 Replay 参数模型`。
- [x] 更新 Wayfinder map Decisions so far。
- [x] 验证关闭后的 frontier。
- [x] 创建 `WORKLOG__recording_redaction.md`,并检查无需 LATER/EPIPHANY。

**阶段 4 已完成**: 本 ticket 的设计、验证、发布与支线上下文收口全部完成。

## [2026-07-23 17:16:00] [Session ID: omx-1784512435044-92wxat] [上下文发布]: 提交支线记录

- [x] 支线 task plan、notes、WORKLOG、ERRORFIX 均已有实质内容。
- [x] 不需要创建同后缀 LATER_PLANS 或 EPIPHANY_LOG。
- [ ] 仅暂存 4 个 `recording_redaction` 支线上下文文件。
- [ ] 验证 staged diff 后提交并推送。

**状态**: 正在发布本 ticket 的可追溯决策与验证记录。

## [2026-07-23 17:21:00] [Session ID: omx-1784512435044-92wxat] [最终状态]: 全部待办完成

- [x] 正式规格 commit `28c21dc70338f2180a5a789ec7c10ec9eeb8b063` 已推送。
- [x] 支线上下文 commit `fe39ffeb738c982ebdb9e58780bf32f522490275` 已推送。
- [x] Local HEAD 与 `origin/main` 已通过 SHA 相等验证。
- [x] Issue resolution、ticket close、map decision pointer 和新 frontier 均已验证。
- [x] 默认六文件中的其他会话改动保持未暂存、未撤回。
- [x] 已回溯 LATER/EPIPHANY,本 ticket 无需新增记录。

**任务完成**: 当前 ticket 没有剩余设计、验证、发布或上下文待办。
