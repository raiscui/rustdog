## [2026-08-17 16:25:00] [Session ID: omx-1786949079888-fq4u2i] 归档修正: 移除已失效的 LFM2.5 重试建议

### 现象
- `lfm25_ops` 归档 manifest 仍写有“本地服务就绪后重新运行完整矩阵”的未来待办。

### 原因
- 该归档生成时 LFM2.5 仍属于可重试范围,后来用户明确放弃该模型方向。

### 修复
- 删除 manifest 中的重试待办,明确归档只保留历史调查与验证事实。

### 验证
- 已重新阅读 manifest,确认不再存在 LFM2.5 后续执行入口。

## [2026-08-18 10:40:00] [Session ID: omx-1786949079888-fq4u2i] Bugfix: cached AX resource epoch source

### 现象
- AX snapshot 注册缓存时,`with_observation` 已经消费 `resource_epoch_capture`,缓存无法再直接读取 capture-start epoch。

### 原因
- 缓存实现错误地在 snapshot 缺少 capture 时回读当前 resource epoch,可能把旧 observation 误判为新鲜。

### 修复
- 缓存注册改为通过 `resolve_observation_resource_epoch` 从 observation store 获取创建时资源 epoch。

### 验证
- 新增缓存 round-trip、resource write 后 stale、unknown observation/ref 和 executor cache-hit 测试。
- `cargo check -j 2` 通过;定向测试通过;串行 `cargo test -j 2 -- --test-threads=1` 通过 924 项。

### 未完成边界
- 并行全量测试仍会触发现有全局 observation store 容量竞态;串行全量测试是本次完整验证口径。
## [2026-08-18 17:50:00] [Session ID: omx-1786949079888-fq4u2i] Qwen DashScope key 注入路径误判

### 现象
- Qwen 3.7 和 Qwen 3.6 首轮 suite 各为 0/8,artifact 显示 provider 未找到模型 API key。

### 候选假设
- 初始候选是 `DASHSCOPE_API_KEY` 缺失。备选解释是运行 runner 的 shell 没有加载仓库的 direnv 环境。

### 验证
- 检查 `.envrc` 发现它加载 `.envrc.private`。
- `direnv exec .` 动态读取到 `DASHSCOPE_API_KEY=present`。
- 使用 `direnv exec .` 启动 daemon 并重跑同一 runner: Qwen 3.7 和 Qwen 3.6 均 `successCount=8`,`runCount=8`。

### 结论
- "API key 不存在" 的初始假设被动态证据推翻。已确认问题是 shell 未继承 direnv,而非 DashScope 凭据缺失或 rdog 控制链故障。
- 后续 Qwen 测试命令必须显式使用 `direnv exec .`。

## [2026-08-19 15:23:42] [Session ID: omx-1787115582924-n1rbi7] durable observations 根目录无界增长

### 问题
- 默认 observation 根累计 7,510 个一级目录,大部分是没有 observation/selector 的测试空 store。
- 旧日期 cleanup 在收尾审查中发现会删除任意子目录,存在未知数据误删风险。

### 原因
- durable store 按 daemon name 隔离,旧 `open` 在 daemon 启动时立即物化目录和空文件。
- 普通集成测试生成一次性 daemon identity,却继承真实 HOME 且未关闭 durable observation。
- 单 store count/byte retention 不限制 sibling store 数量。
- cleanup 删除前缺少 `looks_like_observation_store` 识别门槛。

### 修复
- 新 store 延迟到第一次 record 才物化。
- 默认 store 使用日期目录并跨日移动同一状态;每小时清理 7 天前 store。
- root maintenance lock 与 daemon owner lock 保护移动和活动状态。
- 未知子目录只记录 `skipped_unknown_stores`,不进入删除分支。
- 普通集成测试关闭 durable observation,专项测试使用临时 `state_dir`。
- 旧空测试 store 经 dry-run、quarantine 和逐项复核后删除。

