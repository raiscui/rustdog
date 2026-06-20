# 计划: rdog control macOS 本地 fast path (Zenoh unixpipe 方向 A)

## 目标

让 macOS / Linux 上同机的 `rdog daemon` + `rdog control <target-name>` 把 Zenoh link 层从 UDP loopback 换成 Unix domain socket(`unixpipe` transport),把一次 `@ping` / `@web-find` / `@screenshot` 的 round-trip 延迟从 200~500ms 量级压到 50~150ms 量级;远端 / 跨主机保持 UDP/TCP 透明 fallback。

**不做的事**(留给方向 B / C,本轮不引入):
- 不在 `ControlTransport` enum 里新增 `UnixSocket` 变体(那是绕过 Zenoh 的本地直连)
- 不引入共享内存、io-uring、process group leader 之类的"真内存 IPC"
- 不改 line-control 协议本身

## 设计原则

1. **zenoh 协议层不变**:query / reply / liveliness / `@savefile` 多 frame 全部沿用当前 Zenoh 实现,unixpipe 只是 link 层换协议。
2. **本机 fast path 必须透明**:不引入新的 CLI flag,`rdog control <target>` 不需要加任何参数就走 unixpipe。
3. **远端 fallback 必须可靠**:unixpipe 路径不可达时,client 必须自动 scout 走 UDP/TCP,不能让 fast path 把远端场景搞坏。
4. **路径要确定可推导**:`(namespace, daemon_name)` 在 daemon 端和 client 端都唯一决定一条 unixpipe 路径,这样不需要额外的 control-plane "发现 socket 路径" 步骤。
5. **stale socket 必须清理**:进程异常退出留下的 socket 文件,启动时必须能识别并 unlink,避免 `Address already in use`。
6. **配置文件最小侵入**:不强制所有 daemon 配 unixpipe,只在不显式禁用时自动加入 listen_endpoints。
7. **Windows 行为不变**:`transport_unixpipe` 在 Windows 上要么用 zenoh-link 的兜底,要么编译期就关掉,本轮不在 Windows 上验证。

## 范围

### 范围内

- 启用 zenoh `transport_unixpipe` Cargo feature。
- 新增 `ZenohConfig::unixpipe: UnixpipeConfig` 子结构,允许显式 disable / 显式指定 socket 路径。
- 新增 unixpipe socket 路径推导函数,平台默认:
  - macOS: `$TMPDIR/rdog-{namespace}-{daemon_name}.sock`(macOS 的 `$TMPDIR` 是 per-user,自然隔离权限)
  - Linux: 同样优先 `$TMPDIR`,回退 `/tmp/rdog-{namespace}-{daemon_name}.sock`
  - 路径总长必须 ≤ 100 字节(macOS `sun_path` 限制 104,留 4 字节给 `\0` + 容差)
- `daemon` 启动时:
  1. 先 `unlink` 旧 socket 文件(stale cleanup)
  2. 把 `unixpipe/{path}` 注入到 `zenoh.listen_endpoints`(如果用户没显式禁)
  3. 把 socket 文件路径写入启动日志,便于排错
- `client` 端:
  - 在 `resolve_client_connect_endpoints` 之前,先按 `(namespace, daemon_name)` 算出预期 unixpipe 路径
  - 用很短的 connect timeout(默认 200ms)尝试 Unix domain socket connect
  - 成功 → 把 `unixpipe/{path}` 作为唯一 connect endpoint 传给 zenoh::open
  - 失败 → 走原来的 autodiscover_router_endpoints 路径
- 至少 4 个新增单测 + 1 个新增 e2e 集成测试。

### 范围外

- 不实现"绕过 Zenoh 的 UDS 控制面"(那是方向 B,放在后续 plan)
- 不重命名 `rdog_macos.toml` / `rdog_linux.toml` 模板文件名
- 不改 `--namespace` / `--target-name` CLI 默认值
- 不动 `@savefile` 多 frame 协议
- 不在 Windows 上落地 `unixpipe` 自动启用
- 不写 `dirs` crate 之类的跨平台 runtime_dir 解析(本轮用 `std::env::var("TMPDIR")` + 兜底 `/tmp`)

