# 任务计划: 定义 Recording Session lifecycle control protocol

## [2026-07-21 08:15:11] [Session ID: omx-1784512435044-92wxat] [任务计划]: Claim 并逐项收敛 lifecycle 契约

### 目标

通过 HITL grilling 固定 Recording Session 的 ownership、start/status/mark/stop/cancel、权限失败、断线与 crash 恢复、原子 finalize、capabilities 和远程 Recording Bundle 交付契约,把完整 resolution 写回 Wayfinder ticket 并更新 map。

### 阶段

- [ ] 阶段 1: Claim ticket,读取现有协议与已关闭 research ticket,固定决策树和停止条件。
- [ ] 阶段 2: 一次一个问题确认 session ownership、命令幂等、状态机、failure/recovery 和 export 语义。
- [ ] 阶段 3: 产出并验证 lifecycle protocol 规格,同步必要 glossary 或既有规格引用。
- [ ] 阶段 4: 发布资产,写 resolution comment,关闭 ticket,更新 map 和新 frontier。
- [ ] 阶段 5: 支线 WORKLOG、风险回溯、主线摘要和最终验证。

### 两个总体方向

1. **最佳方案,推荐**: daemon-owned durable Recording Session。Controller 只是命令发起方;断线不自动结束录制,session 用 `recording_id` 和持久化 journal 恢复,stop/cancel 支持幂等重试。
2. **先能用方案**: connection-scoped ephemeral Recording Session。Controller 断线即自动 cancel,daemon restart 后不恢复 active session,协议和实现较短,但远程网络抖动会直接破坏录制。

### 已知硬约束

- 每个 daemon 同时最多一个 active Recording Session。
- lifecycle 真相源是 control protocol;CLI 只是 wrapper。
- `@record-stop` 负责原子 finalize 和 Recording Bundle export,不自动 replay。
- Recording Journal 是 canonical append-only source,Replay Script 是派生产物。
- 权限与 tap health 分 lane;缺失和 gap 不能静默当作 complete。
- HITL ticket 不允许 agent 代替用户做产品决策,每轮只问一个问题并给推荐答案。

### 决策树

- [ ] Session ownership 和 controller 断线语义。
- [ ] `@record-start` 的 request shape、幂等键、冲突和权限 preflight。
- [ ] `@record-status` 的可观察状态、lane health、progress 和恢复信息。
- [ ] `@record-mark` 的用途、顺序、evidence 触发与幂等。
- [ ] `@record-stop` 的 freeze/finalize/compile/export 原子边界和重试。
- [ ] `@record-cancel` 的保留/丢弃策略与审计边界。
- [ ] daemon crash/restart、残留 active journal 和恢复状态。
- [ ] capabilities、错误码、远程 `@savefile` 和 CLI wrapper。

### 当前状态

**阶段 1 进行中**: 下一动作是 claim `定义 Recording Session lifecycle control protocol`,确认当前 assignee 后开始第一个 ownership 决策。

## [2026-07-21 08:17:11] [Session ID: omx-1784512435044-92wxat] [状态更新]: Claim 已动态复核,进入 ownership grilling

### 已验证状态

- [x] 阶段 1: ticket `#5` 当前为 `OPEN`,标签为 `wayfinder:grilling`,assignee 为 `raiscui`。
- [ ] 阶段 2: 当前只确认 Recording Session ownership 与 controller 断线语义。

### 验证证据

- 命令: `gh issue view 5 --repo raiscui/rustdog --json number,title,state,assignees,labels,url`。
- 关键输出: `state=OPEN`,assignee=`raiscui`,label=`wayfinder:grilling`。

### 当前状态

**阶段 2 进行中**: 等待用户决定 controller 断线时,daemon-owned Recording Session 是继续录制还是立即取消。

## [2026-07-21 08:30:55] [Session ID: omx-1784512435044-92wxat] [决策]: Controller 断线即录制失败

### 已确认契约

