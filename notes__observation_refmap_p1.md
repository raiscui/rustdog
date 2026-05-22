## [2026-05-19 23:50:49] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] 笔记: P1 durable observation state brownfield 事实

## 来源

### 来源1: `specs/rdog-observation-scoped-refmap-plan.md`

- P1 的目标是 durable observation state,重点包括:
  - daemon 重启后还保留 selector 线索。
  - observation metadata 可审计。
  - 可以复用最近一次 refmap cache。
- roadmap 明确 P2 才是 permanent selector schema,P3 才是 semantic re-find。
- 这意味着 P1 可以定义 selector envelope / selector draft,但不能把自动语义重找做成默认行为。

### 来源2: `src/control_observation.rs`

- 当前 P0 store 是 `OnceLock<Mutex<ObservationStore>>`,完全进程内。
- 当前 `ObservationHeader` 已有 `selector_count`,但 P0 固定写 `0`。
- 当前 ref entry 只保存:
  - `ref_id`
  - `backend_id`
  - `kind`
- 当前错误已经区分:
  - `OBSERVATION_EXPIRED`
  - `STALE_REF`
- 当前注释明确说 daemon 重启后旧 `observation_id` 必须失效。P1 不能直接让旧 `@eN` 跨重启复活。

### 来源3: `src/control_ax.rs` / `src/control_ax/query.rs`

- `AxSnapshot::with_observation()` 会给窗口和元素分配短期 `@eN`,并通过 `record_observation()` 写入 store。
- `AxWindow` / `AxElement` 已有 wire 字段 `ref`。
- `AxTarget` 支持 `target:{ref,observation_id}`,但明确禁止和 semantic locator 混用。
- `@ax-get` 可以从 `target:{ref,observation_id}` 解析到 backend id,然后抓取更深 snapshot。
- P1 如果新增 selector 输入,应避免直接塞进现有 `AxTarget` 的 `ref` 分支,否则会破坏 P0 的短期 ref 单一语义。

### 来源4: `src/control_window.rs` / `src/control_window/macos.rs`

- `WindowFindResponse` 已有 `observation` header。
- `WindowCandidate` 已有 `ref`。
- `@window-activate` / `@window-close` 可以消费 `target:{ref,observation_id}`。
- window 侧的 selector 候选字段天然包括:
  - app name
  - bundle_id
  - pid
  - title / title_contains
  - window state / rect / current_space
- P1 的 window selector 应优先从这些字段生成,但 pid 只能作为弱线索,不能当 durable 主键。

### 来源5: `src/config.rs`

- 当前 daemon config 由 figment 加载 TOML / env。
- `DaemonConfig` 现在包含 `daemon`、`hidden`、`outbound`、`inbound`、`zenoh`,没有 observation state 配置。
- 项目约定是配置优先走 TOML / figment,不应为 P1 新增开发环境变量。
- 因此 P1 需要新增 `[observation]` 或 `[state.observation]` 配置块,并更新 config template / validation / tests。

### 来源6: `ControlPeerSession`

- `ControlPeerSession` 当前负责 frame queue、dispatch、request id 和 PTY lifecycle gate。
- 它不拥有 screenshot backend、AX backend 或 UI state。
- P1 的 durable store 应属于 daemon runtime / control observation 层,不能塞到 TCP/WebSocket/Zenoh transport 私有实现。

## 综合发现

### P1 必须钉住的边界

- 旧 `@eN` 仍然是 observation-scoped ephemeral ref。
- daemon 重启后,旧 `ref + observation_id` 不能被当成有效 action target。
- P1 保存的是:
  - observation metadata
  - selector draft / selector envelope
  - 最近一次 backend id / path cache
  - 审计信息和淘汰信息
- P1 不做:
  - 自动 semantic re-find
  - confidence ranking
  - `@observe`
  - mouse ref 化

### 推荐 P1 方向

- 新增 `control_observation::durable` 或 `control_observation/state.rs`,把内存 store 和 durable snapshot writer 分开。
- 默认 durable backend 使用 JSONL + compact index,不要第一步引入 sqlite。
- 新增 `[observation]` 配置块:
  - `durable_enabled`
  - `state_dir`
  - `retention_observations`
  - `retention_bytes`
  - `persist_values`
  - `persist_screenshots`
- 默认不持久化 AXValue 原文和截图图像,只保存可审计 metadata / selector 线索。
- 后续 P2/P3 可以把 selector index 升级为更强查询层,但 P1 只负责把数据写对、读回、解释清楚。

## [2026-05-20 01:09:12] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] 笔记: P1 completion audit

## 来源

### 来源1: 代码实现

- `src/control_observation.rs`
  - `record_observation_with_selectors` 先由内存 store 生成 `observation_id`,再把 selector draft 转成 durable record。
  - expired/stale ref 只通过 durable store 返回 hint,不会改变内存 ref resolver。
- `src/control_observation/durable.rs`
  - JSONL backend 写 observation、selector、hint-only ref cache 和 index。
  - index replay 只在缺失/损坏时触发,权限或其他 IO 错误不静默降级。
  - durable hint 会根据 selector draft 生成 `@window-find` / `@ax-find` / `@screenshot include_ax` 恢复命令。
- `src/control_ax.rs` / `src/control_window.rs`
  - AX window、AX element、window candidate 都生成 selector drafts。
  - `selector_count` 不再固定为 0。
- `src/daemon.rs` / `src/zenoh_control.rs`
  - TCP daemon 和 Zenoh router daemon 都初始化 daemon-owned durable observation state。

### 来源2: 验证命令

- `cargo fmt -- --check`: passed。
- `cargo test --package rustdog --bin rdog control_observation::tests`: 5 passed。
- `cargo test --package rustdog --bin rdog control_observation::durable::tests`: 3 passed。
- `cargo test --package rustdog --bin rdog control_ax::tests`: 14 passed。
- `cargo test --package rustdog --bin rdog control_ax::query::tests`: 5 passed。
- `cargo test --package rustdog --bin rdog control_window::tests`: 6 passed。
- `cargo test --package rustdog --bin rdog screenshot::tests`: 17 passed。
- `cargo test --package rustdog --bin rdog config::tests`: 26 passed。
- `cargo test --package rustdog --test zenoh_router_client -- --test-threads=1`: 23 passed,2 ignored。
- `git diff --check`: passed。

## 综合发现

- P1 的核心契约已经被测试锁住: durable state 只能提供恢复提示,不能让 fresh daemon store 直接解析旧 `@eN`。
- 当前 durable backend 是 JSONL + index 的 P1 形态。JSONL compact、fsync 强化和 SQLite 迁移仍属于 P1b/P2 后续工作。
- 外部 architect gate 环境不可用: Claude role 缺失,Claude provider 余额不足,Gemini 超时未产出 artifact。本轮不能把外部审查说成已通过,只能把它记录为环境限制。
