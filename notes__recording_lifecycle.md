## [2026-07-21 08:30:55] [Session ID: omx-1784512435044-92wxat] 笔记: Session ownership 与断线失败语义

### 用户决定

- 选择简化方案: controller 断线就是录制失败。

### 协议解释

- Session 生命周期绑定启动它的 controller connection。
- 断线触发捕获停止和 `failed` 终态,不是用户主动 `@record-cancel` 所产生的 `cancelled` 终态。
- 失败录制不能输出声称完整的 Replay Script,避免把事件缺口伪装成可可靠回放的资产。
- 该选择主动放弃跨连接恢复,换取更简单、可解释的 ownership 和资源回收模型。

### 待确认

- 失败 journal 的保留与清理策略。
- `@record-start` 的幂等、冲突与权限 preflight 语义。

## [2026-07-21 08:31:51] [Session ID: omx-1784512435044-92wxat] 笔记: request id 与 start 幂等边界

### 现有协议事实

- line-control 的 `#request_id` 是可选无符号整数。
- 它只绑定显式协议请求,用于稳定的请求-响应关联。
- 现有规格没有把 request id 定义为幂等键,因此 Recording Session 不应暗中改变其全局语义。

### 当前推荐

- `recording_id` 由 daemon 在成功 start 时生成,作为 Session identity。
- `#request_id` 继续只做相关性标识。
- active session 存在时,重复 start 返回结构化冲突并携带 active `recording_id`;调用方可用 `@record-status` 收敛状态。
- connection 断开即失败后,不存在跨连接恢复原 start 的合法路径,没有必要新增 durable client idempotency key。

## [2026-07-21 08:59:05] [Session ID: omx-1784512435044-92wxat] 笔记: start identity 决策

### 用户决定

- 接受 daemon-generated `recording_id` 方案。

### 结果

- Session identity 与 request correlation 保持正交。
- 重复 start 通过 active-session conflict + status query 收敛,不依赖隐藏的重放缓存。

## [2026-07-21 08:59:44] [Session ID: omx-1784512435044-92wxat] 笔记: start 权限 preflight 证据

### 研究原文边界

- `event_listen` 是物理录制必需 lane,缺失时不能启动有效的键鼠 journal。
- `accessibility` 是默认语义 profile 必需 lane,但运行中撤销时物理 lane 技术上仍可继续并把 semantic lane 标为 degraded。
- Screen Recording 只服务显式 screenshot evidence,不是键鼠 journal 的启动条件。
- AX 权限提示是异步的,不能在触发 prompt 后立即把 start 视为成功。

### 待决策

- start 时严格按声明 profile 检查全部 required lane,还是缺少 AX 时静默降级为 physical-only。
- 无论采用哪种策略,Screen Recording 都只在请求 screenshot evidence 时参与检查。

## [2026-07-21 10:07:57] [Session ID: omx-1784512435044-92wxat] 笔记: profile-strict start 决策

### 用户决定

- 接受严格按声明 profile 完成 permission preflight 后再原子启动。

### 结果

- start 成功响应是 Session 已真实进入 recording 状态的提交点。
- blocked start 没有 `recording_id`,因此不会污染单 active session 约束。
- 想在缺少 AX 时录制必须显式选择 physical profile,不能把 semantic 请求静默降级。

## [2026-07-21 10:15:44] [Session ID: omx-1784512435044-92wxat] 笔记: 运行期 required lane failure

### 用户决定

- 接受 required lane 有界恢复失败后立即结束录制并进入 `failed`。

### 状态机不变量

- `completed` 只代表 required lane 没有不可恢复缺口。
- `degraded` 只表达 optional lane 异常,不能覆盖 required event gap。
- fatal failure 仍保留 journal 中的故障证据,但 journal 的长期保留策略稍后决定。

## [2026-07-21 10:27:53] [Session ID: omx-1784512435044-92wxat] 笔记: status 跨连接只读可见

### 用户决定

- 允许非 owner controller 查询 active Recording Session 的结构化状态。

### 安全与 ownership 边界

