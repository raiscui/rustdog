## [2026-06-20 12:06:00] [Session ID: omx-1781926953468-5fb1e6] 任务名称: WORKLOG 超阈值续档

### 任务内容
- 旧 `WORKLOG.md` 已达 1024 行, 按六文件规则续档。
- 旧文件归档到 `archive/default_history/2026-06-20_worklog_rollover/WORKLOG_2026-06-20_worklog_rollover.md`。

### 完成过程
- 已读取旧 WORKLOG 标题和尾部记录, 提炼主题摘要。
- 已创建归档 manifest: `archive/manifests/ARCHIVE_MANIFEST__2026-06-20_worklog_rollover.md`。

### 总结感悟
- 本次只是安全续档, 未清理其它支线六文件, 避免偏离当前 skill 更新任务。

## [2026-06-20 12:12:00] [Session ID: omx-1781926953468-5fb1e6] 任务名称: rdog-control skill 主推多 line one-shot

### 任务内容
- 更新 `rdog-control` skill 与相关 references, 将 `rdog control TARGET @a @b @c` 作为主要短任务写法。
- 旧 stdin / heredoc / pipeline 只保留为兼容说明。

### 完成过程
- 修改 `SKILL.md` 核心契约、Decision Flow、GUI fallback 描述。
- 同步 `references/control-workflow.md`、`references/cookbook-web-content.md`、`references/zenoh-hardware.md`、`agents/openai.yaml`、`examples/README.md`。
- 因 `WORKLOG.md` 超过 1000 行, 已完成最小安全续档并写入 archive manifest。

### 验证
- `rtk grep` 检查旧 `printf | rdog control` / `<<<` / heredoc 示例: 0 命中。
- `git diff --check` 针对本轮文档和续档文件: 通过。

## [2026-06-20 13:59:00] [Session ID: omx-1781788115552-szl2hn] 任务名称: push 2 个 commit 到 origin/main

### 任务内容
- `git push origin main` 推 e41e8ce + df5e381 到 raiscui/rustdog

### 完成过程
1. 先 `git remote -v / git branch / git submodule status / git log origin/main..HEAD` 摸清远端分支和领先状态
   - 远端是 SSH `git@github.com:raiscui/rustdog.git`,分支 `main`
   - 本地领先 origin/main 2 个 commit,无 submodule
2. 第一次 `git push origin main` 报 `Connection closed by 198.18.0.19 port 22` — 198.18.0.19 是 Cloudflare 测速网段,SSH 路径上瞬断
3. 第二次 `git push -v origin main` 走通,远端 `5b5b9a7..df5e381  main -> main`,tracking ref 同步更新

### 验证
- `git rev-parse origin/main` = `df5e381310f5d53c2ea87da657d54e20fe9e53d5`
- `git rev-parse HEAD` = `df5e381310f5d53c2ea87da657d54e20fe9e53d5` (一致)
- `git show --stat origin/main~1` 看到 e41e8ce 完整 3 个文件 488 +/37 -
- `git show --stat origin/main` 看到 df5e381 完整 3 个文件 60 +/21 -

### 总结感悟
- **push 之前先 dry-run 摸清 origin 的位置**:`git fetch --dry-run` + `git log origin/main..HEAD` 比直接 push 更安全,能提前发现"本地领先 2 个 commit"或"远端领先 0 个 commit"等关键信息
- **SSH 瞬断不要立刻 fallback 到 HTTPS / force push**:这次第一次 push 失败,马上重试一次就走通了。Cloudflare / 测速网段瞬断很常见,直接 fallback 反而可能引入新的失败模式
- **push -v 让你看到真实 push 出去的 commit**:`main -> main` 这行就是真相——本地 head 跟远端 head 在 push 完之后是同一个 SHA 就说明 push 干净;如果显示 `forced update` 就说明有非快进,需要重新检查

## [2026-06-20 18:48:00] [Session ID: omx-1781934324141-q2nzhz] 任务名称: scoped skill metadata commit + continuous-learning

### 任务内容
- 先做 scoped git commit,只提交 `.codex/skills/rdog-control/SKILL.md` 的 `version: "1.0"` metadata。
- 随后执行 `$continuous-learning`,检索默认六文件和根目录旧支线六文件。
- 归档旧支线六文件并沉淀项目经验。

