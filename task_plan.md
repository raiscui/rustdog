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

## [2026-07-17 17:00:00] [Session ID: omx-1783957580965-m4bn8e] Phase F-3.5: PermissionDenied live trigger (refactor execute_open_app 暴露 injectable open_fn)

### 触发
- 用户选 "1: Phase F-3.5" (LP-ticket-15-deferred-5: PermissionDenied live trigger)
- 收口 11 个 ComputerActErrorCode variant 中 3 个 live trigger (Cancelled/VerifyFailed/PermissionDenied)

### 当前状态 (LP-ticket-15-deferred-5 真实根因)
- error_envelope.rs::permission_denied_envelope_json() helper 已存在 (Phase F-1)
- run_open_app_on_macos PermissionDenied 分支已走 envelope helper (Phase F-1)
- **PermissionDenied 真触发路径难稳定**:
  - daemon PATH 是 daemon 启动时 env 决定的, smoke 改 client shell PATH 不影响 daemon
  - macOS 上 `open` 命令通常在 /usr/bin/open, 不会因 PATH 缺失
  - 真实能触发的: chmod -x /usr/bin/open (sandbox 限制), spawn 失败 (OS 限制)
  - 跟 Phase F-1 test 2 一样退到 unit-test driven 也行, 但单元测
    覆盖的是 envelope shape, 缺少 dispatch + envelope 协同验证

### 实施范围 (Phase F-3.5)

**Step 1: refactor execute_open_app 暴露 injectable open_fn (cfg(test) trait)**
- 抽 `trait OpenAppCommand { fn run(&self, app_name: &str) -> io::Result<std::process::Output>; }`
- `SystemOpenAppCommand` 默认实现: 调 `Command::new("open")`
- `execute_open_app` 接收 `&dyn OpenAppCommand` 参数, 默认参数是 `&SystemOpenAppCommand`
- cfg(test) 测试用 `MockOpenAppCommand` 注入失败场景

**Step 2: 单测 cfg(test) 覆盖 PermissionDenied live path**
- 注入 MockOpenAppCommand 返 Err(IO error)
- 调 execute_open_app + 验 response envelope shape
- (跟 Phase F-1 test 2 风格一致, 但这次是 execute_open_app 完整路径)

**Step 3: smoke_computer_act_error_envelope.sh test 2 升级为 cfg(test) 驱动 (不依赖 env)**
- test 2 之前是 unit-test driven (跑 cargo test)
- 升级: 同时跑 cargo test 验 envelope shape + 用 mock 跑 execute_open_app 验端到端

**Step 4: 跑 7/7 smoke + 600+ tests 全过**

### 实施决策 (待办)
1. **injectable 设计**: 用 trait object (`&dyn OpenAppCommand`) 而不是 generic, 保持
   execute_open_app 签名向后兼容 (tester 传 mock, production 走 system)
2. **不在 macOS 上依赖 PATH 缺失**: daemon 启动时 PATH 固定, smoke 改不到
3. **cfg(test) 单测覆盖 end-to-end**: 测 dispatch + envelope 协同 (Phase F-1
   unit-test driven 只测 envelope shape)
4. **OpenAppErrorCode 留 `app_not_found` 区别于 PermissionDenied**:
   - `app_not_found`: `open -a <bad_app>` 返 exit 1 (e.g. app 不存在)
   - `permission_denied`: spawn `open` 本身失败 (PATH 缺失 / 权限)
   - 两者是不同 error_code, 都需要 envelope helper, 这次只补 PermissionDenied live

### 状态
**Phase F-3.5 计划已建, 准备从 Step 1 开始。**


### 状态 (2026-07-17 17:30:00)
**Phase F-3.5 收口 ✓**

- Step 1 (OpenAppCommand trait refactor) 完成
- Step 2 (3 mock + 3 unit tests + `fake_exit_status` helper) 完成, 3/3 passed
- Step 3 (smoke_computer_act_error_envelope.sh test 2 升级 mock 注入) 完成
  - 2a Phase F-1 envelope shape 单元测: 1 passed (原保留)
  - 2b Phase F-3.5 execute_open_app live trigger via mock: 3 passed (新加)
