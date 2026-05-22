## [2026-05-21 08:48:19] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] 事项: P5 前拆分 observe 子模块

### 背景

- Architect verifier 已批准 P4,但提醒 `src/control_observation/observe.rs` 已接近项目 1000 行健康线。
- 当前文件承载 parser、producer、renderer、ref sample collector 和 tests 外的所有 observe 逻辑。

### 后续建议

- 在 P5 mouse ref 化前,优先把 `observe.rs` 拆成更清晰的子模块。
- 推荐拆分方向:
  - `request.rs`: `ObserveMode` / `ObserveTarget` / `ObserveRequest` / parser。
  - `producer.rs`: visual / AX / window section production。
  - `response.rs`: bundle response render、status、primary observation source。
  - `refs.rs`: ref sample 和 selector summary。

### 触发时机

- 开始 P5 之前。
- 或者 `observe.rs` 继续增长到 950 行以上时。

## [2026-05-21 17:37:51] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] 事项完成: P5 前拆分 observe 子模块

### 完成状态

- 已完成 `src/control_observation/observe.rs` 的 request / producer / response / refs 分层。
- 原 879 行单文件已拆成:
  - 根 `observe.rs`: 46 行。
  - `observe/request.rs`: 379 行。
  - `observe/producer.rs`: 199 行。
  - `observe/response.rs`: 167 行。
  - `observe/refs.rs`: 134 行。

### 后续口径

- 此 LATER 项已落地。
- 后续 P5 只需要在拆分后的层次上继续推进,不应再把新 mouse/ref 逻辑写回根 `observe.rs`。