- [x] Session ownership 和 controller 断线语义。
- Recording Session 采用 connection-scoped ephemeral ownership,归启动录制的 controller connection 持有。
- 连接关闭、网络中断或 controller 进程退出时,active session 必须立即停止捕获并进入 `failed` 终态。
- 断线失败不能记为正常 `cancelled`,也不能生成声称完整的 Replay Script。
- 断线后不允许新连接通过 `recording_id` 继续原 session;重新录制必须创建新 session。

### 设计影响

- `@record-status` 需要能够区分 `failed`、`cancelled` 与正常 finalized 状态。
- 失败 journal 是否保留、保留多久,在 `@record-cancel` 与 failure policy 决策中继续确认。
- daemon restart 与连接断线遵循同一条失败原则,但残留 journal 的清理和诊断语义稍后单独决定。

### 当前状态

**阶段 2 进行中**: 下一项只确认 `@record-start` 是否使用 client-provided 幂等键。

## [2026-07-21 08:31:51] [Session ID: omx-1784512435044-92wxat] [研究更新]: request id 只做相关性标识

### 静态证据

- `specs/control-line-protocol.md` 明确 request id 可选,只绑定显式协议请求,用于稳定请求-响应关联。
- 当前协议未定义 request id 的幂等语义,Recording Session 不应单独重载其含义。

### 当前状态

**阶段 2 进行中**: 等待用户决定 `recording_id` 是 daemon 生成,还是由 client 提供以承担 start 幂等。

## [2026-07-21 08:59:05] [Session ID: omx-1784512435044-92wxat] [决策]: recording_id 由 daemon 生成

### 已确认契约

- `recording_id` 由 daemon 在 `@record-start` 成功提交时生成。
- `#request_id` 保持现有语义,只用于请求-响应关联,不兼任幂等键。
- active session 存在时,重复 `@record-start` 不创建新 Session,返回 `RECORDING_ALREADY_ACTIVE` 和 active `recording_id`。
- 调用方使用 `@record-status` 收敛当前 Session 状态,不新增 client-provided durable idempotency key。

### 当前状态

**阶段 2 进行中**: 下一项只确认 `@record-start` 的权限 preflight 是全量通过后原子启动,还是允许降级启动部分 capture lane。

## [2026-07-21 10:07:57] [Session ID: omx-1784512435044-92wxat] [决策]: @record-start 严格按 profile 原子启动

### 已确认契约

- 默认 `profile:"semantic"` 要求 `event_listen` 与 `accessibility` 均通过 preflight。
- 显式 `profile:"physical"` 只要求 `event_listen`。
- required lane 缺失时返回 `blocked`,不创建 `recording_id`,不留下 active 或半初始化 Session。
- Screen Recording 只在请求 screenshot evidence 时参与检查,不阻止未请求截图的普通录制。
- 权限提示是异步恢复动作;触发提示后仍返回 blocked,授权完成后由 controller 显式重试 start。

### @record-start 当前收敛状态

- [x] daemon-generated `recording_id`。
- [x] active-session conflict,不创建第二个 Session。
- [x] request id 仅用于相关性。
- [x] profile-strict permission preflight 和原子启动。
- [ ] request payload 的 evidence 配置与默认值在完整 schema 编写时按上述边界收口。

### 当前状态

**阶段 2 进行中**: 下一项只确认 required capture lane 在运行期失效时,Session 是立即失败,还是继续为 incomplete/degraded。

## [2026-07-21 10:15:44] [Session ID: omx-1784512435044-92wxat] [决策]: required lane 运行期失效即失败

### 已确认契约