- status 只暴露 identity、健康度和计数,不暴露捕获内容。
- mutation 命令继续绑定 owner connection。
- 可观察性不能被解释为接管能力;owner 断线仍使原 Session 失败。

## [2026-07-21 10:29:00] [Session ID: omx-1784512435044-92wxat] 笔记: 最小状态模型

### 用户决定

- 接受 `recording -> finalizing -> completed` 正常路径,以及 `failed`、`cancelled` 终态。

### 建模边界

- `blocked` 没有 `recording_id`,不能伪装成 Session。
- `degraded` 只属于 lane health,避免产生 `recording_degraded`、`finalizing_degraded` 等组合状态。
- `finalizing` 继续占用 active slot,防止 Bundle 提交期间启动下一次录制。

## [2026-07-21 10:31:50] [Session ID: omx-1784512435044-92wxat] 笔记: idle last terminal summary

### 用户决定

- 接受 daemon 内存只保留最近一次 terminal summary。

### 结果

- 新 controller 可以诊断最近一次断线或 lane failure。
- 该摘要不持久化,不改变 connection-scoped ephemeral ownership。
- journal 与 Bundle 的磁盘保留仍由后续 cancel/failure policy 决定。

## [2026-07-21 10:36:25] [Session ID: omx-1784512435044-92wxat] 笔记: mark journal barrier

### 用户决定

- 接受 `@record-mark` 作为有持久化确认的顺序屏障。

### 不变量

- mark 成功响应证明前序 event 与 mark entry 已进入 canonical journal。
- `mark_id` 和 `capture_seq` 由 daemon 分配,避免 controller 自造顺序真相源。
- evidence capture 在 barrier 之后执行并关联 mark,不能阻塞 event tap callback。

## [2026-07-21 12:20:05] [Session ID: omx-1784512435044-92wxat] 笔记: mark evidence opt-in

### 用户决定

- 接受 mark 默认无 evidence,通过 mark payload 或 start default 显式开启。

### 结果

- mark journal commit 与 evidence acquisition 是两个可独立报告的结果。
- optional evidence failure 不改变 Session 主状态。
- Screen Recording 权限不会因普通 mark 被隐式触发。

## [2026-07-21 12:31:04] [Session ID: omx-1784512435044-92wxat] 笔记: mark dedupe_key

### 用户决定

- 用户以“继续”确认上一项推荐,接受 session-scoped 可选 `dedupe_key`。

### 结果

- daemon-generated `mark_id` 仍是唯一 mark identity。
- dedupe key 只表达 controller 对一次 mark mutation 的重试意图。
- canonical payload 不一致时拒绝复用,避免相同 key 静默指向不同证据或 label。

## [2026-07-21 13:59:01] [Session ID: omx-1784512435044-92wxat] 笔记: stop freeze-and-commit

### 用户决定

- 接受 stop 从最终 barrier 到 Bundle atomic rename 的事务边界。

### 结果

- `end_capture_seq` 是录制内容截止点,不受后续编译耗时影响。
- `completed` 的真相源是本地 Bundle 原子提交,不是 staging 文件存在。
- 远程 frame 交付是否属于 completed 边界仍需单独确认。

## [2026-07-21 13:59:56] [Session ID: omx-1784512435044-92wxat] 笔记: @savefile 现有返回链路

### 静态证据

- `specs/control-line-protocol.md` 规定,文件型结果可以先返回一个或多个 `@savefile` frame,再由最终 `@response` 收口本次请求。
- 因此本地 Bundle commit、远程 frame delivery 和 request completion 是三个可以分别观察的事实。

### 待决策

- controller 在本地 commit 前断线,已确认应导致 Recording Session failed。
- controller 在本地 commit 后、`@savefile` 交付完成前断线,应保持 completed + delivery failed,还是把整个 Session 判为 failed。

## [2026-07-21 14:01:37] [Session ID: omx-1784512435044-92wxat] 笔记: completed 与 delivery status 分离

### 用户决定

- 接受 Bundle commit 后 Session 保持 completed,远程交付失败单独报告。

### 结果

