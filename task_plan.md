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

## [2026-07-29 00:00:00] [Session ID: omx-1784512435044-92wxat] [实施继续]: Wayfinder ticket #19 Bundle 导出与远程交付

### 目标
- 交付确定性 `.rdogrec.tar` Bundle writer、原子提交、SHA-256 完整性元数据和 owner-only 单帧 `@savefile` 交付。
- 遵循 ticket #13 简化边界: 不实现 reader、control parser、evidence 收集、真实 replay compiler 或扩展 reject code。

### 阶段
- [x] 阶段1: 恢复 #19 worktree、分支和前序实现状态
- [ ] 阶段2: 阅读 Bundle / lifecycle / savefile 现有契约并确定最小复用面
- [ ] 阶段3: 实现 Bundle writer 与 delivery helper
- [ ] 阶段4: 添加最小回归测试并修复验证问题
- [ ] 阶段5: 全量验证、提交、推送并关闭 ticket #19

### 当前状态
- 当前在阶段2。先复用已有 `SaveFileFrame`、Session owner 和哈希依赖,避免平行协议模型。

## [2026-07-29 00:01:00] [Session ID: omx-1784512435044-92wxat] [阶段更新]: #19 最小复用面确认

- [x] 阶段2: 已确认复用 `SaveFileFrame`、`ConnectionId`、Session owner/getter、`sha2` 和 `base64`。
- [ ] 阶段3: 实现 Bundle writer 与 delivery helper。
- 决定: 不新增 dependency、不新增 reader、不接 control parser。Bundle manifest 按 ticket #13 只保留 `redaction_summary.segment_count`,不生成 `warnings`。

## [2026-07-29 00:05:00] [Session ID: omx-1784512435044-92wxat] [阶段完成]: Bundle 与 delivery 验证完成

- [x] 阶段3: `bundle.rs` 提供 canonical JSON、POSIX USTAR、路径安全、256 MiB 限制、staging + fsync + rename、per-file/whole-archive SHA-256。
- [x] 阶段4: `delivery.rs` 提供 owner-only base64 `@savefile` frame、384 MiB frame gate、4 个 stable reason code、5/sec/connection sliding limit。
- [x] 阶段4验证: `cargo test --bin rdog control_recording` -> 45 passed; `cargo test --bin rdog` -> 657 passed, 1 ignored;系统 `tar -tf` 成功读取并确认 entry order。
- [ ] 阶段5: commit/push/关闭 ticket #19,更新 map #14。

## [2026-07-29 00:08:00] [Session ID: omx-1784512435044-92wxat] [阶段完成]: 代码交付完成,GitHub ticket 更新受认证阻塞

- [x] 阶段5代码部分: commit `a4bd441` 已创建并推送到 `origin/feature/recorder-bundle-delivery`。
- [ ] 阶段5 issue部分: `gh issue comment/close` 未执行,GitHub API 返回 `invalid header field value for "Authorization"`;不能宣称 ticket 已关闭。
- 当前停止条件: 本地实现、测试、推送均完成;仅外部 GitHub CLI 凭据需要后续恢复后再更新 issue/map。

## [2026-07-29 00:12:00] [Session ID: omx-1784512435044-92wxat] [外部同步重试]: 使用 GITHUB_TOKEN 更新 issue

- 重试命令使用 `GH_TOKEN="$GITHUB_TOKEN" gh issue ...`。
- GitHub API 仍返回 `invalid header field value for "Authorization"`,因此 #19 comment/close 与 #14 map comment 仍未提交。
- 代码分支 `feature/recorder-bundle-delivery` 已推送且本地验证完成,不修改实现。

## [2026-07-29 00:16:00] [Session ID: omx-1784512435044-92wxat] [阶段完成]: GitHub 状态同步完成

- [x] 诊断: `GITHUB_TOKEN` 原始值含 `\r`,导致 gh Authorization header 非法。
- [x] 修复执行: 仅在进程内使用 `tr -d '\r\n'` 清洗 token,未持久化或修改配置。
- [x] GitHub: #19 已评论并关闭; #14 已追加 map 完成记录。
- [x] 阶段5: commit `a4bd441` 已推送,代码/测试/issue/map 全部完成。

## [2026-07-29 00:25:00] [Session ID: omx-1784512435044-92wxat] [实施继续]: control plane 集成 @record-start/status/mark/stop/cancel

### 目标
- 把 lifecycle / Bundle writer / DeliveryManager 接入 line-control。
- 仅引入 5 个 line-control 解析分支 + 1 个 `ControlCommand::Record` 变体 + 1 个 `RecordingHandler`。
- 不重写 `LifecycleManager`、不重写 Bundle writer、不暴露 `internal` API。