- Step 4 (worklog + LATER_PLANS LP-15-deferred-5 RESOLVED + EPIPHANY_LOG) 完成
- cargo test 601 passed, 0 failed, 1 ignored
- 8/9 smoke scripts 7+ 段端到端验证通过 (smoke_cancel_seq test 5 self-target
  是 main 上 pre-existing bug, 不在 Phase F-3.5 范围内, 已在 EPIPHANY 记录)


## [2026-07-17 18:00:00] [Session ID: omx-1783957580965-m4bn8e] 任务: @cancel#seq self-target bug fix (Phase F-3.5 follow-up)

### 触发
- 用户说 "继续" (Phase F-3.5 收口后续)
- 我上一轮标 smoke_cancel_seq test 5 self-target 是 pre-existing bug 跳过
- 这一轮仔细 trace 发现是 root cause 明确的真 bug

### root cause 静态 + 动态证据
**静态证据**:
- control_core.rs:104 `command =>` catch-all (包括 Cancel)
- control_core.rs:141 `cancel_registry.register(seq)` 把 cancel 自己的 seq 加进共享 registry
- control_actions.rs:146 Cancel 分支: `execute_cancel(request, &self.cancel_registry)` 用同一 registry
- control_actions.rs:317 `registry.signal(target_seq)` 然后 `signaled = true`

**动态证据**:
- 跑 smoke_cancel_seq, test 5 输出 `{signaled:true, ok:true}` 而不是
  `{ok:false, error_code:unknown_target_seq}`
- git stash 验证 main (9e2b329) 上同样 fail → 排除本会话引入
- fix 后跑 smoke_cancel_seq, test 5 输出 `{ok:false, error_code:unknown_target_seq}` ✓

### 实施
**Step 1** (committed in this session): control_core.rs catch-all 加
`is_cancel_command` guard, Cancel 命令不进 cancel registry (signal-only,
没有 in-flight 期).

**Step 2**: src/control_actions/tests.rs 末尾 2 个 unit test
(`execute_cancel_emits_unknown_target_seq_when_target_not_in_registry` +
`execute_cancel_emits_ok_when_target_signal_succeeds`).

**Step 3**: cargo test 603 passed (+2), 0 failed
**Step 4**: smoke_cancel_seq 5/5 PASSED, 6 个其他 smoke 全过不退化
**Step 5**: WORKLOG + LATER_PLANS (LP-15-deferred-3-RESOLVED 追加) + EPIPHANY_LOG 一起发

### 状态
**Self-target bug fix 收口 ✓**

## [2026-07-17 19:30:00] 跨项目索引: fast-infer Mano-CUA OpenAI server 上线 (port 18094)

### 触发
- 用户指令: LFM2.5 已删除 + 主要关注 Mano-CUA + "继续 A" (开 OpenAI-compatible server wrapper)
- fast-infer commit: 36a0872 (feat) + 46d3ed6 (docs)

### 上线状态 (fast-infer origin/main)
- **端口 18094** Mano-CUA OpenAI-compatible server 已 runnable
  - 16 action space (OpenAI tools=[] schema 完整)
  - 双 parser 路径 (自然 XML / qwen3-coder XML)
  - 4/4 smoke 全过 (含 click 精度 ~5px)
- **Pi 集成**: `local-mano-cua-vlm` provider 已在 `~/.pi/agent/models.json`
  - baseUrl http://127.0.0.1:18094/v1
  - 支持 tools + image_url
- 待补: rdog-control-16-actions toolUseProfile (LP-2026-07-06-1 follow-up)

### 关键发现 (跨项目共享经验)
- **Apple Metal multi-call GPU crash** (fast-infer EPIPHANY 沉淀):
  - Apple MLX Metal stateful, 连续推理第二次 prefill 时崩
  - 修复: per-request `mx.clear_cache()` + `gc.collect()` — 跟 Holo 3.1 / mlx-vlm 同款
  - 重要性: rustdog 未来如果接 Apple Silicon MLX 后端, 必须继承这个 pattern