## 设计决策

### 决策 1: socket 路径默认用 `$TMPDIR/rdog-{namespace}-{daemon_name}.sock`

**理由**:
- macOS 的 `$TMPDIR` 是 per-user(例如 `/var/folders/xx/yy/T/`),自然提供权限隔离,不需要额外 chmod。
- Linux 上 `$TMPDIR` 不一定存在,直接 `/tmp` 兜底。
- `(namespace, daemon_name)` 双方都已知,推导稳定,不需要 Zenoh liveliness 二次查询。
- 路径生成函数纯函数,易测。

**风险**:
- macOS `sun_path` 104 字节上限。`/var/folders/.../T/rdog-lab-mac.lab.sock` 一般 50~80 字节,留足余量。
- 异常退出后遗留 socket 文件,必须 stale cleanup。

### 决策 2: client 端用"先 unixpipe connect + 短超时,失败再 scout"的两段式

**理由**:
- 同机场景:unixpipe connect 100us 内成功,几乎零延迟。
- 远端场景:unixpipe connect 在 timeout 内失败(因为路径不存在),转入 scout 路径,行为完全不变。
- 短超时(默认 200ms)兜底了"路径存在但 daemon 死锁"这种 corner case,不会让用户多等很久。

**风险**:
- 200ms 在高频调用(`@screenshot` 一帧 1~3 次)时会累积。需要让超时可配,且默认值要保守(150~250ms)。
- unixpipe connect 在 daemon 半死(进程在、accept 卡死)时也会 timeout,会拉长 round-trip,但 200ms 上限可控。

### 决策 3: unixpipe feature 默认 `enabled = true` 在 macOS / Linux,Windows 不开

**理由**:
- macOS / Linux 是 unixpipe 真正有意义的平台。
- 显式 `unixpipe.enabled = false` 可以关掉,用于调试或权限限制场景。
- Windows 编译期走 `#[cfg(unix)]` 包裹,避免在 Windows 上跑出奇怪问题。

### 决策 4: 不动 `rdog control <target>` 的 CLI

**理由**:
- unixpipe 对用户透明,符合"fast path 是默认"的产品直觉。
- 需要排查时可以走 `RUST_LOG=debug` + 日志看到 `unixpipe connect succeed, skipping UDP scout` 这类信息。
- 真正需要显式控制的人可以走 `--entry-point unixpipe/<path>` 或 `--entry-point udp/<host>:<port>`。

## 实现步骤

### Step 1: Cargo.toml 增加 zenoh transport feature

**文件**: `Cargo.toml`

**改动**:
```toml
zenoh = { version = "1.8.0", default-features = false, features = [
    "transport_serial",
    "transport_tcp",
    "transport_udp",
    "transport_unixpipe",
] }
```

**验证**:
- `cargo check` 必须通过。
- 预期会下载 `zenoh-link-unixpipe` 子 crate。

### Step 2: 新增 `UnixpipeConfig`

**文件**: `src/config.rs`

**结构**:
```rust
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct UnixpipeConfig {
    /// 是否启用 unixpipe endpoint。
    /// macOS / Linux 默认 true,Windows 编译期强制 false。
    pub enabled: bool,
    /// 显式覆盖 socket 路径。None = 按 (namespace, daemon_name) 自动推导。
    pub socket_path: Option<PathBuf>,
}
```

**改动要点**:
- `ZenohConfig` 增加 `pub unixpipe: UnixpipeConfig` 字段。
- `UnixpipeConfig::default()` 在 unix 平台 `enabled = true`,Windows 平台 `enabled = false`(`#[cfg(unix)]` / `#[cfg(windows)]` 双 impl Default)。
- `validate_zenoh_config` 加上 `unixpipe.socket_path` 路径长度 ≤ 100 字符的硬校验。

**验收**:
- 单测 `unixpipe_default_should_be_enabled_on_unix_disabled_on_windows`。
- 单测 `validate_zenoh_config_should_reject_oversized_unixpipe_socket_path`。