### 阶段
- [ ] 阶段6: 设计 `ControlCommand::Record` 与 `RecordingHandler` 边界
- [ ] 阶段7: 在 control_protocol.rs 注册 5 个解析分支
- [ ] 阶段8: 在 control_core.rs 路由到 RecordingHandler
- [ ] 阶段9: 实现 `RecordingHandler` 调用 LifecycleManager/BundleWriter/DeliveryManager
- [ ] 阶段10: 添加集成测试,跑全量回归
- [ ] 阶段11: 提交并推送分支

### 当前状态
- 当前在阶段6 起步。

## [2026-07-29 00:45:00] [Session ID: omx-1784512435044-92wxat] [阶段完成]: control plane 集成 + 全量测试

- [x] 阶段6: 设计 `ControlCommand::Record` 与 `RecordingHandler` 边界。
- [x] 阶段7: 在 `control_protocol.rs` 注册 5 个 `@record-*` 解析分支。
- [x] 阶段8: 在 `control_core.rs` 加全局 `RecordingHandler` 槽 + `handle_record_command` 路由。
- [x] 阶段9: `RecordingHandler` 完整 start/status/stop/cancel + mark 未实现占位。
- [x] 阶段10: 4 项 handler 集成测试 + 全量 661 pass,1 ignored。
- [ ] 阶段11: 提交并推送分支,更新 issue。

## [2026-07-29 00:52:00] [Session ID: omx-1784512435044-92wxat] [阻塞]: GitHub 推送受网络阻断

- commit `e60eea6`(本次集成)已创建在本地分支 `feature/recorder-bundle-delivery`。
- 推送连续 3 次被 `Connection closed by UNKNOWN port 65535` 阻断,LibreSSL SSL_ERROR_SYSCALL + SSH 代理异常都失败。
- 本地备份:`/tmp/rdog-feature-recorder-bundle-delivery.bundle` 包含 `feature/recorder-lifecycle..feature/recorder-bundle-delivery`。
- issue #20+#14 的 GitHub 同步等推送恢复后补做。

## [2026-07-29 00:55:00] [Session ID: omx-1784512435044-92wxat] [阶段完成]: 网络恢复 + 推送 + issue 同步

- [x] 推送: SSH 远程恢复,一次成功,`a4bd441..e60eea6` 已落在 `origin/feature/recorder-bundle-delivery`。
- [x] map #14 已追加 4/5 集成完成 + @record-mark 阻塞说明。
- [x] follow-up issue 已创建,等 issue 号回填到本任务文件。

## [2026-07-29 01:00:00] [Session ID: omx-1784512435044-92wxat] [实施继续]: Wayfinder issue #20 @record-mark 落地

### 目标
- 把 `control_handler` 里 `RecordRequest::Mark` 的 not_implemented 占位换成真实调用。
- Session 已有 `mark(label, redaction_active)`,只接它,不重新实现。
- 加 3 条 owner-only / no-active / success 集成测试。

### 阶段
- [ ] 阶段A: 用 red 测试锁住当前 not_implemented 行为
- [ ] 阶段B: 切换为真实 `session.mark(label, false)` 调用并把 `redaction_active` 暴露为可选
- [ ] 阶段C: 单测 + 全量验证
- [ ] 阶段D: 提交、推送、关闭 #20

### 当前状态
- 阶段A 起步。`session.mark(label, redaction_active)` 在 #18 提交时已存在,本次只做桥接。

## [2026-07-29 01:10:00] [Session ID: omx-1784512435044-92wxat] [阶段完成]: issue #20 @record-mark 落地 + 全量 664 pass

- [x] 阶段A: 旧测试用 `assert_eq!(value["kind"], ...)` 一直 panic,改用 `as_str().unwrap_or("")` 显示 left:"" 暴露真实根因。
- [x] 阶段B: handler.mark 接 Session::mark(label, redaction_active);RecordRequest::Mark 加 redaction_active;protocol 解析支持对象 payload。
- [x] 阶段C: 补 3 条 mark 测试, 修复 `temp_dirs` 测试目录在同 ms 多线程下文件名冲突 (加 `std::thread::current().id()` 唯一化)。
- [x] 阶段D: 全量 664 pass,1 ignored,recorder 52 pass。
- [ ] 阶段E: 提交 + 推送 + 关闭 #20。

## [2026-07-29 01:25:00] [Session ID: omx-1784512435044-92wxat] [实施继续]: Wayfinder issue #15 rdog record CLI dispatcher