- Session state 表达录制资产是否完整提交。
- delivery status 表达当前 controller 是否完整收到 outbound frames。
- completed 不允许因后续网络问题转回 failed。

## [2026-07-21 14:30:01] [Session ID: omx-1784512435044-92wxat] 笔记: stop terminal readback

### 用户决定

- 接受复用幂等 `@record-stop` 重放已提交 Bundle,不新增 `@record-export`。

### 结果

- active state 下 stop 仍是 owner mutation。
- completed state 下 stop 只读取 immutable Bundle,因此可由新 controller 使用精确 recording id 重试交付。
- checksum 是多次 delivery 是否来自同一 committed asset 的验证依据。

## [2026-07-21 15:25:25] [Session ID: omx-1784512435044-92wxat] 笔记: cancel privacy-first discard

### 用户决定

- 接受 cancel 删除全部未提交 capture artifacts,不产出 partial Bundle。

### 结果

- stop 表达“提交资产”,cancel 表达“放弃资产”,两条路径不可混用。
- cancelled terminal summary 不包含录制内容。
- 协议不会对 copy-on-write 文件系统和 SSD 做无法验证的 secure erase 承诺。

## [2026-07-21 15:26:38] [Session ID: omx-1784512435044-92wxat] 笔记: finalizing 不可取消

### 用户决定

- 接受 stop 最终 barrier 之后拒绝 cancel。

### 结果

- finalizing 不会与 discard 清理并发。
- `RECORD_FINALIZING` 明确表示 stop transaction 已超过可取消边界。
- 重复 cancel 只返回已有 cancelled summary,保持幂等。

## [2026-07-21 17:35:13] [Session ID: omx-1784512435044-92wxat] 笔记: crash/restart 不恢复 active Session

### 用户决定

- 接受 daemon restart 只识别已提交 Bundle,不恢复任何 incomplete Session。

### 结果

- atomic rename + manifest/checksum 是重启后 completed 判定的唯一证据。
- 残留 journal 或 staging 不足以证明完成,也不能成为自动编译输入。
- restart 后 recorder 回到 idle,后续 recording id 必须重新生成。

## [2026-07-21 17:58:52] [Session ID: omx-1784512435044-92wxat] 笔记: restart 记录校正与 orphan cleanup

### 校正

- restart 决定因继续流程发生重复落盘;两条内容一致,只按一次决定处理。

### 用户决定

- 接受 crash orphan privacy-first cleanup + fail-closed。

### 结果

- incomplete raw capture 不跨 daemon restart 保留。
- cleanup failure 暴露为 Recorder availability 问题并阻止新 start。
- 结构化日志只保留非敏感 metadata,不保留原始输入内容。

## [2026-07-21 15:28:16] [Session ID: omx-1784512435044-92wxat] 笔记: crash/restart 不恢复 active Session

### 用户决定

- 接受 daemon restart 只识别已提交 Bundle,不恢复任何 incomplete Session。

### 结果

- atomic rename + manifest/checksum 是重启后 completed 判定的唯一证据。
- 残留 journal 或 staging 不足以证明完成,也不能成为自动编译输入。
- restart 后 recorder 回到 idle,后续 recording id 必须重新生成。

## [2026-07-21 18:20:10] [Session ID: omx-1784512435044-92wxat] 笔记: 最终校正与 Bundle retention

### 校正

- 本记录是实际 EOF 锚点;restart 重复项只代表一次决定,`17:58:52` 的 orphan cleanup 决定保持有效。

### 用户决定

- 接受 completed Bundle 默认持久保留,只在显式删除或配置 retention limits 时清理。

### 结果

- delivery retry 不会因隐式短 TTL 失效。
- 自动清理排除 active、finalizing、staging 和 in-flight delivery assets。
- retention 使用 `config.toml`,不使用环境变量。

## [2026-07-21 19:59:08] [Session ID: omx-1784512435044-92wxat] 笔记: start 主动权限 prompt

### 用户决定

- 不采用默认静默 preflight;显式启动录制时应主动弹出缺失的系统权限窗口。

### 结果