- 当前 profile 的任一 required lane 失效后,daemon 先执行有界恢复;恢复失败则立即停止捕获并把 Session 置为 `failed`。
- `event_listen` 权限丢失、event tap 有界恢复失败、不可恢复的 required queue gap 都属于 fatal failure。
- `semantic` profile 中 Accessibility 权限被撤销或 required semantic runtime 不可恢复时同样属于 fatal failure。
- failure/gap marker 必须先写入 canonical journal,再停止 capture runtime。
- failed Session 不生成可声称完整的 Replay Script。
- `degraded` 只用于 screenshot evidence 等 optional lane;optional lane 失败不终止 Session。

### 当前状态

**阶段 2 进行中**: 下一项只确认 `@record-status` 是否允许非 owner controller 只读查询 active Session。

## [2026-07-21 10:27:53] [Session ID: omx-1784512435044-92wxat] [决策]: @record-status 对所有 controller 只读可见

### 已确认契约

- 任一已连接到同一 daemon control plane 的 controller 都可调用 `@record-status`。
- 非 owner 可读取 active `recording_id`、profile、state、duration、owner presence、event/gap counters 和分 lane health。
- status 不返回按键值、文本内容、截图或其他录制 payload。
- `@record-mark`、`@record-stop`、`@record-cancel` 仍只允许 owner connection 执行;只读 status 不改变 ownership。
- 非 owner 看到 active Session 后只能等待、诊断或开始自己的新连接流程,不能接管原 Session。

### 当前状态

**阶段 2 进行中**: 下一项只确认 Recording Session 的最小主状态集合,以及 blocked/degraded 是否保持为正交字段。

## [2026-07-21 10:29:00] [Session ID: omx-1784512435044-92wxat] [决策]: 采用最小 Recording Session 状态模型

### 已确认主状态

- `recording`: 捕获已提交并持续写 journal。
- `finalizing`: 输入已冻结,正在编译和原子提交 Recording Bundle;仍占用 single-active slot。
- `completed`: Bundle 已成功提交。
- `failed`: controller 断线或 required lane 故障。
- `cancelled`: owner 主动取消。

### 正交状态

- `idle` 是 recorder-level 状态,表示没有 active Session。
- `blocked` 是没有创建 Session 的 start 结果,不是 Session state。
- `degraded` 是 optional lane health,不是 Session state。
- 首版不提供 `paused`、`resuming` 或跨连接 `recovering`。

### 当前状态

**阶段 2 进行中**: 下一项只确认 recorder 进入 idle 后,`@record-status` 是否保留一个易失的 last terminal summary。

## [2026-07-21 10:31:50] [Session ID: omx-1784512435044-92wxat] [决策]: idle status 保留一条易失终态摘要

### 已确认契约

- recorder 回到 `idle` 后,内存中保留最近一个 Session 的 terminal summary。
- summary 包含 `recording_id`、terminal state、结束时间、failure reason、lane health、counters 和 Bundle commit 状态。
- summary 不包含按键、文本、截图或其他录制 payload。
- 新 Session 成功启动时覆盖旧 summary;daemon 重启时 summary 丢失。
- last terminal summary 只用于诊断,不允许恢复、接管或继续原 Session。

### @record-status 当前收敛状态

- [x] 所有 controller 可只读查询。
- [x] owner-only mutation 不变。
- [x] 最小主状态与正交 health。
- [x] active progress/lane health/counters。
- [x] idle 时返回一个易失 last terminal summary。

### 当前状态

**阶段 2 进行中**: 下一项进入 `@record-mark`,先确认 mark 是否是有顺序保证的 journal barrier。

## [2026-07-21 10:36:25] [Session ID: omx-1784512435044-92wxat] [决策]: @record-mark 是持久化 journal barrier

### 已确认契约

- `@record-mark` 只允许 owner connection 在 `recording` 状态调用。
- daemon 先排空并提交所有已取得 sequence 且位于 mark 之前的 capture events。
- daemon 再向 canonical journal 追加 mark entry,分配 daemon-generated `mark_id` 和确定的 `capture_seq` 边界。
- journal append 成功后才返回成功响应;后续 capture event sequence 必须位于该 mark 之后。
- mark 可作为 Replay Script 分段、截图/AX evidence 和故障诊断的确定性锚点。
- barrier 只同步 recorder queue 与 journal,不在 capture callback 内执行磁盘 IO 或 evidence capture。