### 目标
- `rdog record <start|status|mark|stop|cancel> [args] <target> [--url|--transport|--namespace|--target-name]` 5 个子命令。
- 复用 `control_invocation::resolve_control_invocation` + `send_control_lines_for_invocation`。
- 5 个 subcommand 生成对应 `@record-*` 控制行;`record stop` 等待 `SaveFile` 帧并保存到 `rdog_downloads/`。

### 阶段
- [ ] 阶段A: `RecordCommand` enum + `RecordSubCommand` 在 `input.rs` 中
- [ ] 阶段B: `control_recording::cli::run` dispatch 实现 (单文件,~150 行)
- [ ] 阶段C: 在 main.rs 接 `Command::Record { subcommand, host, opts }` 路由
- [ ] 阶段D: 单元测试覆盖 5 个 subcommand 的 line 构造
- [ ] 阶段E: 提交 + 推送 + 关闭 #15

### 当前状态
- 阶段A 起步。决定不引入新依赖,clap derive + 现有 helper 足够。

## [2026-07-29 01:45:00] [Session ID: omx-1784512435044-92wxat] [阶段完成]: issue #15 rdog record CLI dispatcher 落地

- [x] 阶段A: `RecordSubcommand` + `RecordCommandShared` 已注册到 `input::Command`。
- [x] 阶段B: `control_recording::cli::run` 复用 `resolve_control_invocation` + `send_control_lines_for_invocation`。
- [x] 阶段C: main.rs 加 `Command::Record` match 分支。
- [x] 阶段D: 7 条 unit test 覆盖 5 个 subcommand 的 line 构造 + label escape + profile validation。
- [x] 阶段E: 全量 637 pass, 1 ignored; `rdog record --help` 展示 5 个子命令,`record start --help` / `record mark --help` 验证 clap options。
- [ ] 阶段F: 提交 + 推送 + 关闭 #15。

## [2026-07-29 02:00:00] [Session ID: omx-1784512435044-92wxat] [阶段完成]: issue #22 --duration CLI + protocol 字段落地

- [x] humantime parser: 11 unit test 覆盖 (s/m/h / 1.5m / optional whitespace / invalid / validate bounds).
- [x] `RecordRequest::Start { profile, duration_ms }` 协议字段.
- [x] `parse_record_start_payload` 直接 `serde_json::from_str(input.trim())` (不依赖 `object_inner` 错误设计).
- [x] `RecordSubcommand::Start` clap 字段名 `duration` (kebab-case 自动产生 `--duration`).
- [x] `RecordCommand::Start` 透传 `duration_ms`.
- [x] `render_line` 输出 `duration_ms` 字段.
- [x] 5+3+5 = 13 new tests (humantime + cli + protocol); 690 全过, 1 ignored.
- [x] 实际 `rdog record start --duration 1.5m self` 走通 (clap 接受 humantime, 后端 daemon 找不到是预期).
- [ ] commit + push + close #22.

## [2026-07-29 22:00:00] [Session ID: omx-1784512435044-92wxat] [#23 任务规划]: Recording auto-stop timer + lifecycle integration

### 目标
- 按 issue #23 验收清单,实现 daemon-side auto-stop timer + lifecycle 集成,并发到 `feature/recorder-bundle-delivery`。
- 新增 `StopTrigger` 枚举 + `TerminalSummary.stop_trigger` + `RecordingHandler::Drop` + 6 集成测试 + 1 enum 序列化测试。
- All 690+ 已有测试保持绿,新增 +6 测试也绿。
- 关闭 GitHub issue #23 并更新 Wayfinder map 决策索引。

### 阶段
- [ ] 阶段1:在 `session.rs` 加 `StopTrigger` enum + `TerminalSummary.stop_trigger` + `with_trigger()` builder。
- [ ] 阶段2:在 `control_handler.rs` 加 `AutoStopTimer` struct + `auto_stop_timer: Option<AutoStopTimer>` 字段 + `Drop` impl。
- [ ] 阶段3:在 `start` 时 spawn timer 线程 (100 ms tick poll, `Arc<AtomicU8>` flag 三态)。
- [ ] 阶段4:在 `stop` / `cancel` / `Drop` 中 set flag 1 + join thread, 复用现有 stop 路径。
- [ ] 阶段5:`@record-status` last_session 暴露 stop_trigger;@record-stop 响应加 trigger 字段。
- [ ] 阶段6:6 集成测试 + 1 enum 序列化测试 (auto_stop_fires_after_duration 等)。
- [ ] 阶段7:`cargo test --bin rdog` 全过,scoped commit, push, close #23,更新 Wayfinder map。

