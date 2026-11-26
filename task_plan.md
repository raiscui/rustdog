# 任务计划: rustdog OCR 内容层 (live 阶段)

> 本文件由前档 `archive/default_history/task_plan_2026-09-02_ocr_content_layer_rollover.md` (1684 行, Wayfinder #95 到站 + OCR 第一阶段实现 + live e2e 调试) 续档而来。
> 前档完整记录: 引擎横评反转、#96-#101 六票、TCC/显示休眠排障链。
> 交接文档: /tmp/rdog-ocr-handoff-2026-09-02.md

## 当前状态 (2026-09-02 续档时点)

- 已实现并验证: ocr-oar 引擎 + include_ocr 协议 + rdog.ocr.v1 层 + fail-closed reason code (948 单测绿 + 真机冒烟) + live 三件套 python 闭环一次通过 + overlay 可视化子进程架构
- 已沉淀: solutions 两份新增 (macos-live-gui-testing-environment-gates / appkit-helper-subprocess-isolation) + TCC solution refresh, 校验全过
- 未提交 git: 上述全部改动 (提交前按并行会话纪律)

## 阶段

- [ ] 阶段1: overlay 面板渲染修复 (onscreen=false, 调试入口见 #102 最新 comment: 自定义 NSView drawRect 或 swift 对照实验)
- [ ] 阶段2: rust e2e 连续 3 次稳定通过 (RDOG_OCR_LIVE_E2E=1 [+ _VIA_TERMINAL=1], caffeinate 保持唤醒)
- [ ] 阶段3: #103 陈旧帧亮屏重验 (结论是否维持)
- [ ] 阶段4: cookbook-wechat-ocr.md + git 提交 (拆 OCR 实现 / e2e / 文档)

## 关键前置 (每次跑 e2e 前)

1. 重签: `codesign -f -s - --identifier rdog --requirements '=designated => identifier "rdog"' target/release/rdog` (重编覆盖签名)
2. 屏幕检查: 全屏截图亮度均值 ≈0 = 休眠 (caffeinate -dis 保持)
3. daemon: `open -a Terminal /tmp/ocr99/daemon.command` (OAR_HOME 已含); 模型缓存 /tmp/ocr99/oar_home 勿删

## 做出的决定

- overlay 拆子进程: 非主线程 NSPanel 会 ObjC 异常 abort 整个 daemon (日志铁证), 子进程边界隔离
- 数字定位用按钮区域过滤 + conf>=0.5 + 候选对回退: Calculator 恢复显示值污染定位 + 单帧抖动的防御

## [2026-09-02 18:05:00] [Session ID: sess_68ea20f1-1318-4541-85bc-08d0ca1ddbd8] 状态更新: 交接待办 1/2/3 处理完毕, #103 待受控时段
- [x] git 提交: PR #104 (分支 feat/ocr-content-layer-phase1, 5 commits: 实现/e2e/文档/overlay修复/Linux门控)
- [x] #102 overlay 渲染: 根因 NSPanel+Borderless 不合成, 已修复 (NSWindow) + 真机验证 + 回写; e2e 连跑验收留受控时段
- [x] #103 重验尝试: 现场被用户活动污染中止, 方法学与 composite-crop 发现已回写票
- [ ] cookbook-wechat-ocr.md: 待实现验收 (e2e 连跑) 后写
- 状态: 等 CI 绿; #102 e2e 验收与 #103 重验都需要用户空闲时段配合

## [2026-09-02 19:25:00] [Session ID: sess_68ea20f1-1318-4541-85bc-08d0ca1ddbd8] 状态更新: #103 关票, #102 e2e 四缺陷修复 (验收 open)
- [x] #103 受控重验: 静态 (window 路径已挂 reject_stale_composite_capture 守卫, screenshot.rs:109) + 动态 (内容交替变化 daemon OCR 精确跟随 + daemon 帧 vs 同刻真值窗口区 0% diff) -> 关票, 降级项 (睡眠显示器 fail-closed 显式化) 记 LATER_PLANS
- [x] #102 e2e 深入: 修复 4 个确定性缺陷 (显示区定位误用/tooltip 劫持/持久残留匹配/诊断日志, commit 06ad1d2), 已回写票
- [ ] #102 live 三件套单次绿验收: 需干净环境 (重启后/清理窗口), 剩余问题 = 按钮单帧误读 + 超长残留表达式 + via-terminal 窗口泄漏
- 状态: CI 绿 (unixpipe PID 复用抽签族重跑过, 已记 LATER_PLANS)

## [2026-09-02 20:05:00] [Session ID: sess_68ea20f1-1318-4541-85bc-08d0ca1ddbd8] 状态更新: PR #104 已合并 main
- [x] gh pr merge 104: 远程 merge commit 8c5daec, 远程分支已删
- [x] 本地: main ff 到 8c5daec, feature 分支已删; 六文件收尾记录经 stash 往返保留 (期间三次 UU 冲突均为 stash diff 含 docs commit 内容自撞, 取 stash 超集版解决)
- [x] 工作树遗留: 七文件 M (我的六文件收尾 + 并行会话 solution 相关改动) 保持未提交, 归属各自后续处理
- 状态: OCR 交接待办全部收口; 剩 live 验收 (重启后) 与 cookbook 在 LATER_PLANS

## [2026-09-02 21:15:00] [Session ID: sess_68ea20f1-1318-4541-85bc-08d0ca1ddbd8] 状态更新: #102 终极根因修复, e2e 三连绿, 双票关闭
- [x] #102 终极根因: warp 与紧随 click 之间 WindowServer 收敛不足, click 落旧位置 (纯 click 对照实验实证) -> backend Move 步后 40ms (PR #105 已合并 62ca1b7)
- [x] e2e 配套: Esc 归零 (新版计算器无 AC 按钮) / 稳定候选对 (4,6) / 几何一致性校验 / 串行点击 / 诊断日志
- [x] live 三件套: 三连绿且均第一轮通过 (100s+ -> 12s)
- [x] #102 关票 (遗留: overlay 多屏坐标 / rec small bench, 记票内+LATER_PLANS)
- 教训: 中途一次 git add 误把 index 里并行会话的 staged 六文件打进 commit (9 files/-2816 行), 已在自己新分支上 reset+unstage+强推重写, 未进 main

## [2026-09-02 21:35:00] [Session ID: sess_68ea20f1-1318-4541-85bc-08d0ca1ddbd8] 状态更新: cookbook-wechat-ocr 交付, OCR Wayfinder 全线收口
- [x] references/cookbook-wechat-ocr.md: 完整 recipe (请求形态/boxes 读法/匹配规则/guarded coordinate/fresh 验证/失败路径/红线)
- [x] SKILL.md 指向 cookbook / spec §10 勾选 / AGENTS.md 索引
- [x] 顺带落盘并行会话已校验的两份 solution 及索引 (commit f863be6, 6 files 与清单核对一致)
- OCR 内容层全部挂账清零; 剩余均为 LATER_PLANS 远期项 (overlay 多屏 Y 偏 / rec small bench / OCR ref 化 fog)

## [2026-10-06 12:30:00] [Session ID: rustdog-win-verify-2026-10-06] 记录类型: 新任务 — Windows 11 真机验证 (4K/200% 缩放坐标系批次)

### 目标
在 rais-razer.local (Win11 x64, 4K 物理/1920x1080 逻辑, 200% 缩放) 上部署刚交叉编译的
rdog.exe (target/x86_64-pc-windows-msvc/release/rdog.exe, 43611136 bytes, Oct 6 12:18),
逐项验证 a~f 六项, 只观测不改实现代码。

### 背景
- 本批次核心 = 坐标换算三地基: DPI awareness (7264f6b) + manifest os_logical_bounds (510dc2d)
  + Windows 原生鼠标后端 (b2c3b33); 另有 @ping 结构化 / @cmd 超时 / @window-find 上 Windows /
  @cmd 引号容错。
- 远端旧 daemon (PID 13040, rdog.exe Oct 5 21:13) 占 5555; 计划任务 RdogDaemon2 (ONLOGON)
  会抢端口, 部署前必须删。

### 阶段
- [ ] 阶段1: 部署 — 停旧 daemon / 删计划任务 / scp / 设 RDOG_LOG_FILE / 起 daemon / 验 5555 listen
- [ ] 阶段2: 验证 a (启动横幅) + b (@ping 结构化)
- [ ] 阶段3: 验证 c (@cmd 超时 15s + 超时后控制面存活)
- [ ] 阶段4: 验证 d (@window-find) + f (@cmd 引号容错)
- [ ] 阶段5: 验证 e (截图坐标换算 — 手算 manifest 公式 + click 前后对比截图)
- [ ] 阶段6: 报告

### 硬约束
1. 只观测, 不改 rdog 实现代码
2. 不为让某项"通过"而放宽判据
3. 每项贴真实输出, 跑不了的标"未测"并说原因

### 状态
** 目前在阶段1 ** — 已确认 SSH 通 (cmd.exe 默认壳, 需显式调 powershell) / 旧 daemon PID 13040 占 5555 /
二进制 43611136 bytes 与 src 同版本 (0 个 .rs 更新于二进制)。

## [2026-10-06 12:36:00] [Session ID: rustdog-win-verify-2026-10-06] 状态更新: a~f 六项全部验证完毕, 全绿

- [x] 阶段1 部署: 旧 daemon PID 13040 已停, RdogDaemon2 已删, 新二进制 SHA256 两端一致
      (A2CDE103...), 计划任务 RdogVerify 起 daemon PID 13888 跨会话稳定, 5555 listen
- [x] 阶段2 a 启动横幅 (含 transport/host:port/mode/shell/inbound/outbound) + b @ping 结构化
- [x] 阶段3 c @cmd 15s 超时 + 超时后 @ping 仍活 (控制面存活)
- [x] 阶段4 d @window-find (platform=windows) + f @cmd 引号容错
- [x] 阶段5 e 截图坐标换算: PIL 定位开始按钮图上 (1210,2112) → manifest 公式手算
      os-logical (605,1056) → 点击开始菜单打开 + 按钮高亮 + 37.22% 变化 → 再点关闭,
      终态与初始 0.00% 差异
- [x] 阶段6 报告

### 状态
** 全部完成 ** — 六项全绿, 无失败项。远端跑的是今日 12:18 交叉编译的产物
(SHA256 A2CDE103..., Size 43611136, LastWrite 12:24:26)。

### 遗留 / 需用户决定
- 远端计划任务状态已变更: 原 RdogDaemon2 已删, 新建 RdogVerify (ONCE/23:59 + /Run)。
  rdog.toml 追加了 `[hidden] log_file` 段 (原文件已备份为 rdog.toml.bak)。
  是否恢复/改造成用户原本的 ONLOGON 常驻任务, 由用户定。

## [2026-10-22 14:30:00] [Session ID: mvs_04f490f87a924c96ac1cb0adcbd30806] 状态更新: continuous-learning 完整复盘 (补记 10-07~10-20 缺口)

### 本轮做了什么
- [x] 六文件回读分组 + 23 份 solution + 22 个 open issue + 4 个 worktree 清点
- [x] Scoped Refresh: `remote-control-blocked-command-pitfalls.md` 落地表重判
      (P3-2 / P4 已落地, P1-1 换算口诀已核实且**发现有误**, P0-2 / P1-2 / P3-1
      仍欠账, P2 resize 仍不支持) + 补 2026-10-11 增量节
- [x] Capture: 新建 `docs/solutions/best-practices/electron-main-process-forensics.md`
- [x] 修载体缺陷: skill `references/protocol.md` 的换算公式改为代码权威公式
      (`src/control_mouse/screenshot_frame.rs:17`); SKILL.md 10-11 三招口径校正
- [x] 知识卫生: 24/24 solution 双校验全绿 (修前 2 份 FM 失败 + 5 条 claims flag);
      AGENTS.md 补 2 条遗漏索引
- [x] 归档: `task_plan_2026-09-02.md` / `WORKLOG_2026-09-02.md` → `archive/default_history/`
      + manifest; 未勾选项转出核对无缺口
- [x] 六文件更新 + 延期项移交 issue

### 补记: 10-07 ~ 10-20 的账本缺口
六文件此前停在 10-06, 但期间有实质产出 (10-06 a7ecbec 之后的 10-07/10-08 五个
commit + 10-11 跨机 Electron 调试 + 10-20 跨网互联 wayfinder 建图 13 个 issue)。
本轮已把 10-07/10-11 的内容按证据分级补记进 solution 与六文件; **10-20 的
跨网互联 effort 仍是零本地载体** (13 条已锁定约束只活在 issue #106 正文里),
已记 LATER_PLANS, 等该 effort 自己产出 spec + ADR + 跨网域 glossary 时补齐。

### 状态
- 本轮无业务代码改动 (纯知识载体), 24 份 solution 校验全绿
- 下一步建议见最终报告的项目优化建议三条

## [2026-10-23 20:27:06] [Session ID: 01a168b3-33af-7c53-9c6b-0fb808d7e9dc] 支线索引: yino 使用反馈优化

启用 `yino_rdog_feedback` 支线, 检索 pireel 当前与 archive 上下文, 分析 rdog 需求/缺陷并实施已证实且可验证的调整. 过程见 task_plan__yino_rdog_feedback.md.

## [2026-11-26 06:10:00] [Session ID: RAI-387] 本机 fast path 落盘与生命周期收口

RAI-387: unixpipe 本机 fast path 从 `$TMPDIR` 迁到固定运行目录,补名字派生/死进程回收
与 `rdog local list|clean`。

### 已完成
- [x] 落盘: `$XDG_RUNTIME_DIR/rdog` / `~/.rdog/run`(0700),名字 `<ns>__<name>.pipe`,
      超 95 字节预算压缩成 `<slug>-<8位哈希>.pipe` + `<stem>.meta` 记完整身份;FIFO 0600
- [x] 生命周期: `<stem>.lock`(flock + pid + 启动时刻),拿到 lock 即判定旧 owner 已死并回收;
      正常退出与 SIGINT/SIGTERM 删 FIFO + lock + meta
- [x] router daemon 补信号处理: 原先 SIGTERM 走默认处置打死进程,RAII 清理永不执行
- [x] 命令: `rdog local list`(live / stale-pid-dead / stale-pid-reused / legacy-stale)
      与 `rdog local clean`(默认 dry-run,`--force` 删死残留;活 daemon 一律不动)
- [x] 旧 `$TMPDIR` 残留纳入 list/clean;local-default 未托管候选改读运行目录 + meta
- [x] 验证: 20 轮启停目录回落 0 文件 / kill -9 回执 `pid_alive=no` + `previous_pid_alive=false` /
      60+60 字符身份 base=95 字节且显示全名 / 权限 0700+0600 / `cargo test --bin rdog` 1098 passed
- [x] 文档: spec §3.1 重写 + rdog-control skill 入册 (v2.31-local-run)
- 提交: dce9397 (实现+单测), 98879fc (文档)

### 未覆盖 / 留给后续
- [ ] macOS 真机回执(本机 Linux 已跑真 daemon 验收;mac 由 Captain 代跑同一脚本)
- [ ] Windows 命名管道 fast path 本身(Rai-385);本票只提供 `rdog local list` 观测面,
      该分支仅做隔离类型检查,未在 Windows 上构建(本机 `cargo check --target` 被 ring 的
      lib.exe 挡住,见 docs/solutions/runtime-errors/cross-compile-windows-rust-electron-tool.md)
- [ ] `rdog local list --json` 机器可读形态(当前是 `key=value` 单行,够 agent grep)

### [2026-11-26 06:20:00] 评审 8 条修复
- [x] Windows 构建断裂 (CleanReport 引用 unix-only RunEntry + test 钩子 PathBuf)、
      显式 base 父目录被 chmod、显式 base 被改写扩展名、跨版本互斥丢失、
      legacy 去重丢活 owner 证据、clean TOCTOU、删 lock 允许双 owner、
      macOS 测试夹具超预算 —— 全部修复并补 4 个回归用例
- [x] CI 红点归因: ubuntu/macOS 的 `script_success_should_keep_legacy_bare_stdout_response_shape`
      是既有 figment::Jail 删除 cwd 的并行竞态(8123e21 那次 main CI 同一个测试就红过);
      windows 是 ort-sys 下载 onnxruntime 的 panic(网络,基础设施)
- 提交: 7b4e728

### [2026-11-26 06:25:00] 二轮评审 2 条修复
- [x] (medium) 跨版本互斥单向 → 新 owner 也持 `<base>.rdog-owner.pid` flock + 写 pid(双向闭环)
- [x] (low) legacy 分支 clean 的 TOCTOU → 两条分支都持锁删除
- [x] 配套: 接管路径只清 FIFO(不再连带删自己刚拿到的守卫);锁顺序先新后旧,错误信息更准
- [x] 新增 2 回归用例;`cargo test --bin rdog` 1104 passed
- 提交: 8dbd209

### [2026-11-26 06:30:00] 三轮评审 2 条 low 修复
- [x] `clean` 与 `acquire` 的持锁顺序相反 → 统一"先 stem lock,后兼容守卫"
- [x] Drop 持锁 unlink 兼容守卫留下孤儿 inode 窗口 → 兼容守卫收窄到共享显式 base
      (派生路径不建),运行目录恢复"退出后为空";派生路径上若出现旧守卫只探测不接管
- [x] 新增"派生路径不产生兼容守卫"用例;跨版本用例改用运行目录外的共享 base
- [x] `cargo test --bin rdog` 1105 passed;真机脚本 20/20 干净
- 提交: fcb4680

### [2026-11-26 06:35:00] 四轮评审 + recording e2e / shell_lane 红点对账
- [x] 四轮 low: 兼容守卫改按 base **来源**判定(`UnixpipeRunPaths.shared`),覆盖"显式路径落在运行目录里"
- [x] recording e2e 红点归因(三向对照): cargo 1.100 换 build-dir 布局使 `rdog_binary_path()` 夹具失效;
      main 11-06(rustc 1.99.1, deps 布局)绿、本 PR 与兄弟分支 capy/rai368-scout(同日 1.100.0)同款红;
      base+head 等条件本地各 6 passed
- [x] shell_lane 红点: base 5588fcb 本地 16 线程 5 轮中 4 轮复现 ⇒ 既有 flake(figment::Jail 删 cwd),
      本 PR 只提高了并行压力;根治(共享锁)属另一票
- [x] 回执: handoffs/rai387/regression-check/{README.md,recording-e2e-{base,head}.log,run_recording_e2e.sh}
- 提交: 07a3bcf

### [2026-11-26 06:40:00] 五轮评审 1 条 low 修复
- [x] 上一轮(07a3bcf)漏洞: `paths_for_base` 硬编码 `shared: true`,生产入口
      `prepare_unixpipe_listener` 只会用它 ⇒ **默认 daemon 也在运行目录建 `<stem>.rdog-owner.pid`**
      (实测修前 4 文件),与 spec "派生路径不设这把锁/退出后为空" 矛盾;kill -9 后该文件
      不被 `pipe_stem_from_file_name` 认,`local list` 看不到
- [x] 新增 `is_derived_base(base, ns, name)`;生产入口与 `remove_stale_entry` 都按身份反推来源
- [x] 实测修后: 默认 daemon 3 文件/退出为空;显式 `socket_path=/tmp/.../control.sock` 建兼容守卫、
      base 原样保留、退出全清
- [x] 补用例走生产入口(上一轮用例绕过了它);`cargo test --bin rdog` 1107 passed
- 提交: 9c3fd43

### [2026-11-26 06:45:00] 六轮评审 1 条 low 修复
- [x] 上一轮用 `is_derived_base`(路径相等)推断来源 ⇒ "显式 socket_path 恰等于派生路径"会漏建兼容守卫
- [x] 改为 config 层透传: `ComposedListenEndpoints.unixpipe_base_explicit` → `ZenohUnixpipeStartupConfig.shared`
      → `prepare_unixpipe_listener(_, _, _, shared)`;删除路径值推断
- [x] `remove_stale_entry` 简化为无条件持旧守卫再删(随删除一起消失,不留状态)
- [x] 真机实测: 派生 3 文件 / 显式等于派生 4 文件(含守卫),两者退出后均为空;20 轮启停 20/20
- [x] `cargo test --bin rdog` 1107 passed;补生产入口用例
- 提交: 7bfb8ae

## [2026-11-26] RAI-367 实现阶段 (bulk 原语 + 直连门禁 + 请求级 relay 计数)

前置核实 (Captain 更正后): iroh lane 已在 main (`0ba56845` #141, `8ca3363b` #150),
`src/control_iroh.rs` 存在; 但该 lane 的准入面**只有 ping 与 PTY**, `ping_only` 明说
"bulk 传输不在这条通道上, P4 保持关闭". 因此"直连门禁"今天可实现的形态是: 能力表声明
`path_aware=true / bulk_path_pin=false` → 该 lane 上的 bulk 一律以稳定码拒绝,
而不是放行后再靠采样"尽力而为".

### 阶段 1 — 协议准入与生命周期 (不依赖 iroh, 可独立验收)
- [ ] 1a `[file_transfer]` 四项配置 + 跨字段校验: `max_bytes` / `max_concurrent_sessions`
      (`bulk_session_limit_reached`) / `resume_window_ms` (> `idle_timeout_ms`) / `chunk_ack_timeout_ms`
- [ ] 1b `resume` 改必填 (无默认值); A5 `stale_partial_discarded` 回报
- [ ] 1c 注册表按**暂存路径**键 (C1), 支持 A3/C2 同 id 换路径时保留旧半成品; `is_upload` 布尔退役
- [ ] 1d `attempt_id` 可选字段透传 (阶段 3 的括号采样用它归属)

### 阶段 2 — CLI 流式驱动 (G1)
- [ ] `rdog control <target> --put <local> --remote-path <remote>` / `--get <remote> --local-path <local>`
- [ ] 逐块流式发送, 不在内存里拼整文件; `chunk_ack_timeout_ms` 超时即 `@file-cancel`
- [ ] 检测到窗口内半成品时默认 `resume:true`

### 阶段 3 — 直连门禁与证据 (G2)
- [ ] 传输能力表 (`path_aware` / `bulk_path_pin`); iroh lane 声明 `pin=false`
- [ ] 两端门禁: 上传由客户端拒发, 下载由 daemon 拒读; open + 每个 chunk 边界复查
- [ ] `rdog.bulk-path.v1` 快照: 两端各发一半, 禁替对端填数; `relay_bytes_zero` 判定式
- [ ] 正对照: 同路径 `@ping` 的 relay delta **严格 > 0**

### 验收边界 (不许越界宣称)
- A 类 (直连可用) 等 RAI-366 的拓扑结论; B/C/D 可在 A 之前取证
- 未跑 B/C/D 前不得宣称 bulk 可用