### 当前状态

**阶段 2 进行中**: 下一项只确认 mark evidence 是显式 opt-in,还是每个 mark 默认自动采集。

## [2026-07-21 12:20:05] [Session ID: omx-1784512435044-92wxat] [决策]: mark evidence 显式 opt-in

### 已确认契约

- `@record-mark` 默认只提交 barrier、可选 label 和 mark metadata,不隐式截图。
- mark payload 可显式请求 `screenshot`、`ax_snapshot` 等 evidence。
- `@record-start` 可声明 `default_mark_evidence`,后续 mark 未覆盖时继承该策略。
- evidence 在 mark barrier 提交后采集,并通过 `mark_id` 关联。
- optional evidence 失败不回滚已提交 mark,响应分别报告 mark success 与 evidence failure,对应 lane 标记 degraded。
- 未显式请求 evidence 时,mark 不触发 Screen Recording 权限提示。

### 当前状态

**阶段 2 进行中**: 下一项只确认 `@record-mark` 的重试去重语义。

## [2026-07-21 12:27:51] [Session ID: omx-1784512435044-92wxat] [交接检查点]: 等待用户决定 mark dedupe_key

### 当前未决问题

- 尚未确认的推荐方案: `@record-mark` 支持 session-scoped 可选 `dedupe_key`。
- 相同 key + 相同 canonical payload 返回原 `mark_id`;相同 key + 不同 payload 返回 `RECORD_MARK_DEDUPE_CONFLICT`;无 key 时每次创建新 mark。
- 该内容目前只是提案,下一位 agent 必须等待用户明确决定后才能写成已确认契约。

### Ticket 复核

- GitHub REST 复核结果: ticket `#5` 为 `open`,assignee 为 `raiscui`,label 为 `wayfinder:grilling`。
- GraphQL 查询首次遇到 `TLS handshake timeout`;改用 REST endpoint 后成功,不构成仓库或协议错误。

### 当前状态

**阶段 2 进行中,等待 HITL 输入**: 下一轮从 mark 去重问题继续,不要跳到 stop/cancel,也不要一次提出多个问题。

## [2026-07-21 12:31:04] [Session ID: omx-1784512435044-92wxat] [决策]: @record-mark 支持 session-scoped dedupe_key

### 已确认契约

- `dedupe_key` 是 mark payload 中的可选字段,作用域限定为当前 `recording_id`。
- 相同 key 与相同 canonical payload 重试时,返回原 `mark_id`,不重复提交 barrier 或 evidence。
- 相同 key 与不同 canonical payload 冲突时,返回 `RECORD_MARK_DEDUPE_CONFLICT`。
- 未提供 key 时,每次 `@record-mark` 都表示创建新 mark。
- `#request_id` 继续只做请求-响应关联,不承担 mark 去重。
- dedupe mapping 进入 canonical journal,与 mark commit 使用同一持久化边界。

### @record-mark 当前收敛状态

- [x] owner-only mutation。
- [x] 持久化 journal barrier。
- [x] daemon-generated `mark_id` 与 `capture_seq` 边界。
- [x] evidence 显式 opt-in,optional failure 不回滚 mark。
- [x] session-scoped 可选 `dedupe_key`。

### 当前状态

**阶段 2 进行中**: 下一项进入 `@record-stop`,先确认 freeze、compile 与 Bundle commit 的原子边界。

## [2026-07-21 13:59:01] [Session ID: omx-1784512435044-92wxat] [决策]: @record-stop 采用 freeze-and-commit 事务

### 已确认契约

