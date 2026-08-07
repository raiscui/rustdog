# 任务计划: 验证语义提升与坐标 fallback 的可行性

## [2026-07-23 17:35:00] [Session ID: omx-1784512435044-92wxat] [任务启动]: Wayfinder prototype ticket

### 目标

制作一个可运行、可审阅的低成本 logic prototype,用具体 trace 比较点击、文本、快捷键、滚轮和拖拽的语义动作与 guarded `os-logical` 坐标 fallback,让 human 决定首版 promotion policy。

### 两个方向

1. 不惜代价方案:直接实现 macOS CGEventTap + AX/Web live capture prototype,真实录制五类动作。证据最接近生产,但会提前侵入 capture runtime、权限和线程设计,超出低成本 decision prototype。
2. 先验证决策方案:用现有协议/schema构造可交互 trace lab,显式展示 physical evidence、semantic candidates、staleness、ownership、guard 和 compiler choice;必要时再对单一争议点做 live calibration。

采用方向 2。理由是 ticket 明确要求低成本 prototype,用户此前也确认不要过度设计;本轮问题是 promotion policy 是否合理,不是提前实现 Recorder backend。

### 阶段

- [ ] 阶段 1: 重新验证 frontier,claim ticket,固定 prototype 问题与停止条件
- [ ] 阶段 2: 回读 window/observation/AX/Web/mouse/non-mouse/Journaling 契约,提取可复用 command shape和 hard guard
- [ ] 阶段 3: 在 main 之外的 throwaway worktree/branch构建单命令 logic prototype
- [ ] 阶段 4: 运行五类动作与 ambiguity/stale/dynamic/no-AX scenarios,保存 trace与量化结果
- [ ] 阶段 5: 向 human展示具体结果,一次只确认一个 promotion policy 决策
- [ ] 阶段 6: 根据 verdict固化 resolution asset,关闭 ticket并更新 Wayfinder map

### 约束

- 本 ticket 只做 prototype和决策,不实现 Recorder生产代码。
- Observation ref只能在原 observation内使用;Replay持久化只能用 durable selector或带 window/display guard的坐标。
- Semantic promotion没有足够 ownership、target与freshness证据时必须 fail closed或使用显式 guarded fallback。
- 每个 side effect必须有 fresh post-action evidence;成功 response本身不算证明。
- Prototype必须明确标为 throwaway,单命令可运行,状态保存在内存,最终捕获在 main之外的分支。
- 本轮在 human verdict前不关闭 ticket。

### 停止条件

- 五类动作均有可读 trace。
- 至少覆盖 ambiguity、stale target、dynamic page和 no-AX。
- 输出语义命令、guarded coordinate或拒绝编译的选择及理由。
- 给出可复核的量化汇总。
- 向 human提出一个具体 promotion policy问题。

### 当前状态

**阶段 1 进行中**: live frontier已验证,下一动作是 claim ticket并确认 assignee。

## [2026-07-23 17:36:00] [Session ID: omx-1784512435044-92wxat] [阶段完成]: Ticket 已 claim

- [x] Live frontier确认该 ticket为 open、unblocked、unassigned。
- [x] GitHub assignee已设置为 `raiscui`。
- [x] 问题固定为 promotion policy fidelity,不提前实现 Recorder backend。
- [x] 阶段 1 完成。
- [ ] 下一步回读既有控制与记录规格,提取 prototype scenario矩阵。

**阶段 2 进行中**: 先建立真实协议边界,再设计 trace lab。

## [2026-07-23 17:48:00] [Session ID: omx-1784512435044-92wxat] [阶段完成]: 协议与源码边界收敛

- [x] 回读 window、observation/refmap、display-aware chain、non-mouse、mouse、web、Journal和redaction规格。
- [x] CodeGraph确认现有AX/Web/type-text action路由、observation ref归属和display guard结构。
- [x] 固定semantic、parameterized-semantic、guarded-coordinate、reject四种prototype decision。
- [x] 固定量化口径,不使用无事实依据的confidence score。
- [x] 记录主假设、最强备选和推翻条件。
- [x] 阶段 2 完成。
- [ ] 下一步在throwaway worktree创建单命令logic prototype与scenario fixtures。

**阶段 3 进行中**: Prototype将只使用Python标准库,不引入生产依赖或修改main源码。

## [2026-07-23 16:36:08] [Session ID: omx-1784512435044-92wxat] [继续执行]: 复核 prototype 后完成独立分支交付