### 验证
- cleanup 未知目录测试 1/1,durable 测试 12/12,observation/config 测试 42/42。
- quarantine 复核 7,422 项,0 错误;68 个含 observation 的目录全部保留。
- 全量 nextest 936/936 通过;真实 HOME 根保持 88 个目录、23 MiB。

## [2026-08-19 15:23:42] [Session ID: omx-1787115582924-n1rbi7] 全量验证中的测试时钟与进程隔离错误

### 问题
- observe epoch 测试要求两次独立 wall-clock 取值相差不超过 1ms,全量和 exact 均稳定失败。
- recording E2E 并行时收到空 response,失败后还会遗留 daemon 子进程。

### 原因
- epoch 契约只要求等于 primary observation 创建时间,没有 1ms response 组装时限。
- recording 使用 host-global capture 能力,但 E2E 没有跨线程/跨进程串行保护;panic 又跳过显式 cleanup。

### 修复
- 删除无规格依据的 1ms 接近性断言,保留 epoch 等值和 fallback 等值测试。
- recording E2E helper 获取测试专用文件锁;guard Drop 在异常路径 kill/wait 子 daemon。

### 验证
- observe exact 1/1 通过。
- recording 普通并行 cargo test 5/5,nextest 5/5。
- 最终全量 nextest 936 passed,21 skipped;遗留的两个旧测试 daemon 和临时目录已清理。
## [2026-08-20 17:25:00] [Session ID: omx-1787115582924-n1rbi7] 问题: #54 successor changes contract 与 resource epoch

### 现象
- `@computer-act` 已返回 successor observation/target,但没有接入 #53 的 trusted changes decision。
- `successor_target.epoch` 使用 observation 创建时间,而下一次 PID-backed mutation 实际校验 resource epoch。
- 初版接入让 verify 与 changes-first 各自计算一次完整 AX diff。

### 原因
- pre snapshot 只为 verify 采集,且没有 observation metadata,无法通过 trusted identity gate。
- successor target 构造函数复用了 `created_at_unix_ms`,没有从 `observation_id + ref` 解析 resource lane snapshot。
- verification 与 changes-first 原先是两条独立消费路径,没有共享 #53 已产出的 `DiffReport`。

### 修复
- pre/successor 继续各采集一次,两者都记录 observation;同一对 snapshot 生成 changes/full/unavailable。
- successor target 从 observation store 读取对应 ref 的稳定 resource epoch。
- trusted changes 的 `DiffReport` 直接供 best-effort/always verification 使用;full fallback 才按需补算。
- executor 统一装配 successor、changes、postcondition 和 outcome;after capture 缺失时返回 unavailable/unknown,不伪造 successor。

### 验证
- `cargo check -j 2`: 通过,无 warning。
- 定向测试: computer-act 31/31、verify 18/18、changes-first 7/7。
- `cargo nextest run -j 2 --no-fail-fast`: 945 passed,21 skipped。
- 双轴复审: Standards `APPROVE`,Architecture `CLEAR`。
- solution frontmatter/claims: 通过,0 flags。

## [2026-08-20 18:05:00] [Session ID: omx-1787115582924-n1rbi7] 问题: cached AX query 生命周期错误码不稳定

### 现象
- cached @ax-get 对不存在 ref 或过期 observation 直接透传 observation store 的 STALE_REF / OBSERVATION_EXPIRED。

### 原因
- resolve_cached_ax_get 在读取 ref 前没有把内部 observation 生命周期错误映射到 cached query 的公共错误契约。

### 修复
- 在共享 cached query 入口统一映射: STALE_REF -> target_not_found,其他 observation 生命周期错误 -> stale_observation_cache。
- 增加 helper 与 executor 两层动态回归测试。

### 验证
- cargo test -j 2 --bin rdog control_ax::tests::cached_ax_get -- --nocapture: 4 passed。
- cargo test -j 2 --bin rdog control_actions::tests::cached_ax_get_executor -- --nocapture: 2 passed。
- cargo nextest run -j 2 --no-fail-fast: 949 passed、21 skipped。