- owner 在 `recording` 状态发出合法 stop 后,daemon 立即建立最终 barrier,冻结新输入并确定 `end_capture_seq`。
- daemon 排空所有已取得 sequence 的事件,写入最终 lane health、gap counters 和 terminal marker。
- Session 随后进入 `finalizing`,在整个编译与提交阶段继续占用 single-active slot。
- Replay Script 只从 frozen canonical journal 派生。
- manifest、journal、script 和 evidence 先在 staging 路径组装并校验。
- 通过同文件系统 atomic rename 提交 Recording Bundle 后,Session 才进入 `completed`。
- freeze、flush、compile、validate 或 Bundle commit 任一步骤失败时进入 `failed`,不得暴露部分 Bundle 为完成品。

### 当前状态

**阶段 2 进行中**: 下一项只确认本地 Bundle 已原子提交后,远程 `@savefile` 交付失败是否改变 `completed` 终态。

## [2026-07-21 13:59:56] [Session ID: omx-1784512435044-92wxat] [研究更新]: Bundle commit 与 @savefile request 收口可分离

### 静态证据

- 现有 line-control 文件型结果使用 `@savefile* -> @response` 多 frame 收口。
- 本地 Bundle atomic commit 发生在 frame 生成前;远程连接可能在 commit 后、最终 response 前断开。

### 当前状态

**阶段 2 进行中**: 等待用户决定 post-commit delivery failure 的状态语义。

## [2026-07-21 14:01:37] [Session ID: omx-1784512435044-92wxat] [决策]: post-commit delivery failure 不回滚 completed

### 已确认契约

- controller 在 Bundle atomic commit 前断线,Session 进入 `failed`。
- Bundle atomic commit 成功后,Session 进入不可逆的 `completed` 终态。
- commit 后 `@savefile` 交付中断不改变 Session state,只把 `delivery_status` 记为 `partial` 或 `failed`。
- 已提交 Bundle 保留在 daemon 本地,不能因传输失败而否认其完整性。
- status terminal summary 同时报告 `bundle_committed:true` 与独立 delivery 状态。

### 当前状态

**阶段 2 进行中**: 下一项只确认是否复用幂等 `@record-stop` 重放已提交 Bundle 的 `@savefile` frames。

## [2026-07-21 14:30:01] [Session ID: omx-1784512435044-92wxat] [决策]: 幂等 @record-stop 兼作 committed Bundle delivery retry

### 已确认契约

- `recording` 状态下的 `@record-stop` 是 owner-only mutation,执行唯一 freeze-and-commit 事务。
- 指定 `recording_id` 已为 `completed` 时,重复 `@record-stop` 是只读 delivery retry。
- terminal readback 直接重放已提交 Bundle 的 `@savefile` frames,不重新编译、不修改 manifest 或 Bundle。
- 新 controller 可凭精确 `recording_id` 请求 completed Bundle,但不能恢复或接管 Session。
- 响应携带 Bundle checksum,多次重传必须对应同一 committed bytes。
- Bundle 已按 retention policy 删除时返回 `RECORD_BUNDLE_NOT_FOUND`。
- failed 或 cancelled Session 不允许通过 stop 伪造 completed Bundle。

### @record-stop 当前收敛状态

- [x] final barrier 与 `end_capture_seq`。
- [x] freeze-and-commit transaction。
- [x] atomic Bundle commit 才进入 completed。
- [x] post-commit delivery failure 正交报告。
- [x] completed Bundle 的幂等 delivery retry。

### 当前状态

**阶段 2 进行中**: 下一项进入 `@record-cancel`,先确认取消时是否丢弃全部未提交 capture artifacts。

## [2026-07-21 15:25:25] [Session ID: omx-1784512435044-92wxat] [决策]: @record-cancel 采用 privacy-first discard

### 已确认契约