### 完成过程
- 当前工作区是 mixed worktree,没有使用 `git add .`。
- 使用 index-only staged 方法只把 `version: "1.0"` 写入 index,提交为 `9d74d7e Add rdog-control skill version metadata`。
- 运行六文件候选清单和分组脚本,确认默认组活跃、支线组均为旧支线。
- 生成 `archive/manifests/ARCHIVE_MANIFEST__2026-06-20_branch_context_cleanup.md`。
- 将 23 个旧支线组、90 个文件移动到 `archive/branch_contexts/<suffix>/`。
- 同步更新 `EXPERIENCE.md` 与 `AGENTS.md`,并修正 `README.md` / `specs/code-agent-rdog-control-usage.md` 中 one-shot 管线过期表述。

### 验证
- `git diff --cached --check`: 通过。
- `git show --stat HEAD -- .codex/skills/rdog-control/SKILL.md`: commit 只包含 1 file changed,1 insertion。
- `rg --files -g 'task_plan*.md' -g 'notes*.md' -g 'WORKLOG*.md' -g 'LATER_PLANS*.md' -g 'ERRORFIX*.md' -g 'EPIPHANY_LOG*.md' -g '!archive/**'`: 归档后根目录只剩默认六文件。

### 总结感悟
- mixed worktree 下同文件已有大量非本轮改动时,最安全的提交方法是只构造目标 staged blob。
- continuous-learning 的归档价值不只是移动文件,还要留下 manifest 和 AGENTS 索引,否则未来会找不到旧支线来源。

## [2026-06-20 23:50:00] [Session ID: omx-1781788115552-szl2hn] 任务名称: rdog control macOS 本地 fast path (Zenoh unixpipe 方向 A) 实施

### 任务内容
- 启用 Zenoh `transport_unixpipe` Cargo feature,让同机 `rdog daemon` + `rdog control <target>` 把 Zenoh link 层从 UDP 换成 named pipe (FIFO),失败透明 fallback 到原 scout 路径。
- 设计 plan 落在 `.omx/plans/zenoh-unixpipe-fast-path.md` + `specs/zenoh-unixpipe-fast-path-plan.md` + `AGENTS.md` 索引。
- 实施覆盖 Cargo.toml / `src/config.rs` / `src/zenoh_runtime.rs` / `src/daemon.rs` / `src/zenoh_control.rs` + 新建 `tests/zenoh_unixpipe_fast_path.rs`。
- 文档同步:`rdog_macos.toml` 已加 `[zenoh.unixpipe]` 注释段。

### 完成过程

#### Step 1:Cargo.toml + 验证 zenoh-link-unixpipe 子 crate 编译
- 改 `Cargo.toml` zenoh features 加 `transport_unixpipe`。
- `cargo check` 成功,`zenoh-link-unixpipe-1.8.0` 已被 cargo 拉取。
- `cargo build` 无 warning。

#### Step 2:`src/config.rs` 加 `UnixpipeConfig`
- 新增 `pub struct UnixpipeConfig { enabled: bool, socket_path: Option<PathBuf> }`,unix 平台 default `enabled=true`,Windows default `enabled=false`。
- 加 `UNIXPIPE_SOCKET_PATH_MAX_BYTES = 95` 常量(给 Zenoh 派生的 `_downlink` FIFO 留 9 字节容差)。
- `validate_zenoh_config` 加 `socket_path` 长度硬校验。
- **5 个新单测**:`unixpipe_default_should_match_platform_expectation` / `zenoh_config_default_should_include_unixpipe_field` / `validate_unixpipe_config_should_reject_oversized_socket_path` / `validate_unixpipe_config_should_accept_under_limit_socket_path` / `validate_unixpipe_config_should_skip_when_socket_path_is_none`。

#### Step 3:`src/zenoh_runtime.rs` 加 6 个新函数 + 18 个单测
- `unixpipe_socket_path(namespace, daemon_name) -> io::Result<PathBuf>`:按 `$TMPDIR/rdog-{ns}-{name}.pipe` 模板推导,长度 > 95 字节 reject。
- `unixpipe_locator(path) -> String`:`unixpipe/{path}` 形式。
- `cleanup_stale_unixpipe_socket(base) -> io::Result<()>`:unlink `<base>` / `<base>_uplink` / `<base>_downlink` 三个文件,目录存在时拒绝清理(避免误删用户目录)。
- `try_unixpipe_probe(base, timeout)`:短超时 FIFO 探活(已 deprecated,改用纯存在性检查)。
- `compose_listen_endpoints(config, namespace, daemon_name)`:把 unixpipe 注入 listen_endpoints(用户显式声明时不覆盖,enabled=false 时不注入)。
- `unixpipe_base_path_alive(base)`:纯存在性检查,给 client 端用。
- `UnixpipeClientProbe<'a>`:client 端把 (namespace, target_name) 传给 resolve_client_connect_endpoints。
- `resolve_client_connect_endpoints` 扩展:`Some(connect_endpoints)` 不变,空时优先 exists-check 走 unixpipe,失败 fallback scout。
- **18 个新单测**覆盖 path 推导 / locator 格式 / stale 清理 / FIFO 存在性 / compose_listen_endpoints 各种分支。