### Step 3: 新增 socket 路径推导函数

**文件**: `src/zenoh_runtime.rs`(新模块或新函数)

**接口**:
```rust
pub fn unixpipe_socket_path(namespace: &str, daemon_name: &str) -> io::Result<PathBuf>
pub fn unixpipe_locator(path: &Path) -> String  // 返回 "unixpipe/{path}" 形式
pub fn cleanup_stale_unixpipe_socket(path: &Path) -> io::Result<()>
```

**实现要点**:
- 路径模板: `{tmpdir}/rdog-{namespace}-{daemon_name}.sock`
- `tmpdir` 解析优先级: `TMPDIR` 环境变量 > `/tmp`(macOS 几乎永远命中第一档)
- 推导完后 canonicalize(避免 `/tmp` vs `/private/tmp` 不一致,macOS 上 `/tmp` 是 `/private/tmp` 的 symlink)
- 总长度必须 ≤ 100 字节,否则返回 `ErrorKind::InvalidInput` + 明确消息
- `cleanup_stale_unixpipe_socket`: 尝试 `unlink`,如果文件不存在则忽略(`NotFound` 不算错),如果是目录则返回 `ErrorKind::AlreadyExists`

**验收**:
- 单测 `unixpipe_socket_path_should_respect_tmpdir_env`
- 单测 `unixpipe_socket_path_should_fallback_to_slash_tmp`
- 单测 `unixpipe_socket_path_should_reject_oversized_combination`
- 单测 `cleanup_stale_unixpipe_socket_should_remove_file_but_ignore_missing`

### Step 4: daemon 端把 unixpipe 注入 listen_endpoints

**文件**: `src/daemon.rs::run_zenoh_router`、`src/zenoh_control.rs::run_router_daemon`

**改动要点**:
- 在 `run_zenoh_router` 校验 `validate_zenoh_daemon_profile` 之后、调用 `run_router_daemon` 之前:
  - 如果 `config.zenoh.unixpipe.enabled`,计算 socket 路径,调用 `cleanup_stale_unixpipe_socket`,把 `unixpipe/{path}` 拼到 `listen_endpoints` 列表(放在最前)
  - 如果 `listen_endpoints` 已经包含 `unixpipe/`,跳过注入(用户显式控制时不覆盖)
  - 把最终 socket 路径通过 `log::info!` 打出来
- 在 `run_router_daemon` 的"zenoh router daemon ready"日志里,把 unixpipe socket 路径作为单独字段打印

**验收**:
- 单测 `run_zenoh_router_should_inject_unixpipe_endpoint_when_enabled` (需要 mock zenoh session,可以测纯函数 `compose_listen_endpoints`)
- 把 `compose_listen_endpoints(zenoh_config, namespace, daemon_name) -> Vec<String>` 抽成纯函数,方便测
- 集成测试:`rdog daemon -c rdog_macos.toml`,日志中能直接看到 `unixpipe_endpoint: /var/folders/.../rdog-lab-mac.lab.sock`

### Step 5: client 端先尝试 unixpipe,失败再 scout

**文件**: `src/zenoh_runtime.rs::resolve_client_connect_endpoints`、`src/main.rs`、`src/zenoh_control.rs`

**改动要点**:
- 在 `resolve_client_connect_endpoints` 增加一个早退路径:
  - 调用方传入 `(namespace, target_name, control_timeout_ms)`,如果 `target_name.is_some()` 且 unixpipe feature 编译期开启,先 `UnixStream::connect_timeout(&path, Duration::from_millis(200))`
  - 成功 → 立刻 drop 这个 stream,返回 `Ok(vec!["unixpipe/{path}".to_string()])`
  - 失败 → 走原来的 scout 路径
- `unixpipe_connect_timeout_ms` 配置项加到 `ZenohConfig` 的 client 端(也作为环境变量 `RDOG_ZENOH__UNIXPIPE_CONNECT_TIMEOUT_MS`)
- main.rs 入口把 `target_name` 和 `namespace` 透传给 `resolve_client_connect_endpoints`
- 启动日志:`unixpipe connect succeeded, skipping UDP scout (path: ...)` 或 `unixpipe connect failed, falling back to UDP scout`