- `@record-cancel` 是 owner-only mutation,用于主动放弃未完成录制。
- cancel 立即停止捕获,不编译 Replay Script,不提交 Recording Bundle。
- daemon 删除未提交 journal、evidence 和 staging artifacts。
- 仅在易失 last terminal summary 中保留 `recording_id`、`cancelled`、时间和非内容 counters。
- 首版不提供 `keep_partial` 或 `retain_diagnostics`。
- 需要保留捕获内容时必须使用 `@record-stop`,不能使用 cancel。
- 删除保证限定为正常 unlink 与目录清理,不宣称在 SSD/APFS 上完成物理安全擦除。

### 当前状态

**阶段 2 进行中**: 下一项只确认 Session 进入 `finalizing` 后是否仍允许 cancel 中断 Bundle commit。

## [2026-07-21 15:26:38] [Session ID: omx-1784512435044-92wxat] [决策]: finalizing 阶段拒绝 cancel

### 已确认契约

- `@record-cancel` 只接受 owner 持有且 state 为 `recording` 的 Session。
- stop 建立最终 barrier 并转入 `finalizing` 后,commit transaction 不可切换到 discard 路径。
- `finalizing` 时调用 cancel 返回 `RECORD_FINALIZING`,不改变正在执行的事务。
- finalization 自身失败时进入 `failed`,不伪装成 `cancelled`。
- 已为 `cancelled` 的同一 `recording_id` 重复 cancel 返回原 terminal summary,不重复执行清理。

### @record-cancel 当前收敛状态

- [x] owner-only,仅 recording state。
- [x] privacy-first discard。
- [x] 不生成 partial Bundle。
- [x] finalizing 后不可取消。
- [x] terminal retry 幂等。

### 当前状态

**阶段 2 进行中**: 下一项进入 daemon crash/restart,先确认不恢复 active Session,只做 committed-vs-incomplete 判定。

## [2026-07-21 17:35:13] [Session ID: omx-1784512435044-92wxat] [决策]: daemon restart 不恢复 active Recording Session

### 已确认契约

- daemon 启动时扫描 recording storage,但绝不恢复 crash 前的 active capture lifecycle。
- 已完成 atomic rename 且 manifest/checksum 验证通过的 Bundle 视为 `completed`,可继续通过幂等 stop 交付。
- 只有 active journal、staging 目录、临时 manifest 或 checksum 不完整的记录视为 crash orphan。
- crash orphan 不得继续录制,也不得自动编译 Replay Script。
- daemon 启动后的 recorder state 为 `idle`,原 controller ownership 永久失效。
- 新录制必须生成新的 `recording_id`。

### 当前状态

**阶段 2 进行中**: 下一项只确认 crash orphan raw artifacts 的清理与 fail-closed 策略。

## [2026-07-21 17:58:52] [Session ID: omx-1784512435044-92wxat] [校正与决策]: 去重 restart 记录并确认 crash orphan cleanup

### 记录校正

- `15:28:16` 与 `17:35:13` 的 restart 决策内容完全相同,后者是继续流程时产生的冗余副本。
- 为遵守 append-only 规则,不删除历史记录;后续只把该决定计为一次。

### 已确认契约

- daemon startup 从 crash orphan header 提取非敏感 metadata 并写结构化日志,不记录按键、文本或截图。
- daemon 删除残留 journal、evidence、temporary manifest 和 staging 目录,不保留 orphan 供恢复或取证。
- 清理成功后才允许启动新 Recording Session。
- raw artifact 清理失败时,Recorder availability 标记为 `cleanup_required`。
- `@record-start` 在该状态返回 `RECORD_ORPHAN_CLEANUP_FAILED`,直到显式 doctor/recovery 清理成功。
- cleanup 失败只阻塞 Recorder capability,不伪装成 active 或 recoverable Session。

### 当前状态

**阶段 2 进行中**: 下一项只确认 completed Bundle 的默认 retention policy。

## [2026-07-21 15:28:16] [Session ID: omx-1784512435044-92wxat] [决策]: daemon restart 不恢复 active Recording Session

### 已确认契约