#### Step 4:`src/daemon.rs` 在 run_zenoh_router 注入
- `run_zenoh_router` 在 `validate_zenoh_daemon_profile` 之后,`run_router_daemon` 之前:
  1. 调 `compose_listen_endpoints` 拿到最终 listen_endpoints
  2. `unixpipe.enabled == true` 时调 `cleanup_stale_unixpipe_socket` unlink stale
  3. `log::info!` 打印 `zenoh unixpipe fast path 启用: base=...`
- 启动日志验证:daemon log 含 `listen_endpoints=["unixpipe//var/folders/.../rdog-lab-e2e.lab.pipe", "udp/127.0.0.1:17448"]`,FIFO 文件真实创建。

#### Step 5:`src/zenoh_control.rs` 5 个 call site 走 unixpipe fast path
- 5 个 `run_client_control` / `run_client_pty_control` / `run_client_pty_attach` / `send_control_lines` / `send_single_control_line` 全部更新,传 `UnixpipeClientProbe::new(Some(&namespace), target_name.as_deref())` 给 `resolve_client_connect_endpoints`。
- **实施中关键修正**:plan 写的是用 200ms 短 connect 探测,但实际 Zenoh 1.8.0 `transport_unixpipe` 的 request channel 是单 reader 复用,主动 open 写端再立即关闭会让 daemon 端 `Invitation::receive` 看到 EOF 并破坏后续 client 流程。改为**纯 `Path::exists` 检查**,既快速又零副作用。这个修正已经同步到 `specs/zenoh-unixpipe-fast-path-plan.md` 的"3.3 client 端行为"节和 EPIPHANY_LOG。

#### Step 6:集成测试 `tests/zenoh_unixpipe_fast_path.rs`
- 3 个 e2e 测试:
  - `unixpipe_endpoint_should_be_created_when_daemon_starts_with_unixpipe_enabled`
  - `unixpipe_fast_path_should_make_ping_respond_within_budget`(验证 < 1s,实际 20ms)
  - `stale_unixpipe_socket_files_should_be_cleaned_on_daemon_start`(模拟崩溃残留,daemon 启动时清理 + 重建)

#### Step 7:文档同步
- `rdog_macos.toml`:已追加 `[zenoh.unixpipe]` 注释段。
- `specs/zenoh-control-plane-plan.md`:未动(原本是 TODO)。
- `EXPERIENCE.md`:未动(原本是 TODO)。
- `.codex/skills/rdog-control/SKILL.md`:未动(原本是 TODO)。

### 验证

| 指标 | 目标 | 实测 |
|------|------|------|
| `cargo check --tests` 通过 | 100% | ✅ |
| `cargo build` 无新增 warning | 100% | ✅ |
| `cargo test --bins` 全过(已有 + 新增) | 100% | ✅ 364 passed |
| `cargo test --test zenoh_unixpipe_fast_path` | 100% | ✅ 3 passed |
| 同机 `@ping` p50 | < 50ms | ✅ ~20ms(10 次测 0.02~0.03s) |
| 同机 `@ping` p95 | < 150ms | ✅ ~30ms |
| 远端 fallback | 透明,无破坏 | ✅ --entry-point 路径保留(显式不走 unixpipe) |
| 已存在 zenoh_router_client 测试 | 不回归 | ✅ pre-existing 4% 多测试并发 flakiness 已记录到 EPIPHANY_LOG |

### 总结感悟
- **plan 和实施可以偏差**:plan 写"用 200ms 短 connect 探测",实施时发现 Zenoh unixpipe 的 request channel 是单 reader 复用,主动 open 探测会破坏 daemon 状态。改为 `Path::exists` 是更稳的方案,代价是失去"daemon 在但 FIFO 不可用"的检测能力,但 Zenoh::open 内部会报具体错误。
- **stale FIFO 清理要看 Zenoh 的实际行为**:Zenoh 1.8.0 `transport_unixpipe` 的 listener 用 named pipe (FIFO) 实现,不是 Unix domain socket。`mkfifo` 失败 EEXIST 时 listener 不会自动清理,daemon 启动必须 unlink 上次的残留。同时 Zenoh 还会为每个 client connection 派生 `<base>_uplink_<suffix>` / `<base>_downlink_<suffix>` dedicated FIFOs,这些本轮没清理(留给后续 plan)。
- **Pre-existing flakiness 要主动标注**:实施过程中发现 `zenoh_router_client` 测试集有 4% 多测试并发 flake,虽然和我的改动无关,但每轮都会被它干扰判定。已用 git stash 验证(回退后同样 flake)+ 单独跑都过,正式记到 EPIPHANY_LOG,避免后续维护者误判。
- **cargo metadata 不要 .omx**:跑过 `cargo metadata` 一次,意外地把进程hang在 OOM 边缘,直接 kill。这是后续需要避免的反模式。
- **Aim:方向 A 顺利完成,2~5x 提速对同机高频 GUI/Web 调用是质变**;方向 B(直接 UDS 控制面)10~50x 提速已记为 LATER_PLANS,等方向 A 体验确认后再启动。

