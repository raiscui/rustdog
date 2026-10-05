# Daemon 模式运行时规格

> 状态：**现行规格**（自 openspec 归档迁入，2026-10-06）
> 来源：openspec change `add-config-driven-daemon-mode` 归档产物，四份合成

## 目标

定义 `rdog daemon`（配置驱动的常驻模式）的运行时行为契约：

1. 有效配置如何分层合成
2. 启动期如何 fail-fast 校验
3. 入站监听器的绑定与持续接受
4. 出站连接器的首连与重试
5. 守护进程如何持续监管各 endpoint worker

这份文档是 daemon 模式的单一规格入口。修改 `src/daemon.rs`、`src/config.rs`
或相关 worker 时应先回读这里。

---

## 术语

- **daemon 模式**：`rdog daemon` 启动的常驻进程，由配置驱动 endpoint worker
- **有效配置**：内建默认、可选 TOML 文件、`RDOG_` 前缀环境变量三层合成后的结果
- **endpoint role**：入站（inbound）或出站（outbound）这一类 worker
- **监管循环（supervision loop）**：启动并持续保有各 role worker 的运行时结构

---

## 1. 运行时配置分层

有效配置由三层合成，内建默认 → TOML → `RDOG_` 环境变量，后者覆盖前者。

旧命名（`rcat*.toml` / `RCAT_` 环境变量）**仅作为升级回退**接受。
同一字段出现在多个来源时，新的 `RDOG_` 变量 SHALL 覆盖 `RCAT_` 变量、
TOML 值与内建默认。

### 需求：配置 SHALL 从分层提供方加载

#### 场景：无配置文件时使用内建默认

- **WHEN** 运行 `rdog daemon` 未带 `--config` 且无匹配的 `RDOG_` 环境变量
- **THEN** 重试策略与 endpoint 开关使用内建默认

#### 场景：环境变量覆盖 TOML

- **WHEN** TOML 设 `outbound.port = 4444` 且存在 `RDOG_OUTBOUND__PORT=5555`
- **THEN** 有效出站端口为 `5555`

---

## 2. 启动期 fail-fast

daemon SHALL 在**任何 worker 线程启动之前**校验有效配置。启用的 endpoint
缺必填字段、或显式指定的配置文件无法加载时，启动 MUST 以配置错误失败，
而不是进入监管循环后才暴露问题。

### 需求：无效启动输入 SHALL fail fast

#### 场景：启用的出站 endpoint 缺必填 host

- **WHEN** `outbound.enabled = true` 且默认/TOML/环境变量都未提供出站 host
- **THEN** daemon 命令在任何 worker 启动前退出并报配置校验错误

#### 场景：显式配置路径不可读

- **WHEN** 运行方传入 `--config /path/to/rdog.toml` 且该文件不可读或格式错误
- **THEN** daemon 命令以配置加载错误退出

---

## 3. 入站监听器

入站监听器 SHALL 绑定配置的本地地址，并在进程存活期间持续接受入站会话。

单个会话处理期间的失败 MUST 只终止该会话；监听角色 SHALL 保持可用。
若监听 socket 本身无法立即重建，worker SHALL 按配置的重试间隔重试，
而不是终止进程。

### 需求：入站监听器 SHALL 绑定并持续接受会话

#### 场景：启动时绑定

- **WHEN** daemon 以 `inbound.enabled = true` 启动
- **THEN** 入站 worker 在启动监管期间尝试绑定配置的本地主机与端口

#### 场景：前一个会话结束后接受下一个

- **WHEN** 一个入站会话成功完成，或其挂载的本地命令退出
- **THEN** 入站 worker 回到 accept 循环
- **THEN** 下一个入站连接可在不重启 daemon 的情况下被接受

### 需求：单会话失败 SHALL 与监听器生命周期隔离

#### 场景：会话 I/O 失败

- **WHEN** 已接受的入站会话在桥接 socket 与本地命令时发生 I/O 错误
- **THEN** 当前会话被关闭，进程保持存活
- **THEN** 入站 worker 继续服务后续连接

#### 场景：端口暂时不可用导致初始绑定失败

- **WHEN** 入站 worker 在启动期或监听器重启后无法绑定配置的本地地址
- **THEN** worker 记录绑定失败，进程保持存活
- **THEN** worker 按配置的重试间隔等待后再次尝试绑定

---

## 4. 出站连接器

出站连接器启用时，daemon 启动 SHALL **立即**尝试建立配置的出站会话，
不等待第一个重试间隔。

初次连接失败、以及已建立的出站会话结束之后，出站 worker MUST 持续重试。
每次重试 SHALL 等待配置的重试间隔，并 SHALL 持续使用同一配置目标，
直到配置变化或进程停止。

### 需求：出站连接器 SHALL 在启动时立即尝试会话

#### 场景：首次出站尝试立即发生

- **WHEN** daemon 以 `outbound.enabled = true` 启动
- **THEN** 出站 worker 在启动后立即执行第一次连接尝试

### 需求：失败或会话关闭后 SHALL 重试

#### 场景：首次连接失败

- **WHEN** 第一次出站连接尝试返回网络错误
- **THEN** worker 记录失败并等待配置的重试间隔
- **THEN** worker 执行另一次连接尝试，而不是退出

#### 场景：已建立的出站会话结束

- **WHEN** 出站会话成功建立后关闭，或其子进程退出
- **THEN** worker 将该会话结束视为非致命
- **THEN** worker 等待配置的重试间隔后发起新的出站连接尝试

---

## 5. 守护进程监管

daemon 模式启动成功后，进程 SHALL 为每个启用的 endpoint role 启动一个
worker，并在这些 worker 被监管期间保持存活。

可恢复的失败 MUST NOT 终止守护进程：连接失败、监听绑定失败、会话 EOF、
单会话 I/O 失败都属于可恢复的 worker 事件。进程 SHALL 记录失败、等待配置的
重试间隔、并在不终止整个进程的前提下继续监管对应 worker。

### 需求：守护运行时 SHALL 持续监管已启用的 endpoint worker

#### 场景：两个 endpoint role 同时启动

- **WHEN** 有效配置中入站与出站均启用
- **THEN** daemon 在同一次启动序列中启动两个 worker
- **THEN** 进程在启动 worker 后保持运行，而不是启动后立即退出

### 需求：可恢复的 endpoint 失败 SHALL NOT 终止 daemon 进程

#### 场景：启动时出站连接失败

- **WHEN** 启动期出站 worker 无法连接配置的远端
- **THEN** daemon 进程保持存活
- **THEN** 出站 worker 在首次重试间隔前保持等待

#### 场景：入站 worker 丢失其 socket

- **WHEN** 入站 worker 在 daemon 启动后遇到 bind 或 accept 失败
- **THEN** daemon 进程保持存活
- **THEN** 入站 worker 在配置的重试间隔后重新进入重试循环

---

## 相关

- [Line Control Protocol 规格](control-line-protocol.md) —— 控制连接的线协议
- `rdog_win.toml` / `rdog_macos.toml` —— 各平台 daemon 配置样例
