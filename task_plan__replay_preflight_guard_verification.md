# 任务计划: 定义 replay preflight、guard 与 verification policy

## 目标

固化显式 replay 在执行动作前的必检项、缺失/歧义的 fail-closed 边界、post-action verification 范围以及 best-effort profile 能放宽的最小条件,使 controller 和 daemon 在跨 host replay 时不需要再做产品级判断。

## 阶段

- [x] 阶段 1: 任务计划与设置,核对 ticket #4 状态与 blockers
- [ ] 阶段 2: 静态证据收集(已有规格覆盖的 preflight/guard/verification 边界)
- [ ] 阶段 3: HITL 决策(一次只问一个问题,每次提供推荐答案)
- [ ] 阶段 4: 撰写正式规格、commit、push、关闭 ticket、更新 Wayfinder map

## 关键问题

1. preflight 必须做哪些 gate?
2. 哪些缺失或歧义必须 fail closed?
3. 哪些 action 需要 post-action verification?
4. best-effort profile 能放宽哪些条件?
5. 权限、参数、应用、Participating Window、display topology、Window Geometry Precondition、selector freshness、coordinate guard 各自的判定标准是什么?
6. 用户取消和失败后的安全收口边界在哪?

## 做出的决定

(待 HITL 决策)

## 遇到错误

- 无

## 状态

**当前在阶段 2**:收集已有 preflight / guard / verification 规格边界,准备第一项 HITL 决策。

## [2026-07-28 22:10:00] [Session ID: omx-1784512435044-92wxat] [调查更新]: 已有 preflight / guard / verification 边界已核对

- semantic promotion policy: 已定义 `decision order = semantic first`,`ambiguous fail closed`,coordinate 9 项 gate 全 `reject`,verification 不可伪造。
- window geometry policy: 已定义 Phase 1 (`@window-resize` 三阶段) + Phase 2 (read-only preflight: unique / Missing / WINDOW_AMBIGUOUS),Initial Window Snapshot 是 participating action 前置。
- display-aware chain: 已定义 `display scope -> window identity -> focus precondition -> targeted observation -> action -> verification`,`@window-activate` 必须 verify.focused + timeout。
- 已有 gate 但散落不同规格,需要在 #4 里固化为统一 policy 入口。

## [2026-07-28 22:14:00] [Session ID: omx-1784512435044-92wxat] [决策确认]: 8 gate preflight 顺序

- [x] Human确认8个顺序gate:permission → parameter → application → display topology → participating-window → geometry precondition → selector freshness → coordinate guard。
- [x] 任一gate失败立即fail closed,不修改target state。
- [x] 用户取消被吸收为cancel-safe rollback路径,与gate失败等价。
- [x] 不引入跨gate短路或合并。
- [ ] 下一决策:确认preflight reject reason code命名空间。

## [2026-07-28 22:18:00] [Session ID: omx-1784512435044-92wxat] [决策确认]: preflight reject reason codes

- [x] Human确认11个stable reason codes。
- [x] 复用现有`permission_denied` / `window_ambiguous` / `window_missing` / `cancelled`。
- [x] 新增7个:parameter_unbound / application_unreachable / display_topology_invalid / geometry_precondition_failed / selector_stale / coordinate_guard_missing / coordinate_guard_invalid。
- [x] 每个reason code携带stable structured field,不暴露target详情。
- [x] Reader接受未知additive code,不抛错。
- [ ] 下一决策:确认哪些action需要post-action verification。

## [2026-07-28 22:22:00] [Session ID: omx-1784512435044-92wxat] [决策确认]: post-action verification 分类

- [x] Human确认按side-effect class划分:state-mutating必选,state-read不需要,input-primitives不需要。
- [x] Verification必须fresh re-observe,不允许用pre-action缓存。
- [x] Verified:false必须进入quarantine,quarantine写Replay session metadata。
- [x] Quarantine不自动回滚side effect。
- [ ] 下一决策:确认best-effort profile与strict的差异。

## [2026-07-28 22:26:00] [Session ID: omx-1784512435044-92wxat] [决策确认]: best-effort profile

- [x] Human确认profile只接受`"strict" | "best-effort"`,默认strict。
- [x] 8个preflight gate在任何profile下都fail closed,不允许best-effort跳过。
- [x] Best-effort仅放宽post-action verification与coordinate fallback。
- [x] Profile只写Replay session metadata,不写入Bundle manifest。
- [ ] 下一决策:确认safety rollback触发条件与动作。

## [2026-07-28 22:30:00] [Session ID: omx-1784512435044-92wxat] [决策确认]: safety rollback

- [x] Human确认5类trigger:preflight_gate_failed / verification_failed / coordinate_action_failed / user_cancelled / controller_disconnected。
- [x] Human确认统一收口动作:停止后续step + 报告 + 不回滚side effect。
- [x] Human确认新增3个stable codes:verification_failed / coordinate_action_failed / controller_disconnected。
- [x] `replay_outcome`允许值:`completed` / `rolled_back` / `cancelled`。
- [ ] 下一决策:确认Bundle provenance与Replay入口一致性。

## [2026-07-28 22:34:00] [Session ID: omx-1784512435044-92wxat] [决策确认]: Bundle provenance gate

- [x] Human确认Replay入口先做Bundle provenance检查,再进入8 gate preflight。
- [x] Provenance检查包括producer.version和compiler.version。
- [x] 不兼容时fail closed,reason code `bundle_provenance_incompatible`。
- [x] daemon不自动upgrade/downgrade。
- [x] 不引入新协议命令,利用现有line-control隐式检查。
- [x] Best-effort不允许跳过provenance检查。
- [x] ticket #4所有决策已具备写规格条件,进入写规格阶段。

## [2026-07-28 22:45:00] [Session ID: omx-1784512435044-92wxat] [任务完成]: ticket #4 resolution delivered

- [x] 写完整规格 `specs/rdog-replay-preflight-guard-verification.md` (290行)。
- [x] AGENTS.md 追加长期文件索引。
- [x] git commit 28cbab0 在 auto-optimize/20260728-2316-rdog-control 分支,scope 限定 specs + AGENTS.md。
- [x] 不 push(由用户决定何时 merge 到 main)。
- [x] gh issue close 4 with full resolution comment。
- [x] Wayfinder map ticket #2 body 追加 ticket #4 entry,放在第一个位置(最新优先)。
- [x] 范围内dirty worktree未污染(用户的 47 modified + 24 untracked 不动)。
