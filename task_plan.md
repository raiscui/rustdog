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

## [2026-07-17 15:00:00] [Session ID: omx-1783957580965-m4bn8e] Phase F-3: ticket 03 cancel registry 跨实例 bug 修复 + Cancelled / PermissionDenied live trigger

### 触发
- 用户选 "1: Phase F-3" (修 ticket 03 cancel registry bug + 让 Cancelled / PermissionDenied live trigger)

### 当前状态 (LP-ticket-15-deferred-3 真实根因)
**zenoh_control.rs:240 每次请求新建 `CancelRegistry::new()`**, 跟
`SystemControlActionExecutor::cancel_registry` 字段 (control_actions.rs:78) 跨实例。

控制流:
```
handle_daemon_control_query (zenoh_control.rs:240)
  └─ parse_and_execute_control_line(line, shell, executor, &CancelRegistry::new())  ← 临时 registry_A
       └─ execute_explicit_control_request(request, shell, executor, &registry_A)
             ├─ ControlCommand::Cancel(req) → executor.execute(Cancel, ...)
             │     └─ execute_cancel(request, &executor.cancel_registry_B)         ← registry_B
             └─ default arm:
                   let token = registry_A.register(seq)
                   executor.execute(command, shell, &token)
                   registry_A.unregister(seq)
```

结果:
- `wait#1` → token register 到 registry_A (临时, 函数返回就释放)
- `cancel#seq#99:{target_seq:1}` → 走 `ControlCommand::Cancel` 分支, signal registry_B → 找不到 seq=1 → unknown_target_seq
- wait 完整跑完 (cancel 没生效)

### 实施范围 (Phase F-3)

**Step 1: 修 ticket 03 cancel registry 跨实例 bug**
- [ ] 给 `SystemControlActionExecutor` 加 accessor `pub(crate) fn cancel_registry(&self) -> &Arc<CancelRegistry>`
- [ ] `zenoh_control.rs:240` 改 `&executor.cancel_registry()` 传引用, 不再新建临时 registry
- [ ] 加单测: 模拟 wait register + cancel signal 命中同一 registry

**Step 2: Cancelled live trigger smoke**
- [ ] `scripts/smoke_computer_act_error_envelope.sh` test 1 改成 live trigger:
  `@wait#1:{duration_ms:10000}` (background) + `@cancel#seq#99:{target_seq:1}` → 验证 wait 返回 cancelled envelope
- [ ] 注意: 取消 hit 后 sleep_cancellable 50ms 内醒, 总耗时 ~50-200ms 不是 10000ms

**Step 3: PermissionDenied live trigger**
- [ ] refactor `execute_open_app` 暴露 injectable open_fn (或单独抽 helper)
- [ ] 单元测 mock open_fn 返回 Err → 验证走 permission_denied_envelope_json 路径
- [ ] smoke 不需要 live trigger (因为 daemon PATH 隔离), 单元测足够覆盖

**Step 4: 跑回归**
- [ ] cargo test 全过 (595+ tests)
- [ ] 8/8 smoke 全过
- [ ] commit + push + 文档收口

### 实施决策 (待办)
1. **executor accessor 暴露级别**: `pub(crate)` 让 zenoh_control 兄弟模块能访问, 不暴露 pub API。
2. **Arc<CancelRegistry> vs &CancelRegistry**: 保持 Arc 包装, 让 executor 可以 Clone 同时共享 registry。
3. **Step 3 open_fn refactor**: 抽 `run_open_app_command(app_name) -> io::Result<()>` helper,
   `run_open_app_on_macos` 调它。 单元测直接测 helper 用 mock Command (cfg(test))。
   Live trigger 不强求 (daemon PATH 隔离), 单测覆盖即可。

### 状态
**Phase F-3 计划已建, 准备从 Step 1 开始。**

## [2026-07-17 16:00:00] [Session ID: omx-1783957580965-m4bn8e] Phase F-2: VerifyFailed envelope 真实触发 (verify logic 真实化)

### 触发
- 用户选 "1: Phase F-2" (LP-ticket-15-deferred-2: VerifyFailed envelope 真实触发)

### 当前状态 (LP-ticket-15-deferred-2 真实根因)
- `run_best_effort_verify` / `run_always_verify` 已经真跑 AX diff (前后 snapshot + compute_diff)
- `compute_verification_passed` 根据 diff 数量判断 verify 是否通过
- **`verify` 失败时 envelope 仍 `ok:true`** — 关键 bug: dispatch ok + verify 失败,
  client 看到 ok:true 以为动作成功, 但 GUI 实际没变 (动作点错地方了)

### 实施范围 (Phase F-2)

**Step 1: 在 mod.rs:execute_computer_act 末尾, dispatch ok 之后 + verify 完成后, 加 verify 失败分支**
- if `verify_policy` 是 `BestEffort` 或 `Always`
- if `verification_passed == false`
- if dispatch ok 是 true (dispatch 错误优先)
- 改 payload: `ok: false`, 加 `error_code: "verify_failed"`, `error_message: "..."`,
  `retry: {strategy: "manual_only", hint: "..."}`, `evidence: {verification: ..., ax_diff: ...}`
- exit_code 改为 64 (跟 parse error / platform_unsupported 一致)
- 用 `error_envelope(ComputerActErrorCode::VerifyFailed, msg, Some(evidence))` helper

**Step 2: 单测 envelope shape + dispatch+verify_failed 决策**
- `verify_failed_envelope_json_matches_e2_shape` (跟 Phase F-1 风格一致)
- `dispatch_ok_with_failed_verify_emits_verify_failed_envelope` (集成测, 模拟 dispatch 成功但 verify 失败)
- `dispatch_failed_with_passed_verify_keeps_dispatch_error_code` (dispatch 错误优先)

**Step 3: 跑 7 smoke + 600+ tests 全过**

**Step 4: smoke_computer_act_error_envelope.sh 新加 test 4: VerifyFailed live trigger**
- 跑个不太可能改变 GUI 的 action (e.g. click off-screen @wait 然后 verify=best_effort, 等待 GUI 不变)
- 或者: 跑 click 在 fixed position (0,0) + verify=best_effort → 可能 GUI 不变
- 验 envelope shape: ok:false + error_code:verify_failed + retry.strategy:manual_only

### 实施决策 (待办)
1. **VerifyFailed 优先级**: dispatch 错误 > verify 错误. 如果 dispatch 失败, 用
   dispatch 错误码; 只有 dispatch 成功但 verify 失败才用 VerifyFailed
2. **verify=none 不触发 VerifyFailed**: VerifyPolicy::None 永远不验 verify,
   所以 verify_failed 不应该出现 (跟现有 compute_verification_passed 行为一致)
3. **error_envelope helper 复用**: 直接调 `error_envelope(ComputerActErrorCode::VerifyFailed, msg, Some(evidence))`,
   envelope shape 自动对齐 ADR-0004 E2 (error_code + retry.strategy + retry.hint + evidence)
4. **live trigger 难点**: 跑真 GUI 动作很难保证 GUI 不变, 可能用 `click off-screen`
   或者 `wait long` 之类; smoke live trigger 如果不稳, 退到 unit-test driven
   + live trigger 双重覆盖 (Phase F-1 模式)

### 状态
**Phase F-2 计划已建, 准备从 Step 1 开始。**