### 测试环境发现
- `cargo test -j 2 --bin rdog` 的同进程并行测试会共享 64-entry observation singleton,使长时间 direct-ref 测试的 observation 被驱逐。exact 单跑通过,nextest 进程隔离全量通过。
- 该既有隔离问题已记录到 `LATER_PLANS.md`,没有通过扩大 production capacity 掩盖。

## [2026-08-21 18:00:00] [Session ID: current] 源码脚本化编辑的两个定位错误

### 错误 1：跨行正则插入属性，匹配到了错误的目标

**现象**
给 `perform_default_ax_press_sequence` 加 `#[deprecated]` 后，编译输出 11 个
内容错乱的警告：`use of deprecated struct control_ax::AxObservationCacheEntry:
use ax_action::press_sequence instead`。

**原因**
脚本用了这个模式定位函数：
```python
pattern = r'(///.*?\n)*pub fn perform_default_ax_press_sequence\('
match = re.search(pattern, s, re.DOTALL)
s = s[:match.start()] + deprecated + s[match.start():]
```
`(///.*?\n)*` 配 `re.DOTALL` 时，`.` 能跨行匹配，导致 `match.start()` 回退到了
文件前部 `AxObservationCacheEntry` 的文档注释开头，而不是目标函数上方。
属性被插到了那个结构体上。

**修复**
改用逐行扫描精确匹配：
```python
for i, line in enumerate(lines):
    if 'pub fn perform_default_ax_press_sequence(' in line:
        lines.insert(i, deprecated)
        break
```

**规律**
在源码里插入属性/注解，用**行匹配**而不是跨行正则。跨行正则在长文件里
很容易把"目标上方的文档注释"匹配成"上游某个结构体的文档注释"。
`re.DOTALL` + `*` 量词的组合尤其危险。

### 错误 2：`rindex("}")` 不等于目标 mod 的结尾

**现象**
往 execute.rs 追加测试后编译报
`error: expected one of '.', ';', '?', '}', or an operator, found doc comment`。

**原因**
脚本用 `s.rindex("}\n")` 找"最后一个大括号"作为插入点，假设它是 `mod tests` 的结尾。
但当时文件末尾是 `perform_press_sequence_with` 这个函数（它在 tests mod 之后定义），
所以测试被插进了函数体内部。

**修复**
把误插块切出来，函数体正确收尾，测试包进新的 `mod press_sequence_tests`：
```python
mi = s.index(marker)
test_block = s[mi:]
s = s[:mi].rstrip() + "\n}\n"          # 函数收尾
s += "\n#[cfg(test)]\nmod press_sequence_tests {\n    use super::*;\n\n" + test_block
```

**规律**
追加测试时，**在文件末尾新建一个 `#[cfg(test)] mod`**，而不是试图插进已有 mod。
Rust 允许一个文件有多个 test mod，新建 mod 是零风险操作；
定位已有 mod 的闭合括号则需要括号配对分析，脚本里做不可靠。

### 共同教训
两个错误都是"用文本位置猜代码结构"。脚本化编辑源码时，
能用行级唯一标识（`pub fn xxx(` 这一行）就不要用跨行模式；
能追加到文件末尾就不要往中间插。改完立刻编译，不要连续做多个位置猜测。

## [2026-08-28 15:10:00] [Session ID: current] control_tty 箭头键测试 "假 flake" 根因修复

### 现象
- tests/control_tty.rs::control_cli_should_treat_arrow_keys_as_local_cursor_motion_in_tty
  断言失败: 远端收到 "@png\u{1b}[D\u{1b}[Di\u{1b}[C" (期望 "@ping"),
  方向键 ESC 序列全部原样透传到控制行
- 2026-08-19 起多轮全量门禁都带此失败, 被标记为 "疑似 TTY 时序竞态的 flake";
  实际在非交互环境里它是稳定失败, 在用户交互终端里是稳定通过

### 原因 (受控实验确认)
- 测试经 `script -q /dev/null` 提供 PTY, rdog CLI 的 stdin 在 PTY 内确为终端
  (实验: pipe 进 script, 内部 `test -t 0` 仍为 true), 行编辑器 rustyline 路径正常启用