### 关键设计
- 单 `Arc<AtomicU8>` flag 三态: 0=pending, 1=cancelled, 2=fired。
- timer 线程 100 ms `std::thread::sleep` 循环 tick, 唤醒后检查 flag 或 elapsed。
- 复用现有 `RecordingHandler::stop` 完整路径 (begin_finalize + bundle write + savefile delivery)。
- `RecordingHandler::Drop` 先 set flag=1 + join thread。
- `0` 视为不传 duration, 不起 timer。

### 风险
- `RecordingHandler` Send? `DeliveryManager` 内 `HashMap<ConnectionId, VecDeque<Instant>>` 中 `Instant` 是 Send + Sync,问题不大。但 `Session` 含 `Box<dyn RecorderCapture>`,需要查 `RecorderCapture` 是否 Send。
- 若非 Send, timer 线程必须捕获克隆而非 `self`。
- Timer 触发时 Mutex 必须不被持有,否则死锁。caller 释放 Mutex 后 timer 才能 lock。

### 状态
**目前在阶段1** — 准备改 `session.rs` 加 StopTrigger 字段。

### 进展
- [x] 阶段1:在 `session.rs` 加 `StopTrigger` enum + `TerminalSummary.stop_trigger` + `with_trigger()` builder。
- [x] 阶段2:在 `control_handler.rs` 加 `AutoStopTimer` struct + `auto_stop_timer: Option<AutoStopTimer>` 字段 + `Drop` impl。
- [x] 阶段3:在 `start` 时 spawn timer 线程 (100 ms tick poll, `Arc<AtomicU8>` flag 三态)。
- [x] 阶段4:在 `stop` / `cancel` / `Drop` 中 set flag 1 + join thread, 复用现有 stop 路径。
- [x] 阶段5:`@record-status` last_session 暴露 stop_trigger;@record-stop 响应加 trigger 字段。
- [x] 阶段6:6 集成测试 + 1 enum 序列化测试都已加,全过。
- [x] 阶段7:`cargo test --bin rdog` 全过,697 pass (从 690 增加 +7),下一步 commit + push + close #23 + 更新 Wayfinder map。

### 关键设计
- 单 `Arc<AtomicU8>` flag 三态: PENDING(0) / CANCELLED(1) / FIRED(2)。
- timer 线程 100 ms tick poll, 唤醒后检查 flag 或 elapsed。
- **auto-stop 本身在 handler 调用时内联执行**(非 timer thread 内),避免死锁。
- `RecordingHandler::Drop` join all timers before fields drop。
- `0` 视为不传 duration, 不起 timer。
- `last_session_override` 字段让 handler 持有 trigger, 不改 `LifecycleManager` API。

### 验证
- `cargo test --bin rdog`: 697 passed, 0 failed, 1 ignored.
- 新增 7 测试: 6 集成 + 1 enum 序列化。
- E2E smoke 文档已写入 `specs/rdog-acceptance-matrix.md`。
- spec/ADR 已写入 `specs/rdog-recording-auto-stop.md` + `docs/adr/0007-recording-auto-stop.md`。

### 收口
- [x] commit `5d9671a` 已 push 到 `origin/feature/recorder-bundle-delivery`。
- [x] GitHub issue #23 已 close (state: closed, state_reason: completed)。
- [x] Wayfinder map #14 已更新: 加 #15/#19/#20/#22/#23 5 条 decision pointer, 删除原 "Open tickets (frontier)" block (因为都已 close)。

### 交付
- 3 个新文件: `docs/adr/0007-recording-auto-stop.md`, `specs/rdog-recording-auto-stop.md`, `specs/rdog-acceptance-matrix.md`。
- 3 个修改: `src/control_recording/session.rs`, `src/control_recording/control_handler.rs`, `src/control_recording/control_handler_tests.rs`。
- 697 个测试通过 (从 690 增加 +7)。
- 分支 `feature/recorder-bundle-delivery` 现在含 6 commits, 都是 recording 相关。

### 后续 #3 实现: remaining_ms 倒计时
- [x] AutoStopTimer 加 `duration_ms` + `started_at` 字段 + `remaining_ms()` 方法。
- [x] `status` 响应在 active recording 时输出 `duration_ms` + `remaining_ms`。
- [x] 加 2 测试: `status_reports_duration_and_remaining_ms_when_auto_stop_active`, `status_remaining_ms_clamped_to_zero_after_deadline`。
- [x] commit `77b9667` 已 push, 699 tests pass (+2)。
- [x] spec `specs/rdog-recording-auto-stop.md` 已加 `duration_ms` + `remaining_ms` 字段说明。