- daemon 启动时扫描 recording storage,但绝不恢复 crash 前的 active capture lifecycle。
- 已完成 atomic rename 且 manifest/checksum 验证通过的 Bundle 视为 `completed`,可继续通过幂等 stop 交付。
- 只有 active journal、staging 目录、临时 manifest 或 checksum 不完整的记录视为 crash orphan。
- crash orphan 不得继续录制,也不得自动编译 Replay Script。
- daemon 启动后的 recorder state 为 `idle`,原 controller ownership 永久失效。
- 新录制必须生成新的 `recording_id`。

### 当前状态

**阶段 2 进行中**: 下一项只确认 crash orphan raw artifacts 的清理与 fail-closed 策略。

## [2026-07-21 18:20:10] [Session ID: omx-1784512435044-92wxat] [最终校正与决策]: append-only 锚点修复及 Bundle retention

### 最终校正

- `17:58:52` 的校正记录因 patch 上下文重复,插入在 `15:28:16` 冗余区块之前,没有成为真正 EOF。
- 本记录位于实际文件末尾,是后续恢复状态的最终锚点。
- restart 决策只计一次;crash orphan cleanup 决策有效且已确认。

### 已确认 Bundle retention 契约

- completed Bundle 是用户通过 stop 明确生成的正式资产,默认持久保留到显式删除。
- 未配置 retention limits 时,daemon 不自动删除 completed Bundle。
- 常规配置写入 `config.toml`,可选限制为 `max_age_days`、`max_bundles` 和 `max_total_bytes`。
- 自动清理只选择最旧的 completed Bundle。
- active、finalizing、staging 和正在 delivery 的 Bundle 不参与清理。
- Bundle 删除后再次 delivery retry 返回 `RECORD_BUNDLE_NOT_FOUND`。

### 当前状态

**阶段 2 进行中**: 下一项只确认 `@record-start` 的系统权限 prompt 是否显式 opt-in。

## [2026-07-21 19:59:08] [Session ID: omx-1784512435044-92wxat] [决策]: @record-start 默认主动请求缺失权限

### 已确认契约

- 用户显式调用 `@record-start` 时,daemon 先对当前 profile 的 required permissions 做无弹窗 preflight。
- required permission 缺失时,daemon 默认主动调用对应系统 request API 弹出权限窗口。
- 每个 start request 对每个缺失 permission 最多发起一次 prompt,禁止后台循环弹窗。
- 系统 prompt 是异步恢复动作,本次 start 仍返回 `blocked`,不创建 `recording_id`。
- 响应包含 `prompt_requested:true`、当前 permission states、recovery actions 和是否需要重启 daemon。
- 用户授权并完成必要重启后,controller 必须重新调用 `@record-start`。
- payload 可显式设置 `request_permissions:false`,供 headless automation 只做 preflight,但默认值为 `true`。

### 当前状态

**阶段 2 进行中**: 下一项只确认 CLI wrapper 收到 Ctrl-C 时是 stop-and-save,还是 cancel-and-discard。

## [2026-07-21 20:12:04] [Session ID: omx-1784512435044-92wxat] [阶段完成]: lifecycle grilling 收口

### 已确认 CLI 契约

- `rdog record` 第一次收到 Ctrl-C 时发送 `@record-stop`,不直接断开连接。
- CLI 显示 `finalizing`,等待 Bundle commit 和 `@savefile` delivery,成功后退出码为 `0`。
- discard 必须使用显式 cancel 操作,普通 Ctrl-C 不等于 cancel。
- 进程强杀、网络断开或连接意外关闭仍按 Recording Session failure 处理。
- `finalizing` 期间再次 Ctrl-C 不切换为 cancel。

### 阶段状态

- [x] 阶段 1: claim、研究入口和决策树。
- [x] 阶段 2: ownership、start/status/mark/stop/cancel、failure/restart、retention、权限 prompt 和 CLI wrapper。
- [ ] 阶段 3: 编写并验证正式 lifecycle protocol 规格。
- [ ] 阶段 4: 发布 resolution、关闭 ticket、更新 map 和 frontier。
- [ ] 阶段 5: 支线记录收口和最终验证。