- 真正根因: rustyline 对 TERM=dumb 降级为无 raw mode 的整行读取, 方向键序列
  不做本地解释 -- 这是正确的生产行为 (dumb 终端确实不支持行编辑)
- agent shell / CI 等非交互 harness 的 TERM 恰好是 dumb, 而用户终端是
  xterm-256color 等, 于是同一测试在两种环境里确定性相反, 被误判为 flake
- 验证: TERM=xterm-256color 跑测试 -> PASS; TERM=dumb 跑测试 -> FAIL;
  逐字节比对确认失败输出与非 TTY 读取路径 (for_each_buffered_line) 的行为吻合

### 修复
- 测试侧显式固定环境: Command 增加 .env("TERM", "xterm-256color"),
  与调用环境解耦。测试的意图就是模拟支持方向键的交互终端,
  继承 harness 的 dumb TERM 等于自我 sabotages 模拟前提
- 生产代码零改动 (rustyline 的 dumb 降级是正确行为, 不修)

### 验证
- TERM=dumb 下单测 PASS, 默认环境单测 PASS
- 全量 nextest 959/959 passed, 21 skipped -- 本分支工作以来首次完整绿灯

### 教训
- "在交互终端里是绿的" + "在 agent/CI 里是红的" = 环境决定性失败, 不是 flake;
  诊断入口是让测试显式固定它假设的环境 (TERM/LANG/TTY), 而不是调时序
- 失败输出与哪条代码路径的产物逐字节吻合, 是快速锁定分支的证据
  (本次 ESC 透传 == for_each_buffered_line 的输出形状, 直接排除 raw-mode 竞态)

## [2026-08-28 15:55:00] [Session ID: current] GNU coreutils kill 参数歧义致进程组信号丢失 (ubuntu CI shell_lane 2001ms)

### 现象

- PR #62 (fix/ci-linux-xcap-deps) ubuntu Build 修通后首次跑到 unit tests,
  `shell_lane_should_mark_timeout_and_continue_to_expect` 挂:
  `duration_ms: 2001, timed_out: true, exit_code: None` — 超时标记对但整体拖满 2 秒

### 原因 (动态实证链)

1. control_flow/process.rs 的 terminate_process_tree 用外部命令
   `Command::new("kill").args(["-TERM", "-<child_id>"])` 发进程组信号
2. docker ubuntu 24.04 + python 复刻 process.rs 完整逻辑:
   TOTAL duration=2.008s, stdout_join_wait=1.937s, rc=-9
   → sh(dash) 被 child.kill() 杀, sleep 成孤儿持有 stdout 管道写端,
   join_stream_reader 阻塞到 sleep 自然退出
3. strace 决定性证据: `/usr/bin/kill -TERM -2555` 实际执行
   `kill(-2, SIGTERM) = -1 ESRCH` — GNU coreutils 把参数解析成
   "信号发往进程组 2", 从未触达目标进程组
4. macOS 的 BSD kill 对同参数正确解析负 pid, 所以 macOS 从不暴露

### 修复 (PR #62 e1f61dc)

- terminate_process_tree (unix) 改 `libc::kill(-pgid, signum)` 进程内直发,
  消灭外部命令参数解析歧义 + PATH 依赖 + spawn 开销三重脆弱点
- Cargo.toml unix 段加 libc = "0.2" (依赖树已有版本 0.2.184, 零成本)

### 验证

- macOS: cargo check 过, shell_lane + open_app 相关 9/9 绿 (nextest)
- linux: CI e1f61dc 轮 (待出结果, 后续回填)

### 教训

- 外部命令做信号发送是脆弱间接层; 进程组语义直接用 syscall 最正确
- "带 -- 才可靠": `kill -s TERM -- -<pgid>` 在 GNU 上可用, 但跨 BSD/GNU
  仍有解析差异风险, 一律优先 libc::kill
- CI 每前进一步会暴露下一层存量问题 (Build 修好暴露链接, 链接修好暴露
  unit tests), 修复链要有耐心逐层实证