**验收**:
- 单测 `resolve_client_connect_endpoints_should_prefer_unixpipe_when_path_exists`(需要 spawn 一个临时 UnixListener 模拟 daemon)
- 集成测试:`rdog daemon -c rdog_macos.toml &` + `time rdog control mac.lab @ping`,日志里看到 unixpipe 成功路径,延迟 < 50ms

### Step 6: 测试与验证矩阵

**新增单测**(在 `src/zenoh_runtime.rs::tests` 和 `src/config.rs::tests`):
1. `unixpipe_socket_path_for_namespace_and_name_should_be_stable`
2. `unixpipe_socket_path_should_reject_oversized_combination`
3. `cleanup_stale_unixpipe_socket_should_remove_file_but_ignore_missing`
4. `compose_listen_endpoints_should_inject_unixpipe_when_enabled_and_not_present`
5. `compose_listen_endpoints_should_not_inject_when_disabled`
6. `compose_listen_endpoints_should_not_override_explicit_unixpipe`

**新增集成测试**(新建 `tests/zenoh_unixpipe_fast_path.rs`):
- `unixpipe_path_should_be_reachable_by_client_on_same_host`
  - spawn rdog daemon(临时 toml,只挂 unixpipe listen)
  - 用 rdog control 客户端连 `@ping`
  - 验证响应且验证客户端日志含 "unixpipe connect succeeded"
- `unixpipe_should_fall_back_to_udp_scout_when_socket_missing`
  - spawn rdog daemon 但不开 unixpipe(用 udp/127.0.0.1:7447 代替)
  - 客户端走 `@ping` 必须成功,日志含 "unixpipe connect failed, falling back to UDP scout"
- `stale_socket_should_be_cleaned_on_daemon_restart`
  - 手动 touch 一个 `/tmp/rdog-lab-mac.lab.sock`(模拟上次崩溃残留)
  - spawn rdog daemon 必须能起来,不报 "Address already in use"

### Step 7: 同步文档

**文件**:
- `rdog_macos.toml` / `rdog_linux.toml` 模板:在 `[zenoh]` 段加 `unixpipe = { enabled = true }` 注释行,解释"macOS / Linux 默认开启,同机会自动尝试 unixpipe,失败回退 UDP"
- `specs/zenoh-control-plane-plan.md`:在"Canonical control-plane behavior"之后增加一节"Local fast path: unixpipe",固定"本机 daemon + control 自动尝试 unixpipe;失败回退"的契约
- `AGENTS.md`:`specs/` 索引里加 `specs/zenoh-unixpipe-fast-path-plan.md` 条目
- `EXPERIENCE.md`:沉淀 2 条经验
  - "Zenoh 本机 fast path 优先用 unixpipe transport 而不是新增独立 UDS 控制面:保留 Zenoh 协议层语义,只是 link 层换 unix domain socket"
  - "同机 IPC 路径用 `$TMPDIR/rdog-{ns}-{name}.sock` 而不是 `/var/run/...`:macOS 的 $TMPDIR 已经 per-user,自然有权限隔离,不需要额外 chmod"

## 验收标准

### 功能性(必须 100%)

- [ ] `cargo build` 通过,无新增 warning。
- [ ] `cargo check` 在 macOS / Linux 上通过。
- [ ] `cargo test --lib` 所有新增单测通过。
- [ ] `cargo test --test zenoh_unixpipe_fast_path` 三个 e2e 集成测试通过。
- [ ] `cargo test --test zenoh_router_client`(已有测试)继续通过,没有回归。
- [ ] `rdog daemon -c ./rdog_macos.toml` 启动日志含 `unixpipe_endpoint: <path>`。
- [ ] `rdog control mac.lab @ping` 客户端日志含 `unixpipe connect succeeded`。
- [ ] `rdog control mac.lab @ping` 响应内容仍是 `pong`,行为不变。
- [ ] 远端(不同主机)`rdog control <remote-target>` 仍然走 UDP scout,行为不变。