### rustdog 后续候选
- LP-2026-07-06-1 follow-up: 设计 rdog-control-16-actions toolUseProfile
  - 16 个 Mano-CUA action 怎么映射到 rdog control 命令 (click/type/scroll/drag/hotkey 等)
  - Pi tools 集成闭环: prompt → Mano-CUA tool_call → rdog control action → screenshot → next step
- LP-2026-07-06-3 multi-step agent loop benchmark (5 步)


## [2026-07-17 20:00:00] 跨项目索引: fast-infer Phase B rdag-control-16-actions profile + Mano-CUA + rdag 端到端 e2e 闭环

### 触发
- 用户 "B 接着做" (rdag-control-16-actions Pi profile + 闭环)
- fast-infer 上 commit 36a0872 (Phase A) → 6f2548b (Phase B)

### 跨项目状态
- **Mano-CUA server schema 已对齐 rdag @computer-act.v1**: 
  `mano_cua_actions.py`: start_box / end_box 从 string literal 改 int array [int, int],
  duration 改 duration_ms:int. 16 action 同名 (rdag <-> Mano-CUA).
  防御性 fallback: model 输出 box_start literal 时, server 自动 strip 转 [int, int].
- **Pi provider local-mano-cua-vlm 已写 + toolUseProfile rdag-control-16-actions 已写**.
- **端到端 smoke (`smoke_mano_cua_to_rdag_e2e.py`)** 全过:
  click(701, 501) (model output) → rdag control @computer-act#1001:click →
  rdag @click dispatch 173ms, ok=true, observation_used.freshness=fresh,
  坐标精度 3-5px.

### rustdog 后续候选
- **LP-2026-07-06-4 (Pi 真实端到端)**: 在 /tmp/干净小目录跑 
  `pi --provider local-mano-cua-vlm --tools bash --skill rdog-control`,
  验证 Pi 真实 binary 走 Mano-CUA → tool_call → bash → rdag control 完整闭环.
- **LP-2026-07-06-3 (multi-step agent loop)**: 仿 smoke_holo31_agent_loop.py 模式,
  5 步 loop, 测 tool role 回灌 + multi-turn image_url 注入.
- **rustdog 没有改动**: rdag @computer-act.v1 已支持 13 个 action, 跟 Mano-CUA 16 个
  只是含 3 个 termination signal (finish/stop/call_user) 不 dispatch, 不影响 rdag 端.

## [2026-07-18 00:10:44] [Session ID: omx-1784304547353-h5409r] [支线索引]: local-default registry 恢复与一致性验证

- 启用支线上下文集后缀: `local_default_registry_recovery`.
- 触发: daemon 报 `local-default` 守卫已存在,但裸 `rdog control` 同时报告没有可用 registry,并发现两个 FIFO 候选.
- 目标: 区分真实存活实例、陈旧 PID guard、缺失/失效 registry 与残留 FIFO,用动态证据决定运行态恢复还是代码修复.
- 当前计划文件: `task_plan__local_default_registry_recovery.md`.

## [2026-07-18 10:23:45] [Session ID: omx-1784304547353-h5409r] [支线完成]: local-default registry 恢复与一致性验证

- 已修复重复 daemon在 ownership确认前删除活跃 unixpipe FIFO的问题.
- canonical base-path guard、endpoint单一真相源、隔离 e2e与规格同步均已完成.
- 真实 `mac.lab` daemon已切换到安装版PID 69053;重复启动正确失败,前后裸 ping都返回pong.
- 详细计划与证据: `task_plan__local_default_registry_recovery.md`、`notes__local_default_registry_recovery.md`.
- 交付与后续: `WORKLOG__local_default_registry_recovery.md`、`ERRORFIX__local_default_registry_recovery.md`、`LATER_PLANS__local_default_registry_recovery.md`、`EPIPHANY_LOG__local_default_registry_recovery.md`.

## [2026-07-18 10:54:47] [Session ID: omx-1784304547353-h5409r] [支线索引]: local-default 原子 lease状态源

- 启用支线上下文集后缀: `local_default_atomic_lease`.
- 触发:用户要求按建议继续,上一轮首个后续风险是PID复用、双文件写入中断与guard状态分裂.
- 目标:用OS生命周期绑定的ownership lease统一三类guard记录格式与校验语义,同时保留现有v1本地状态兼容恢复.
- 当前计划文件: `task_plan__local_default_atomic_lease.md`.

