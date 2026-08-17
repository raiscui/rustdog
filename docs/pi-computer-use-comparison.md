# pi-computer-use 对 rustdog 的可取之处

## 结论

`pi-computer-use` 最值得 rustdog 吸收的不是工具名称,也不是整套 Pi extension。
真正有价值的是 4 个执行约束:

1. live GUI 操作按物理资源排队,mutation 用单调递增 epoch 在 dispatch 前拒绝陈旧写入。
2. action 返回完整 successor observation,并用语义 postcondition 决定 `worked/didnt/unknown`。
3. 同一目标上的依赖动作形成 checked transaction,保留 focus,首错停止并返回停止位置。
4. daemon 保存完整 observation,对 agent 只返回有界视图;可信时返回 diff,不可信时回退完整视图。

这些约束应改良 rustdog 已有 `@observe`、`@computer-act`、`@flow` 和 artifact 路径。
不应新增一套 `find_roots/observe_ui/act_ui` 同义协议。

## 审阅范围

- 上游仓库: [injaneity/pi-computer-use](https://github.com/injaneity/pi-computer-use)
- 固定 commit: [`5e1fab8102ee18e3cf83499bce48a171a0ff5c87`](https://github.com/injaneity/pi-computer-use/tree/5e1fab8102ee18e3cf83499bce48a171a0ff5c87)
- 上游版本: `0.5.0`
- rustdog: 2026-08-17 当前工作树。P1 fixture、transaction、output budget 和
  strict-background flow policy 已有定向代码验证,尚未以 live GUI matrix 认证。

动态检查结果:

| 检查 | 结果 | 关键输出 |
| --- | --- | --- |
| `node scripts/check-tool-schemas.mjs` | 通过 | `Tool schema compatibility checks passed.` |
| `pnpm dlx tsx scripts/check-runtime-concurrency.mjs` | 通过 | `Runtime concurrency checks passed.` |
| `pnpm dlx tsx scripts/check-bounded-output.mjs` | 通过 | `Bounded output and ranked search checks passed.` |
| `pnpm run typecheck` | 未完成 | 临时 clone 的 `/tmp` -> `/private/tmp` 规范化使 pnpm peer symlink 失配,不是源码类型错误结论 |
| `pnpm run test:invariants` | 未完成 | 脚本内部依赖 `npx tsc`,受同一依赖装配问题阻断 |

## 能力对比

| 主题 | pi-computer-use | rustdog 当前状态 | 判断 |
| --- | --- | --- | --- |
| ref 所有权 | `@e` 必须携带拥有它的 `stateId` | `@e` 已绑定 `observation_id`,有 TTL、容量和过期错误 | 已有核心语义,不新增第二套 state id |
| stale write | 每个物理资源有递增 epoch;写前比较并先递增 | 创建时间 epoch 继续验证 observation 身份;daemon 已为 PID ref 保存 capture-start resource epoch,同 PID mutation 串行并 fail closed | P0 第一阶段已吸收 |
| action outcome | delivery 与 semantic outcome 分离,三态结果 | `@computer-act` 已有 `worked/didnt/unknown` 和三态 verification | 已吸收,应推广到共享 mutation contract |
| postcondition | `act_ui.expect` 在同一事务内等待语义条件 | `@computer-act` 已支持 AX `exists/not_exists`;`@flow Expect` 与 `@web-act` 保留各自层级语义 | P0 已吸收 |
| 动作批处理 | 同一资源 checked transaction,首错停止 | `@flow.GuiTransaction` 复用 `@computer-act`,首 action real ref/epoch,后续消费 successor | P1 已吸收 |
| successor state | 每次 mutation 产生完整新 state | 多数 action 返回 report,验证常需额外 fresh read | 值得吸收 |
| changes-first | 可信 identity 时返回 added/updated/removed | fixture prototype 复用 `rdog ax-diff`;同窗口 stable-id 配对率低于 75% 时回退 full | P1 prototype 已吸收 |
| cached progressive query | search/expand/inspect 查询服务端完整树 | 已有 selector/ref store,但通用树查询仍偏 live command | 值得做小型 prototype |
| 输出硬边界 | 48 KiB/2000 行 + UTF-8 continuation | 所有 model-visible response 在 frame boundary 统一限制;超限返 preview/digest/查询提示 | P1 已吸收,无 session-local continuation |
| background policy | `headless` 禁止 focus/raw input/overlay | `@flow.policy.execution.strict_background` 在 dispatch 前拒绝 foreground/raw input | P2 已吸收,不照抄 env |

## P0: 资源调度与真实写 epoch

上游的 `StateStore` 保存 `stateId/resourceKey/epoch`,而 `ResourceScheduler` 只串行同一资源。
不同资源仍可并发。mutation 在 native dispatch 前先把 epoch 加一,所以即使执行发生部分副作用或结果不确定,后续旧状态写入仍会失败。

源码证据:

- [StateStore 与 ResourceScheduler](https://github.com/injaneity/pi-computer-use/blob/5e1fab8102ee18e3cf83499bce48a171a0ff5c87/src/runtime.ts#L3-L128)
- [desktop mutation 使用 resource write lane](https://github.com/injaneity/pi-computer-use/blob/5e1fab8102ee18e3cf83499bce48a171a0ff5c87/src/bridge.ts#L428-L441)
- [并发与 stale write 动态检查](https://github.com/injaneity/pi-computer-use/blob/5e1fab8102ee18e3cf83499bce48a171a0ff5c87/scripts/check-runtime-concurrency.mjs#L74-L105)

rustdog 原有 observation-scoped ref 和名为 `epoch` 的 wire 字段,其值是
`created_at_unix_ms`。它继续验证客户端请求与 observation 的身份关系。本轮没有改写该字段,
而是在 daemon 内为 observation 的每个 PID ref 保存 capture-start resource epoch。

已实施的第一阶段:

- daemon 维护 per-PID monotonic epoch 与 dispatch lane,现有 `observation_id` 继续唯一拥有 ref。
- AX 与 window producer 在真实采集前取得一致 token,record 时按最终 refs 保存各 PID 快照。
- ref mutation 在同 PID lane 内比较 epoch,dispatch 前后各递增一次。采集发生在 mutation 前或期间都会在完成后 stale。
- dispatch 失败或结果不确定时不回滚 epoch。不同 PID 仍可并行。
- 当前只覆盖能从 observation ref 确定 PID 的 `@computer-act` mutation。坐标动作不猜资源归属。

动态证据:

1. 修复前,同 observation / 同 PID 的两条并发 mutation 都能进入 dispatch。
2. 修复后,同 PID 只执行一条,另一条返回 `stale_resource_epoch`。
3. capture 在 mutation 期间开始会在 mutation 完成后 stale;mutation 后的新 capture 获得稳定 epoch。
4. 不同 PID 并行、失败后旧 epoch 失效和 top-level retry contract 均有定向回归。

## P0: Successor observation 与语义 postcondition

上游不把 native delivery 当成任务完成。`expect` 可以要求文本、role 或 value 出现或消失。
只有新观察证明条件成立时,结果才是 `worked`;条件失败覆盖先前的 delivery success。
动作完成后总是保存完整 successor state。

源码证据:

- [三态 outcome 合并规则](https://github.com/injaneity/pi-computer-use/blob/5e1fab8102ee18e3cf83499bce48a171a0ff5c87/src/actions.ts#L112-L129)
- [`act_ui` schema 与 expect](https://github.com/injaneity/pi-computer-use/blob/5e1fab8102ee18e3cf83499bce48a171a0ff5c87/extensions/computer-use.ts#L38-L46)
- [动作后 verification 和 successor capture](https://github.com/injaneity/pi-computer-use/blob/5e1fab8102ee18e3cf83499bce48a171a0ff5c87/src/bridge.ts#L1871-L2008)

rustdog 的 `@computer-act` 已把这套语义收敛到共享 mutation response:

- `dispatch`: 是否成功投递。
- `outcome`: `worked/didnt/unknown`。
- `verification`: AX diff 的 `verified/failed` fresh evidence。
- `postcondition`: AX `exists/not_exists` 的 `verified/failed/unavailable`。
- `successor_observation`: 新 observation header;capture 不可用时不伪造。

实现只执行一次 post-action AX capture。它同时保存 successor observation、计算
pre/post AX diff并判断 postcondition。PID-backed ref 的 successor 在 resource lane
完成后采集,因此带稳定 write epoch,可以直接驱动下一条同 PID mutation。

尚未实现的是 `changes` 的可信 changes-first 摘要;当前完整 AX diff report 继续位于
verification 中,不能把它描述成稳定 successor identity diff。

这能直接减少 macOS ops 中“action 后再发一次 AX/window query”的请求数。当前动态证据
已扩展到五个远程模型。固定两个 TextEdit case、两份 policy prompt、runner、debug binary、
Pi launcher 与 canonical skill 后,每个模型都完成三组 paired run。

五个独立 archive 共记录 15 组配对。consume-successor 在 15 组中都不增加 request,其中
13 组严格减少;post-action evidence 在 15 组中全部严格减少。request 总差额为 `-64`,
post-action evidence 总差额为 `-65`。所有最终 case 通过,也没有 `unknown` 分类。各模型
的 immutable archive 位于 `../pi-rdog-calculator-eval/results/macos-ops-interaction/` 下的
`macos-ops-successor-policy-*-classified-v2` 或 M2.7 的 `*-classified` 目录。

这是固定 2-case、五远程模型的 policy treatment 认证,不是完整 5 x 8 产品 baseline。LFM2.5
已退出当前产品评测范围。对共享协议或 skill 的产品级收益声明,仍必须遵守 interaction workflow 的完整模型矩阵门禁。

另做了一轮 neutral prompt 的 visibility A/B,避免把 successor 操作指令写进 prompt。DeepSeek
`deepseek-v4-flash` 在同一 Pi、runner、skill、binary 和两个 TextEdit case 下运行 3 组配对;
hidden 只通过 test-only proxy 删除 `successor_target` / `successor_observation`, visible
保留字段。三组 request 差额为 `-7/-8/-6`,post-action evidence 差额为 `-1/0/-2`;
中位数从 `21` / `3` 降到 `14` / `2`,且 visible 的 successor 链为 `12/12`。archive
`macos-ops-successor-visibility-deepseek-2case-20260816` 状态为 `certified`。其中一次
失败的 visible source 被完整保留但没有进入 certified matrix,第三组使用同配置的 clean
replacement source。这个结果只证明该 DeepSeek、两 case 和 neutral prompt 范围内的字段可见性
收益,不能外推成完整模型矩阵结论;LFM2.5 不在当前范围内。

## P1: 单资源 checked transaction

上游的 `act_ui` 接受依赖 action 数组。它不是并行动作,而是同一 resource lane 中的顺序事务。
原生 helper 在首个失败或 invalidated step 停止,返回 `stoppedAt`,最后只 settle 一次。

源码证据:

- [`act_ui.actions` 上限 20](https://github.com/injaneity/pi-computer-use/blob/5e1fab8102ee18e3cf83499bce48a171a0ff5c87/extensions/computer-use.ts#L111-L118)
- [transaction dispatch 与停止边界](https://github.com/injaneity/pi-computer-use/blob/5e1fab8102ee18e3cf83499bce48a171a0ff5c87/src/bridge.ts#L1809-L1835)
- [架构对 batching 的边界说明](https://github.com/injaneity/pi-computer-use/blob/5e1fab8102ee18e3cf83499bce48a171a0ff5c87/docs/architecture.md#acting-and-batching)

rustdog 已有 `@flow`,所以不应新增平行脚本引擎。合理落点是改良现有 flow/control action seam:

- 已实现 `GuiTransaction.actions` 上限 20。
- 第一条 action 必须带真实 `args.target:{ref,observation_id}` 与顶层 `epoch`。
- 后续 action 必须使用 `target:"$successor"` 与 `epoch:$successor`; daemon 只从
  上一条成功 response 的 `successor_target` 注入 ref/observation/epoch。
- 非零 response、未产生 response 或缺 successor 都在首错停止,flow summary 记录
  `completed_actions`、`stopped_at`、最后 successor 或 error。
- 每个 action 仍经现有 `@computer-act` 与 PID resource lane 执行,不复制 Pi tool/session 模型。

`@flow` 中的 shell、artifact、通用 ControlLine 仍保持原语义,不能被 GUI transaction 替换。

## P1: 完整内部树与可信 diff

上游保存完整 observation,model 先看到折叠 view。`search_ui/expand_ui/inspect_ui` 只查缓存树。
mutation 后,先用 native identity,再用无歧义 structural identity 稳定短期 ref。
身份不可信、root 被替换或变化过大时,返回完整折叠 view,不输出误导 patch。

源码证据:

- [ref stabilization 与 diff fallback](https://github.com/injaneity/pi-computer-use/blob/5e1fab8102ee18e3cf83499bce48a171a0ff5c87/src/view.ts#L24-L130)
- [cached query 使用完整 stored state](https://github.com/injaneity/pi-computer-use/blob/5e1fab8102ee18e3cf83499bce48a171a0ff5c87/docs/architecture.md#successor-diffs)

rustdog 已有 `rdog ax-diff`,没有再写第二套 diff。fixture prototype 现已输入
before/after AX JSON,输出 `full | changes` 决策和现有 `DiffReport`。窗口 identity
改变或 element stable-id 配对率低于 75% 时返回 `full`;该决策尚未接入 mutation
wire response,因此不能把它描述成 live successor diff。

## P1: 统一输出预算

上游对成功输出和错误统一限制 48 KiB/2000 行,超限结果给出 UTF-8 安全 continuation。
动态检查覆盖不可变分页、任意 byte offset 和错误消息上限。

源码证据:

- [输出预算与 continuation store](https://github.com/injaneity/pi-computer-use/blob/5e1fab8102ee18e3cf83499bce48a171a0ff5c87/src/output.ts#L6-L124)
- [bounded output 动态检查](https://github.com/injaneity/pi-computer-use/blob/5e1fab8102ee18e3cf83499bce48a171a0ff5c87/scripts/check-bounded-output.mjs)

rustdog 不复制 session-local `@oN`。当前统一边界在
`ControlExecutionOutcome::from_response_line` 和 `ControlFrame::to_wire_message`:

- 所有 model-visible control response 限制为 48 KiB 或 2000 wire lines。
- 超限时返回 UTF-8-safe preview、total bytes/lines、SHA-256 content identity 和
  `@savefile`/窄查询提示。
- 当前不会虚构可续读 artifact 或 session-local offset。只有调用方主动使用现有
  `@savefile` 才产生真实 artifact。
- 命令自身的 `limit/depth/top_changes` 仍保留,统一预算是最后一道防线。

## P2: 严格后台策略

上游 `headless:true` 实际表示 strict background:禁止 activate/raise、raw pointer、raw keyboard 和 cursor overlay。
只有 background keyboard 明确返回 `didnt` 时才允许 foreground retry;pointer `unknown` 永不盲目重放。

这个规则已以 `@flow.policy.execution.strict_background:true` 落地,但不照抄
`headless` 名称或 `PI_COMPUTER_USE_*` 环境变量。它是 flow 的唯一 policy source:

- pre-dispatch 拒绝 `@window-activate`、`@window-resize`、`@open-app` 和
  `@ax-focus activate:true`,错误码为 `foreground_prohibited`。
- pre-dispatch 拒绝 raw mouse、raw keyboard、legacy paste、keyboard/clipboard
  type,错误码为 `physical_input_prohibited`。
- semantic AX action 和 `AX value` type 仍可用。既有 `permission_denied` envelope
  继续是权限缺失的权威 blocker,不被 strict policy 改写。

## 已经做得更适合 rustdog 的部分

- `@observe` 已把 visual、AX、window、display scope 和 selector recovery 放进同一 bundle。
- `@web-act` 对 browser AXWebArea 有明确的唯一匹配、AXPress-only、stale re-find 和 fresh verification。
- display guard、window identity 与 `os-logical` 坐标契约比上游通用 extension 更适合多机 daemon。
- Zenoh session channel、request id、`@savefile` 和远程 artifact delivery 是上游 session-local 模型没有覆盖的能力。
- interaction ledger 已能证明改动是否真的减少 agent 请求,不能用静态“看起来更高密度”替代完整认证。

## 不应照搬

1. 不复制 11 个 Pi tool 名称。它会与 line-control 形成第二套真相源。
2. 不复制 `AsyncLocalStorage` 和 chat branch reconstruction。rustdog 状态必须由 daemon 拥有。
3. 不把上游 structural ref matching 当 durable selector。它只适合短期 successor diff。
4. 不复制 cursor overlay。它与当前可靠性目标无关,还会增加窗口排除和权限复杂度。
5. 不复制 `@oN` 临时文件协议。复用 `@savefile` / artifact。
6. 不复制 Pi 的 env policy aliases。配置源必须保持唯一。
7. 不把上游 macOS Swift helper 逐段移植。借鉴不变量,复用 rustdog 现有 AX、SCK、window 和 input backend。

## 推荐落地顺序

1. 已完成: 并发 stale-write 实验与 daemon-owned PID resource lane。
2. 已完成: capture/write 交错、失败后失效、不同 PID 并行和结构化 stale response 回归。
3. 已完成: successor observation 与 `exists/not_exists` postcondition 共享一次 post AX capture。
4. 已完成: 使用现有 interaction ledger 的固定 2-case、五远程模型 3 x 2 policy matrix,认证
   consume-successor 相对 forced-fresh-read 不增加 request,并减少 action 后额外观察请求。
5. 已完成: 在 neutral prompt 下用 hidden/visible response proxy 做 DeepSeek 3 x 2 visibility
   matrix,证明 successor 字段本身能带来同方向的 request/evidence 收益;该结果仍限于两个
   TextEdit case,不能替代完整 5 x 8 产品 baseline。
6. 已完成: changes-first fixture prototype,只在可信 identity 时返回 changes。
7. 已完成: checked transaction、统一输出预算和 strict-background flow policy,保持
   flow/control action seam,没有新增 Pi 同名工具或配置别名。
8. deferred: cached progressive query 仍需独立规格和 live evidence,不与这些完成项混合声明。

## 最终判断

`pi-computer-use` 的优势是把并发、状态所有权、动作成功和输出预算定义成运行时不变量。
rustdog 已经拥有更丰富的远程控制 primitive,真正缺的是这些 primitive 之间更一致的状态与执行契约。

最短正确路线是改良现有 observation 和 mutation seam。跳过新工具表面、session-local 状态复制和 UI 装饰能力。