### 当前状态

**阶段 3 进行中**: 按已确认决定编写最小规格,不继续扩展未要求的 lifecycle 功能。

## [2026-07-21 20:17:00] [Session ID: omx-1784512435044-92wxat] [行动计划]: 编写 lifecycle spec 并验证

### 规格边界

- 新建 `specs/rdog-recording-session-lifecycle.md`,作为 ticket resolution asset。
- 只定义五个 lifecycle command、状态机、ownership、权限、失败/重启、delivery、retention 和最小 CLI wrapper。
- Replay Script 直接引用 `rdog.flow.v1`;窗口恢复直接引用现有 `@window-resize`;不复制 Journal、Bundle 或 replay policy ticket 的职责。
- 更新 `AGENTS.md` 长期知识索引,让实现 lifecycle 或 Recorder runtime 前可发现该规格。

### Tracker 证据

- 当前 ticket 只阻塞 Recording Bundle schema 和最终验收矩阵。
- 当前 ticket 关闭后,唯一 unblocked/unclaimed frontier 是 `定义 rdog.recording.v1 Recording Journal 模型`。

### 遇到错误

- 一次 `rg` 检索因双引号内含反引号触发 zsh `unmatched quote`;已改用单引号重试成功,不影响仓库内容。

### 当前状态

**阶段 3 进行中**: 下一动作是创建规格和索引,随后运行 Mermaid、Markdown 与 diff 验证。

## [2026-07-21 20:26:49] [Session ID: omx-1784512435044-92wxat] [恢复与行动计划]: 收口 tracker 和支线上下文

### 已验证状态

- [x] 阶段 1: claim、研究入口和决策树。
- [x] 阶段 2: lifecycle 契约与权限 prompt 决策。
- [x] 阶段 3: 正式规格、长期索引、Mermaid 与 Markdown 验证。
- [ ] 阶段 4: 发布简短 resolution、关闭 ticket、更新 map 并重新查询 frontier。
- [ ] 阶段 5: 写入支线 WORKLOG、提交支线上下文、推送并复核远端。

### 约束

- 不继续新增 lifecycle 命令、状态或抽象。
- map 只追加本 ticket 的一行 context pointer,不复制完整规格。
- 不清除 Evidence retention fog,因为 Bundle 内部 evidence policy 仍属于后续 Bundle ticket。
- 不暂存默认 `WORKLOG.md`、`notes.md`、`task_plan.md` 的既有改动。

### 当前状态

**阶段 4 进行中**: 先读取 map 和 ticket 的当前远端状态,再执行一次性 tracker 收口。

## [2026-07-21 20:29:51] [Session ID: omx-1784512435044-92wxat] [阶段完成]: tracker 收口完成

### 完成项

- [x] 在 ticket 发布简短 resolution comment,链接固定 commit 中的正式规格。
- [x] 关闭 `定义 Recording Session lifecycle control protocol`。
- [x] 在 map 的 Decisions so far 追加一行 context pointer。
- [x] 保留 Evidence retention fog,未提前代替 Bundle schema ticket 做决定。
- [x] 重新查询 GitHub 原生 sub-issue dependency graph。

### 动态证据

- Resolution comment: `https://github.com/raiscui/rustdog/issues/5#issuecomment-5033989227`。
- Ticket 状态: `CLOSED`,关闭时间为 `2026-07-21T12:28:37Z`。
- Map 状态: `OPEN`,lifecycle pointer 已位于 Decisions so far。
- Frontier: `定义 rdog.recording.v1 Recording Journal 模型` 为唯一 open、unassigned 且 blocker 全部 closed 的子票。

### 当前状态

**阶段 5 进行中**: 创建支线 WORKLOG,仅提交四个支线上下文文件,推送并完成最终远端/工作树复核。
