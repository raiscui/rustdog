# 任务计划: rdog `@computer-act` Phase F-1 (Cancelled / PlatformUnsupported / PermissionDenied envelope)

## 目标

Phase F-1 收口 LP-ticket-15-deferred-1: 把 3 个手写 JSON payload 的 error_code 路径
(Cancelled / PlatformUnsupported / PermissionDenied) 改走 `error_envelope()` helper,
跟其它 4 个已触发的 error_code 形状一致 (ADR-0004 E2: `error_code` + `retry.strategy` +
`retry.hint` + `evidence`)。

不依赖 Phase I (真实 observe 集成), 单 session 收口。

## 阶段

- [x] 阶段 1: 加 3 个 String-returning envelope helper (cancelled_envelope_json /
      platform_unsupported_envelope_json / permission_denied_envelope_json)
- [x] 阶段 2: 加 3 个单测验 envelope shape (ADR-0004 E2 形状)
- [x] 阶段 3: 改 `control_actions.rs` 3 处 caller 走新 helper
      (build_cancelled_wait_response_json + open_app platform_unsupported 分支 +
      open_app permission_denied 分支)
- [x] 阶段 4: `mod error_envelope` 改 `pub(crate) mod error_envelope` 让兄弟模块 use
- [x] 阶段 5: 加 `scripts/smoke_computer_act_error_envelope.sh` (3 段, 单测驱动)
- [x] 阶段 6: 跑回归 (8/8 smoke + 594 tests + 0 warning)
- [x] 阶段 7: commit + push (8b21988) + docs commit + push (ecc8ee4)

## 关键决策

1. **形状正确 vs 行为正确**: Phase F-1 只改 caller 走 envelope helper (形状正确),
   不做 live trigger (行为正确)。Live trigger 路径都撞 ticket 03 遗留 bug
   (zenoh_control.rs:240 每次新建 CancelRegistry), 留 Phase F-3 一起做。
2. **smoke 退到 unit-test driven**: 不假装 e2e live trigger, 显式注释 + 跑 cargo test
   单测验 envelope shape。这是诚实选择, 比编造 live trigger 路径更负责。
3. **`pub(crate) mod error_envelope` 提升可见性**: 让 control_actions 兄弟模块 use
   内部 helper, 不破坏现有依赖图 (单向调用)。
4. **`#[allow(dead_code)]` platform_unsupported_envelope_json**: macOS 编译时
   `cfg(not(target_os))` 分支被排除, helper 没有 live caller, 但单测还在用, 不能删。
5. **不依赖 Phase I**: 4 个剩余 variant (ObservationExpired / TargetNotFound /
   VerifyFailed / Infrastructure) 完全没触发路径, 留 LP-ticket-15-deferred-2。

## 遇到错误

- **smoke live trigger 撞 ticket 03 bug**: `@cancel#seq#99:{target_seq:1}` 取消 `@wait#1`
  返回 `unknown_target_seq`, 因为 zenoh_control.rs:240 每次新建 CancelRegistry 跟
  executor 内部 registry 跨实例。**修复策略**: 留 Phase F-3 + ticket 03 fixup 一起做,
  本 Phase F-1 不动 zenoh_control.rs。
- **smoke 改 PATH 不影响 daemon**: PermissionDenied live trigger 需要让 daemon 进程
  的 `open` Command 失败, 但 PATH 是 daemon 启动时 env, smoke 改 client shell PATH
  不影响 daemon。**修复策略**: 留 Phase F-3 cfg(test) mock Command 或 refactor
  execute_open_app 暴露 injectable open_fn。
- **warning: `platform_unsupported_envelope_json` is never used**: macOS 编译时
  cfg(not(target_os)) 分支不调用。**修复策略**: 加 `#[allow(dead_code)]` 注释。

## 当前状态

**Phase F-1 实施完成 + commit + push (8b21988) + docs commit + push (ecc8ee4)!**

完整时间线:
```
ticket 19 (a9b6401) → ticket 20 (c07dad3) → docs (0150204) → Phase F-1 (8b21988) → docs (ecc8ee4)
 ✓                      ✓                   ✓                ✓                    ✓
```

下一步候选:
- Phase F-3: 修 ticket 03 cancel registry 跨实例 bug + Cancelled/PermissionDenied live trigger
- Phase F-2: verify logic 真实化 (VerifyFailed envelope 触发)
- Phase I: 真实 observe 集成 (ObservationExpired / TargetNotFound 触发)
- fast-infer: LFM2.5 Pi 端到端 + rdog 控制 (LATER_PLANS #6/#7/#8)

## 索引

- 当前 task_plan.md 只追踪本 session (Phase F-1) 状态。
- 历史 plan: `archive/default_history/task_plan_2026-07-17_143000_before_phase_f_1_rollover.md`
  (1018 行, 涵盖 ticket 01-22 + Phase H ticket 19+20 + 早期 planning entry)。
- 详细实施记录: `WORKLOG.md` `[2026-07-17 14:30:00]` entry。
- 后续工作清单: `LATER_PLANS.md` LP-ticket-15-deferred-2/3/4。
- 重要洞察: `EPIPHANY_LOG.md` `[2026-07-17 14:35:00]` smoke 诚实选择 entry。
- 续档 manifest: `archive/manifests/ARCHIVE_MANIFEST__2026-07-17_task_plan_rollover_phase_f_1.md`