### 性能(目标值,非硬性)

- 同机 `rdog control mac.lab @ping` p50 round-trip < 50ms(目标),p95 < 150ms(目标)。
- 对比基线(没改前):p50 round-trip 200~500ms → 改后 30~80ms(预期 2~5x 提速)。
- 验证方法:`for i in 1 2 3 4 5 6 7 8 9 10; do time rdog control mac.lab @ping >/dev/null; done`,记录 best/median/worst。

### 错误处理(必须 100%)

- [ ] socket 路径超过 100 字节时,daemon 启动 fail-fast,错误信息明确。
- [ ] stale socket 文件存在时,daemon 启动时 unlink 掉,不报 "Address already in use"。
- [ ] 同机 unixpipe 不可达,client 自动 fallback 到 UDP scout,行为完全透明。
- [ ] `--entry-point` 显式给 `unixpipe/<path>` 时,直接走 unixpipe,不再 fallback。

## 风险与缓解

| 风险 | 影响 | 缓解 |
|------|------|------|
| zenoh 1.8.0 `transport_unixpipe` 实际依赖系统库,编译失败 | 高 | Step 1 立刻跑 `cargo check` 验证,失败时降级为 `default-features = false, features = [...(去掉 transport_unixpipe)]` 并在 plan 里标记 "需要重选方向"。 |
| macOS `sun_path` 104 字节硬限制 | 中 | 路径推导函数硬校验 ≤ 100 字节,namespace+name 任一过长就报错让用户改短。 |
| stale socket 文件导致 bind 失败 | 中 | `cleanup_stale_unixpipe_socket` 启动前 unlink,集成测试覆盖。 |
| 同机多 daemon 同 `(namespace, daemon_name)` 冲突 | 低 | 已经有 `acquire_daemon_name_guard` PID 锁,沿用。unixpipe bind 会因为 `Address already in use` fail-fast,直接看错误就能定位冲突。 |
| 短 connect timeout 拉长高频调用感知 | 中 | timeout 默认 200ms,环境变量可配;高频 `@screenshot` 走 `@savefile` 多 frame 复用一条 session,不重复走 unixpipe connect。 |
| Windows 编译期报错 | 中 | `UnixpipeConfig::default` 在 Windows 强制 `enabled = false`,所有 unixpipe 相关代码用 `#[cfg(unix)]` 包裹,Windows 行为完全保持现状。 |
| 隐式路径选择让排错变难 | 低 | 启动日志 / 客户端 connect 前打 `unixpipe_path: <path>`,失败时打 `unixpipe connect failed: <err>`,排错信息完整。 |
| `/tmp` 跨用户共享,Linux 上其他用户可能看到 socket | 低 | 启用 `bind` 时设 mode 0600(`std::os::unix::fs::OpenOptionsExt::mode(0o600)` 不可,需要用 `std::os::unix::fs::PermissionsExt` 在 bind 之后 chmod)。macOS 走 $TMPDIR 不存在此问题。 |
| Zenoh `unixpipe/{path}` 实际 locator 语法与 Rust SDK 不一致 | 中 | 写 plan 时假设 `unixpipe/...` 前缀(已在 zenoh 1.x 文档里见过),实现时若 zenoh 1.8.0 实际用 `unixpipe/...` 或 `unixpipe@...` 略有差异,以 cargo run 起来的实际行为为准并修正。 |

## 验证步骤

### 单元测试

```bash
cargo test --lib --package rustdog -- zenoh_runtime::tests::unixpipe config::tests::unixpipe
```

### 集成测试

```bash
cargo test --test zenoh_unixpipe_fast_path
```

### 手动冒烟(macOS)

```bash
# terminal 1
RUST_LOG=info cargo run -- daemon -c ./rdog_macos.toml

# terminal 2
RUST_LOG=info cargo run -- control mac.lab @ping
# 预期看到 "unixpipe_endpoint: /var/folders/.../rdog-lab-mac.lab.sock" 和 "unixpipe connect succeeded, skipping UDP scout"

# 远端 fallback 验证
RUST_LOG=info cargo run -- control some-other-target @ping
# 预期 "unixpipe connect failed, falling back to UDP scout"
```