## [2026-07-18 12:40:06] [Session ID: omx-1784340333160-6bwnss] [支线完成]: local-default 原子process lease

- Unix service-name、canonical path和local-default已迁移到OS文件锁lease,保留独立冲突域与legacy v1读取兼容.
- 已验证metadata失败回滚、lease ID关联、部分managed拒绝、SIGKILL接管和stable inode不变.
- 最终daemon PID 29465正在`rdog-daemon` tmux运行,bare ping返回pong,重复启动正确拒绝.
- 详细证据:`task_plan__local_default_atomic_lease.md`、`notes__local_default_atomic_lease.md`.
- 交付与风险:`WORKLOG__local_default_atomic_lease.md`、`ERRORFIX__local_default_atomic_lease.md`、`LATER_PLANS__local_default_atomic_lease.md`、`EPIPHANY_LOG__local_default_atomic_lease.md`.

## [2026-07-18 12:51:22] [Session ID: omx-1784340333160-6bwnss] [支线索引]: local-default legacy退役

- 启用支线上下文集后缀:`local_default_legacy_retirement`.
- 触发:用户选择上一轮后续建议1,要求退役旧二进制stale PID unlink迁移窗口.
- 目标:把legacy状态限制为fail-closed升级入口,managed-only成为唯一正常运行契约,并用旧版/新版矩阵验证不会出现双owner.
- 当前计划文件:`task_plan__local_default_legacy_retirement.md`.

## [2026-07-18 13:35:10] [Session ID: omx-1784304547353-h5409r] [支线完成]: local-default legacy退役

- 空target/self已改为只接受完整managed registry、匹配sidecar identity与active OS lock;纯v1 PID和FIFO候选不再自动成为owner.
- active legacy PID检查保留为fail-closed升级门;stopped legacy继续在stable inode上原地迁移.
- runtime 34、unixpipe e2e 12、router-client 26通过,all-targets check和release build为0 error.
- 最终安装版hash:`96955460e968cc8ccaf06c1b4fc2bce888e4c5564df5b6f0cac69e348249cc75`;正式daemon PID 19047,bare ping返回pong.
- 详细证据:`task_plan__local_default_legacy_retirement.md`、`notes__local_default_legacy_retirement.md`、`WORKLOG__local_default_legacy_retirement.md`、`ERRORFIX__local_default_legacy_retirement.md`.
- 后续边界:Windows ownership迁移见`LATER_PLANS__local_default_legacy_retirement.md`;超长runtime模块拆分仍见`LATER_PLANS__local_default_atomic_lease.md`.

## [2026-07-18 16:13:17] [Session ID: omx-1784304547353-h5409r] [支线索引]: zenoh_runtime职责拆分

- 启用支线上下文集后缀:`zenoh_runtime_split`.
- 触发:用户要求拆分已经达到1928行的`src/zenoh_runtime.rs`.
- 目标:保持`zenoh_runtime`外部interface和运行行为不变,按session、unixpipe、local-default职责形成深模块,并把单元测试移出生产门面文件.
- 当前计划文件:`task_plan__zenoh_runtime_split.md`.

## [2026-07-18 17:05:00] [Session ID: omx-1784304547353-h5409r] [支线完成]: zenoh_runtime职责拆分

- `src/zenoh_runtime.rs`从1928行收敛为22行稳定门面;session、unixpipe、local-default及各自测试已按职责拆分.
- production symbol、34个测试名与26个外部调用行均与HEAD旧实现等价,没有改变公开调用路径.
- 全bin 612 passed / 1 ignored,runtime 38 passed,unixpipe e2e 12 passed,router-client 26 passed / 2 ignored;check与release build为0 error.
- 安装版和release hash一致,正式daemon PID 82774的bare/self/显式target ping均返回pong,重复daemon正确拒绝.
- 详细记录:`task_plan__zenoh_runtime_split.md`、`notes__zenoh_runtime_split.md`、`WORKLOG__zenoh_runtime_split.md`、`ERRORFIX__zenoh_runtime_split.md`.