- prompt 前仍先 preflight,已授权权限不会重复请求。
- prompt 不改变本次 start 的 blocked 结果,TCC 授权后必须重试。
- 每个 start 对每项缺失权限只请求一次,避免循环弹窗。

## [2026-07-21 20:12:04] [Session ID: omx-1784512435044-92wxat] 笔记: CLI Ctrl-C 与 grilling 收口

### 用户决定

- 接受第一次 Ctrl-C 执行 stop-and-save,并要求后续避免过度设计。

### 规格收口原则

- 只定义 `@record-start`、`@record-status`、`@record-mark`、`@record-stop`、`@record-cancel` 和 `rdog record` wrapper。
- 不加入 pause/resume、partial Bundle、跨连接 active recovery 或新的 export command。
- 低层字段按现有 line-control 和 `@savefile` 结构复用,不再为可推导细节追加 HITL 问题。

## [2026-07-21 20:17:00] [Session ID: omx-1784512435044-92wxat] 笔记: lifecycle spec 现有协议依赖

### 复用边界

- `specs/rdog-flow-control-plan.md`: Replay Script 使用 `schema:"rdog.flow.v1"`,不新增第二套 runner。
- `specs/rdog-window-control-plan.md`: 固定窗口几何使用显式 `@window-resize`,默认 verify,坐标为 `os-logical` outer frame。
- `specs/rdog-observation-scoped-refmap-plan.md`: observation ref 只在原 observation 有效,跨观察持久定位必须使用 selector。
- `specs/rdog-non-mouse-semantic-control-plan.md`: 编译动作优先 AX/value/targeted semantic lane,mouse 是显式 fallback。
- `specs/rdog-mouse-control-coordinate-plan.md`: 坐标 fallback 统一使用 screenshot manifest 的 `os-logical`。
- `specs/control-line-protocol.md`: request id 只做相关性;文件型结果按 `@savefile* -> @response` 收口;数字错误码复用 `64/70/77/78`。

### Frontier

- GitHub dependency 查询确认当前 ticket 关闭后,下一 frontier 是 `定义 rdog.recording.v1 Recording Journal 模型`。

## [2026-07-21 20:26:49] [Session ID: omx-1784512435044-92wxat] 笔记: tracker 收口前实时复核

### 远端事实

- map `录制操作并生成可回放的 rdog control 脚本` 仍为 open,Decisions so far 目前只有 macOS capture research 一项。
- ticket `定义 Recording Session lifecycle control protocol` 仍为 open,assignee 为 `raiscui`,且没有任何 comment。
- commit `9045146` 同时位于本地 `main` 与 `origin/main`,包含正式 lifecycle 规格和 `AGENTS.md` 索引。
- ticket `定义 rdog.recording.v1 Recording Journal 模型` 仍为 open 且未分配。

### 收口边界

- resolution comment 只摘要 lifecycle 决定并链接固定 commit,不复制 382 行规格。
- map 只追加一行 closed-ticket pointer。
- `Evidence retention、bundle size 和清理策略` 继续留在 fog,等待 Recording Bundle schema ticket。

## [2026-07-21 20:29:51] [Session ID: omx-1784512435044-92wxat] 笔记: tracker 收口与 frontier 动态证据

### Resolution 与 map

- Resolution comment 已发布: `https://github.com/raiscui/rustdog/issues/5#issuecomment-5033989227`。
- ticket 状态经 `gh issue view` 复核为 `CLOSED`。
- map 的 Decisions so far 已包含 lifecycle ticket pointer。
- map 的三项 fog 均保留,包括 Evidence retention、bundle size 和清理策略。

### Dependency graph

- 使用 GitHub GraphQL `subIssues` 和每个 issue 的 `blockedBy` 实时查询 map 子票。
- `定义 rdog.recording.v1 Recording Journal 模型` 仍为 open、unassigned,唯一 blocker `调研 macOS 全局操作捕获与权限生命周期` 已 closed。
- 其余 open 子票至少有一个 open blocker,因此不属于当前 frontier。
- 已验证结论: 下一 frontier 唯一为 `定义 rdog.recording.v1 Recording Journal 模型`。
