# local-default legacy退役研究笔记

## [2026-07-18 12:54:20] [Session ID: omx-1784340333160-6bwnss] 笔记: legacy职责拆分

### 结构证据

- `ProcessLease::acquire`的PID检查不是managed liveness来源.它只在sidecar缺失/不匹配时保护仍运行的pre-lease daemon.
- `LocalDefaultDaemonRecord::owner_is_active`的`process_exists`是client正常发现fallback,会把纯v1 registry视为有效owner.
- 非Unix service guard仍使用create-new + stale PID unlink;本轮目标限定为已经有动态验证基础的Unix local-default/unixpipe路径.

### 部署证据

- 本机只有一个可执行副本:`/Users/cuiluming/.cargo/bin/rdog`.
- cargo安装源是当前工作树`rustdog v3.0.0`.
- 当前daemon PID 29465由tmux session持有,没有LaunchAgent/LaunchDaemon副本.

### 新候选风险

- `find_local_daemon_name`在registry无有效结果时还会扫描`$TMPDIR/rdog-*.pipe_uplink`.
- 因此只拒绝v1 registry可能仍通过唯一FIFO找到旧daemon;managed-only必须同时审计并可能退役该fallback.

## [2026-07-18 13:02:44] [Session ID: omx-1784340333160-6bwnss] 笔记: commit 893398c旧版/新版动态矩阵

### 隔离环境

- 旧版来源:detached worktree commit`893398c`.
- namespace:`lr`;daemon name:`d.lr`;UDP:`127.0.0.1:18447`.
- state/TMPDIR都位于`/tmp/rdog-legacy-matrix-*`,没有触碰正式PID 29465.

### 验证结果

1. active旧版PID 64217运行时,新版启动exit 1,在service legacy PID guard处fail-closed.
2. 当前新版client读取纯v1 registry并通过FIFO返回pong,证明legacy仍是正常发现路径.
3. SIGKILL旧版后,新版在三个原inode`512218781/786/788`上启动成功,registry/sidecar升级为`rdog.process-lease.v1`,PID 66308,ping成功.
4. active新版运行时,旧版启动exit 1,说明兼容PID能保护已经完成发布的managed owner.
5. SIGKILL新版后再启动旧版,三个guard inode变为`512218956/959/960`,registry被覆盖回纯v1,动态证明旧版执行了stale unlink.

### 已验证结论

- stopped legacy可以由新版原地迁移,无需unlink stable lock path.
- active legacy PID检查必须保留为升级安全门,不能把它误删成"旧liveness".
- 纯v1 registry与唯一FIFO扫描必须从空target正常路径同时退役,否则旧daemon仍会被自动选择.
- 旧二进制清理是部署验收条件;新版无法强制一个旧可执行文件遵守OS lease.

## [2026-07-18 13:09:31] [Session ID: omx-1784304547353-h5409r] 笔记: managed-only TDD RED

### 测试缝隙

- `find_local_daemon_name(Some(namespace))`是空target/self进入Zenoh前的本地owner解析边界.
- 用纯v1 registry + matching FIFO覆盖PID fallback与后续FIFO fallback的组合路径.
- 用无registry + 唯一FIFO单独覆盖FIFO自动选择路径.

### 动态证据

- `find_local_daemon_name_should_reject_legacy_registry_even_with_matching_fifo`:exit 101.
- `find_local_daemon_name_should_reject_unique_unmanaged_fifo`:exit 101.
- 第二个用例关键panic:`unwrap_err()`实际得到`Ok("findme.findunique")`.
- 两个失败均发生在期望错误、实际成功的断言点,不是FIFO创建、临时目录或编译fixture故障.

### 结论

- 主假设成立:纯v1 PID fallback和唯一FIFO自动选择确实参与当前成功路径.
- 正式修复应同时关闭两条路径;只改其中一个无法让legacy组合用例转绿.

## [2026-07-18 13:13:58] [Session ID: omx-1784304547353-h5409r] 笔记: runtime回归测试与格式化边界

### runtime测试

- 首轮完整runtime结果:29 passed,5 failed.
- 5个失败都来自旧契约断言:stale/missing registry回退唯一FIFO、namespace唯一FIFO成功、multiple FIFO返回`AlreadyExists`,以及用纯v1 fixture模拟multiple valid registry.
- 更新后使用真实managed guards覆盖active owner和启动宽限期,unmanaged FIFO只验证诊断;结果34 passed,0 failed.

### 工具错误与恢复

- `cargo fmt -- src/zenoh_runtime.rs`实际格式化了24个不相关源码文件,超出scoped边界.
- 执行前`git status`证明这些文件均为clean;随后使用显式文件清单恢复,未触碰用户已有的`LATER_PLANS__local_default_atomic_lease.md`、`task_plan.md`或本支线文件.
- 后续不再用该命令做单文件格式化;只使用直接`rustfmt <file>`或在最终验证阶段明确审计全局fmt影响.

## [2026-07-18 13:21:18] [Session ID: omx-1784304547353-h5409r] 笔记: 自动验证矩阵

### 通过项

- process lease单元测试:4 passed.
- runtime单元测试:34 passed.
- unixpipe e2e:12 passed.
- router-client:26 passed,2 ignored.
- all-targets check:0 error.
- release build:0 error.

### warning边界

- check/build报告10条warning,全部来自未改动的`control_actions`和`control_computer_act`基线.
- 本轮修改文件没有新增warning;按既定范围不混入control-act清理.

### 命令误用

- `rtk find`不兼容原生`find -print`,忽略参数后输出了大量历史guard文件并被截断.
- 改用`rtk proxy find`和精确name过滤后得到可信正式guard清单;未修改任何状态文件.

## [2026-07-18 13:29:33] [Session ID: omx-1784304547353-h5409r] 笔记: 文档同步遗漏纠正

- 前一次旧措辞搜索只列举了已知主文件,没有覆盖skill references和第二份control-plane规格.
- 新搜索找到两处真实过期内容:`specs/zenoh-control-plane-plan.md:123-128`和`.codex/skills/rdog-control/references/control-workflow.md:28-89`.
- 上一条"旧措辞为0"结论不成立,原因是验证集合不完整,不是新改动后出现回退.
- 修正后必须使用repo-wide `rg`并排除`archive/target`,不能再用手工文件清单证明"全仓清零".