### 性能验证

```bash
# 同机 fast path 测时
for i in {1..20}; do /usr/bin/time -p rdog control mac.lab @ping > /dev/null 2>>/tmp/rdog-fast.log; done
grep real /tmp/rdog-fast.log | sort
# 期望 best < 0.05s, median < 0.10s
```

### 回归验证

```bash
cargo test --package rustdog --tests
```

## ADR

### Decision
在 `rdog control` 现有 Zenoh client 路径上,加 Zenoh `unixpipe` transport,让同机 daemon + control 自动走 Unix domain socket,失败透明 fallback 到 UDP scout / TCP。

### Drivers
1. 用户在 macOS 上感受到 `rdog control mac.lab` 单次 200~500ms 延迟,影响 agent 高频 `@web-find` / `@screenshot` 体验。
2. Zenoh 1.8.0 官方支持 `transport_unixpipe` feature,只是当前 rustdog 没启用。
3. unixpipe 不需要新引入 IPC 抽象,直接复用现有 Zenoh session / query / reply / liveliness 协议栈,风险最低。

### Alternatives considered
- **A2 - 直接 UDS 控制面(方向 B)**:代码量 200~300 行,跳过 Zenoh 完全,理论 10~50x。但要重写 daemon 端 control queryable 接收循环,要处理 stale socket 清理 + macOS 路径长度 + 权限,工作量大,且会引入"两套控制面" 的长期维护负担。
- **A3 - 共享内存 IPC**:完全 in-memory,但 Zenoh 协议层不复用,要自己实现 query/reply/序列化,工作量和方向 B 接近且更复杂。
- **A4 - 在现有 UDP 上加 tuning(scout 超时调小,query 加 retry)**:成本最低,但只是常数级改善,无法突破 UDP loopback 协议本身的开销,不能解决根问题。

### Why chosen
A 选 A1(方向 A)是因为它在"明显快"和"风险最低"之间达到最佳平衡:
- 2~5x 提速对当前用户体验(高频 control round-trip)已经是质变
- 不引入新抽象,后续如果要再升级到方向 B,UDS 路径可以叠加在 unixpipe 之上,而不是替换
- Zenoh 官方 feature,后续 zenoh 升级天然受益
- 本机 fast path 失败可以透明 fallback,完全不影响远端场景

### Consequences
- 正面:同机 `rdog control` round-trip 明显变快,agent 高频 GUI/Web 调用链更紧凑。
- 负面:`$TMPDIR` 行为对 Linux 用户不直观(虽然实际不冲突,只是首次见需解释),需要文档。
- 负面:zenoh 1.8.0 `transport_unixpipe` 是相对较新的 feature,长期稳定性需要后续版本验证。
- 后续:如果同机延迟仍不满足(高频 benchmark 暴露 unixpipe 也不够),下一步升级到方向 B(直接 UDS 控制面)。

### Follow-ups
- 升级到方向 B(直接 UDS 控制面)作为后续 plan。
- 沉淀 `EXPERIENCE.md` 关于"Zenoh 本机 fast path 优先用 unixpipe,而不是新增独立 UDS 控制面"的经验。
- 更新 `.codex/skills/rdog-control/SKILL.md` 的 troubleshooting 段,加入"同机 ping 慢? 确认 unixpipe 是否 enabled" 的诊断路径。
- 在 `rdog_macos.toml` / `rdog_linux.toml` 增加 `[zenoh.unixpipe]` 段示例,让用户知道这个能力存在并可控。

## 后续建议

- 完成实施后跑一轮 benchmark,验证确实达到 2~5x 提速。
- 如果用户后续对延迟仍有要求,启动方向 B(直接 UDS 控制面)的独立 plan。
- 把这次的 unixpipe 经验沉淀到 `EXPERIENCE.md`,作为后续类似决策的参考。