- [x] 已收到前序 checkpoint: throwaway worktree、prototype脚本、12个scenario fixture和README均已创建。
- [ ] 重新审查prototype diff,确认没有生产代码或main分支污染。
- [ ] 重跑语法检查、完整scenario state history、JSON汇总与负向fixture。
- [ ] 运行`git diff --check`,确认文件规模与throwaway标识。
- [ ] 仅在`prototype/recording-semantic-promotion`分支commit并push。
- [ ] 在[验证语义提升与坐标 fallback 的可行性](https://github.com/raiscui/rustdog/issues/6)留下非resolution prototype证据。
- [ ] 向human提出单一promotion policy verdict问题;在回答前保持ticket open。

**阶段 3/4 复核中**: 先用当前文件和命令重新生成动态证据,不把交接摘要直接当作最终验证。

## [2026-07-23 16:44:29] [Session ID: omx-1784512435044-92wxat] [阶段完成]: Prototype构建与动态验证完成

- [x] 独立worktree和`prototype/recording-semantic-promotion`分支仅新增3个prototype文件,未修改生产源码。
- [x] 阶段 3 完成: 单命令logic prototype、13个常驻scenario和README已构建。
- [x] 阶段 4 完成: Python语法、JSON语法、完整state history、summary-only和machine JSON输出均通过。
- [x] 最小反例先复现`expected reject, got guarded-coordinate`,证明stale semantic target会错误降级。
- [x] 修正后13个常驻scenario全部匹配,4个独立负向fixture也全部得到`reject`。
- [x] 最终汇总: semantic 5、parameterized-semantic 1、guarded-coordinate 3、reject 4;unsafe fallback prevented 3;persisted observation ref 0。
- [x] `git diff --cached --check`通过;Python文件525行,未超过800行限制。
- [ ] 下一步提交并push throwaway branch,然后向ticket发布非resolution comment。

**阶段 5 交付中**: Human verdict仍未给出,因此只发布prototype证据,保持ticket open。

## [2026-07-23 16:47:52] [Session ID: omx-1784512435044-92wxat] [等待HITL]: Prototype已推送并登记到ticket

- [x] Commit `c0d2e0158df2d8bac4d37ce34dcdc7a66276b994`已推送到`origin/prototype/recording-semantic-promotion`。
- [x] `git ls-remote`确认远端ref与本地HEAD完全一致。
- [x] 已在ticket发布[prototype checkpoint](https://github.com/raiscui/rustdog/issues/6#issuecomment-5056296153),明确不是resolution。
- [x] GitHub复查确认[验证语义提升与坐标 fallback 的可行性](https://github.com/raiscui/rustdog/issues/6)仍为`OPEN`。
- [ ] 阶段 5 等待human确认唯一policy问题。
- [ ] 阶段 6 只有收到verdict后才能固化main规格、发布resolution、关闭ticket并更新Wayfinder map。

**当前停止点**: 不再修改prototype。下一动作由human verdict触发。

## [2026-07-23 16:54:46] [Session ID: omx-1784512435044-92wxat] [HITL确认]: Human要求继续

- [x] 将human在唯一verdict问题后的"继续"记录为接受推荐policy。
- [x] 阶段 5 完成: ambiguous或stale semantic target不自动回退旧坐标;coordinate fallback只在semantic candidate为0且全部coordinate guards/verifier fresh时允许。
- [ ] 核对现有recording specs职责边界,选择唯一正式真相源。
- [ ] 将verdict固化到main规格并同步长期知识索引。
- [ ] 验证文档、提交并push main。
- [ ] 发布resolution comment,关闭当前ticket,更新Wayfinder map的Decisions so far。
- [ ] 复查新的frontier,但本session不解决第二张ticket。

**阶段 6 进行中**: 先确定policy的正确文档归属,避免污染Journal schema或重复既有控制规格。

## [2026-07-23 17:00:57] [Session ID: omx-1784512435044-92wxat] [遇到错误]: Staged review wrapper未启动shell

- [x] 现象: `functions.exec`在解析JavaScript时报告`SyntaxError: Unexpected identifier 'reject'`。
- [x] 原因: command template literal内嵌了Markdown反引号,提前结束JavaScript字符串。
- [x] 影响: nested `exec_command`没有被调用,文件和暂存区没有被该失败命令修改。
- [ ] 修正: 校验脚本改用不含反引号的短语和`chr(96)`构造fence,然后重跑完整staged review。

## [2026-07-23 17:03:23] [Session ID: omx-1784512435044-92wxat] [阶段进展]: 正式policy已写入并通过文档验证

- [x] 新增`specs/rdog-recording-semantic-promotion-policy.md`作为唯一compiler lane选择真相源。
- [x] 更新`CONTEXT.md`中的Semantic Promotion与Guarded Coordinate Fallback术语。
- [x] 更新`AGENTS.md`长期知识索引和Journal spec compiler handoff指针。
- [x] 两个Mermaid block均由`beautiful-mermaid-rs --ascii`成功渲染且输出非空。
- [x] `git diff --cached --check`、10项policy assertion和内部Markdown路径检查全部通过。
- [x] Staged diff仅包含4个正式文档文件,共302行新增、1行删除;policy文件287行。
- [x] 前一条wrapper错误已修正并完成全量重跑。
- [ ] 下一步提交并push main,再更新GitHub resolution与Wayfinder map。

**阶段 6 提交中**: 正文冻结,后续只接受验证失败驱动的修正。

## [2026-07-23 17:04:41] [Session ID: omx-1784512435044-92wxat] [遇到错误]: Push成功后的SSH二次核对断线

- [x] Commit `3de8cd631c9a307910829f42f914f09923596f4d`创建成功。
- [x] Push输出明确显示`56b8069..3de8cd6 main -> main`。
- [ ] 最后的`git ls-remote`因`Connection to github.com closed by remote host`和`Broken pipe`失败,不能作为远端复核证据。
- [ ] 改用GitHub HTTPS API读取`refs/heads/main`,确认remote SHA后再更新ticket。

## [2026-07-23 17:05:41] [Session ID: omx-1784512435044-92wxat] [验证完成]: Main规格已远端确认

- [x] GitHub refs API返回`3de8cd631c9a307910829f42f914f09923596f4d`。
- [x] GitHub commit API返回同一SHA。
- [x] 本地HEAD、remote main和commit API三者一致。
- [x] SSH二次核对错误已由独立HTTPS动态证据收口。
- [ ] 下一步按resolution comment -> close ticket -> update map -> re-query frontier顺序执行。

## [2026-07-23 17:07:55] [Session ID: omx-1784512435044-92wxat] [任务完成]: Semantic Promotion prototype ticket收口

- [x] 阶段 1: frontier复核、ticket claim和停止条件固定。
- [x] 阶段 2: Journal、selector、AX/Web、mouse、window和verification边界核对完成。
- [x] 阶段 3: main之外的throwaway prototype构建并推送。
- [x] 阶段 4: 13个常驻scenario、4个负向fixture和完整state history验证完成。
- [x] 阶段 5: Human接受fail-closed promotion policy。
- [x] 阶段 6: 正式规格、术语、索引和Journal handoff提交到main;resolution comment、ticket close和map decision pointer完成。
- [x] Main commit: `3de8cd631c9a307910829f42f914f09923596f4d`,GitHub refs与commit API已确认。
- [x] Ticket[验证语义提升与坐标 fallback 的可行性](https://github.com/raiscui/rustdog/issues/6)为`CLOSED/COMPLETED`。
- [x] Map为open且已包含decision/policy pointer;新frontier为[定义 Participating Window 与 geometry precondition 编译](https://github.com/raiscui/rustdog/issues/11)和[定义 Recording Bundle schema 与原子导出](https://github.com/raiscui/rustdog/issues/9)。
- [x] `LATER_PLANS__semantic_promotion_prototype.md`无需创建:剩余工作均由现有Wayfinder tickets和fog跟踪。
- [x] EPIPHANY已迁入正式policy并追加解决记录;本session不claim或解决第二张ticket。

**最终状态**: 本支线全部待办完成。

## [2026-07-23 17:09:32] [Session ID: omx-1784512435044-92wxat] [最终验证]: 全部完成条件通过

- [x] 正式文档相对HEAD无未提交差异,暂存区为空。
- [x] Local HEAD与GitHub remote main均为`3de8cd631c9a307910829f42f914f09923596f4d`。
- [x] 当前ticket为`CLOSED/COMPLETED`。
- [x] Wayfinder map为open,decision与policy pointer均存在。
- [x] 默认六文件均未超过1000行;未产生新的延期事项。