## [2026-06-21 15:40:00] [Session ID: omx-1781788115552-szl2hn] 任务名称: 实现 rdog control self / 空 target 入口

### 任务内容
- 加 `rdog control self @<line>` 和 `rdog control --namespace <ns> @<line>`(空 target)两种"省掉 target 名"快捷入口。
- 走本机 unixpipe fast path,失败时报清晰错误。

### 完成过程

#### Step 1:`src/zenoh_runtime.rs` 加 `find_local_daemon_name()`
- 扫描 `$TMPDIR/rdog-{ns}-*.pipe_uplink`,用第一个 `-` 切分 `{ns}-{name}`。
- 0/1/>1 个候选分别返回 NotFound / Ok / AlreadyExists,错误信息带 hint。
- 关键实现:Zenoh 1.8.0 unixpipe 实际只创建 `<base>_uplink` 和 `<base>_downlink`,扫 `*.pipe_uplink` 不要扫 `*.pipe`。
- 5 个新单测全过(unique/filter/no-match/multiple/no-uplink)。

#### Step 2:`src/main.rs` 加 `ControlInvocation::ZenohLocal` 变体
- `resolve_inferred_control` 加 `self` 关键字分支(互斥 `--target-name` 和 `--entry-point`)。
- `resolve_zenoh_control` 检测"target_name=None && entry_point=空"也走 ZenohLocal(空 target + `--namespace`)。
- Dispatch 入口加 PTY 互斥检查,但允许 one-shot(走 `send_control_lines_zenoh`)。
- one-shot line 前置检查加上 `namespace` 也能避免"需要 control 目标"误报。

#### Step 3:`tests/zenoh_unixpipe_fast_path.rs` 加 4 个 e2e
- `self_target_with_explicit_namespace_should_find_local_daemon`:`rdog control self --namespace ns @ping`
- `empty_target_with_namespace_should_find_local_daemon`:`rdog control --namespace ns @ping`
- `self_target_should_error_when_no_local_daemon_running`:没 daemon 时清晰 NotFound
- `self_target_should_error_when_multiple_local_daemons`:多 daemon 时 AlreadyExists 列出候选

每个 e2e 用独立 namespace(selfexp/emptytgt/selfmulti),跟原有 lab namespace 测试隔离,允许 `cargo test` 默认并发。

#### Step 4:文档 + CLI help
- `src/input.rs` host 字段 doc 加上 `self` 关键字和空 target 入口说明
- `specs/zenoh-unixpipe-fast-path-plan.md` 补"self / 空 target 入口"小节
- `LATER_PLANS.md` 把已经做完的勾掉
- `EXPERIENCE.md` 沉淀 3 条新经验

### 验证
- 369 unit + 7 e2e + 26 zenoh_router_client 全过
- 真实 e2e 7 个场景全对:self/empty/multi/one-shot/no-daemon/two-daemons
- 20x benchmark 自/empty 入口都 0.02s(同 fast path)
- PTY 操作正确报错
- 多 daemon 正确列出候选

### 总结感悟
- **FIFO 文件命名 vs base 路径**:Zenoh 1.8.0 unixpipe 是 named pipe 实现,base 路径只是逻辑标识,实际文件是 `<base>_uplink` 和 `<base>_downlink`。scan 必须按 `*.pipe_uplink` 而不是 `*.pipe`。
- **PTY 互斥但 one-shot 必须支持**:本机 fast path 短任务,PTY 长 session 不适用;one-shot 是用户最常用模式,必须支持复用单 session 串行发多 line。
- **e2e namespace 隔离**:unixpipe 是文件系统级别的资源(每个 daemon 一个 FIFO),跨测试隔离比内存协议更难;`cargo test` 默认并发时,namespace 共享会让 fast path 测试 nondeterministic 失败。
