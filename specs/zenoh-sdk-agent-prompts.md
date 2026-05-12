# 编程智能体实现提示模板: Zenoh SDK 对接 `rdog` daemon

## 1. 目的

这份文档提供两套可直接复制给编程智能体的实现提示模板:

1. Rust + Zenoh Rust SDK
2. Unity + `mhama/zenoh-unity-plugin`

目标不是泛泛说明思路,而是让智能体能够直接开始写代码。

---

## 2. 当前 `rdog` 对接合约摘要

在把提示模板交给智能体之前,先固定当前 `rdog` daemon 的外部合约:

### 2.1 当前模型

- `daemon = router`
- `control = client`
- 默认通过 autodiscovery 加入网络
- `--entry-point` 只是 fallback
- `service_name = daemon_name`
- `member_id = service_name` (当前 static 模式)

### 2.2 当前 keyexpr

#### alive
```text
rdog/<namespace>/daemon/<service_name>/member/<member_id>/alive
```

#### control
```text
rdog/<namespace>/daemon/<service_name>/member/<member_id>/control
```

#### keyinput
```text
rdog/<namespace>/daemon/<service_name>/member/<member_id>/keyinput
```

#### static 例子
```text
rdog/lab/daemon/mini-a.lab/member/mini-a.lab/alive
rdog/lab/daemon/mini-a.lab/member/mini-a.lab/control
rdog/lab/daemon/mini-a.lab/member/mini-a.lab/keyinput
```

### 2.3 当前支持的请求

- `@ping`
- `@cmd#id`
- bare shell lines
- `@key`
- `@paste`
- `@pty` / `@pty-close` / `@pty-detach` / `@pty-attach` over session channels
- 显式错误响应
- `@key` 成功后的键盘事件发布

### 2.4 当前明确不支持

- 不经 `@pty` 的传统 interactive shell over Zenoh
- 把裸 shell 行升级成带 cwd 状态保持的长期 shell

### 2.5 当前推荐的请求策略

- 会话启动时先 resolve 一次 target
- 后续请求默认复用当前 target
- 若 query timeout:
  - 再 resolve 一次
  - retry 一次

---

## 3. Rust 智能体实现提示模板

> 适用场景: 你要让编程智能体用 Rust + `zenoh = 1.8.x` 直接实现一个对接 `rdog` daemon 的 client。

### 3.1 可直接复制的 Prompt

```text
你现在要实现一个 Rust 程序,通过 Zenoh SDK 对接 `rdog` daemon 的当前 service/member 协议。

请严格遵守以下约束:

【协议与目标】
- 当前 `rdog` daemon 是 Zenoh router
- 当前 service_name = daemon_name
- 当前 member_id = service_name
- 当前 control keyexpr 规则:
  - alive: `rdog/<namespace>/daemon/<service_name>/member/<member_id>/alive`
  - control: `rdog/<namespace>/daemon/<service_name>/member/<member_id>/control`
  - keyinput: `rdog/<namespace>/daemon/<service_name>/member/<member_id>/keyinput`
- 当前 static 例子:
  - `rdog/lab/daemon/mini-a.lab/member/mini-a.lab/alive`
  - `rdog/lab/daemon/mini-a.lab/member/mini-a.lab/control`
  - `rdog/lab/daemon/mini-a.lab/member/mini-a.lab/keyinput`

【当前支持的请求】
- `@ping`
- `@cmd#id`
- bare shell lines
- `@key`
- `@paste`
- `@pty` / `@pty-close` / `@pty-detach` / `@pty-attach` over session channels
- 显式错误响应
- 如果 daemon 开启了 `key_input_events`,订阅方还可以监听 `keyinput` 主题接收 `@key` 成功事件

【当前明确不支持】
- 不经 `@pty` 的传统 interactive shell over Zenoh
- 把裸 shell 行升级成带 cwd 状态保持的长期 shell

【请求/响应格式】
- query payload 是 UTF-8 文本,内容是一整行控制文本,例如:
  - `@ping`
  - `@cmd#42:"printf READY"`
  - `@key:"F11"`
- reply payload 也是 UTF-8 文本,内容是一整行 `@response ...`
- 不要把 payload 当 JSON 对象
- 但 `keyinput` publish payload 是 UTF-8 JSON 文本,表示一次已经成功执行的键盘事件

【连接和重试策略】
- 以 client 模式加入 Zenoh 网络
- 默认先走 autodiscovery
- 如果环境里 autodiscovery 不稳定,允许通过已知 router entrypoint 加入
- 会话启动时先 resolve 一次 target
- 后续请求默认复用当前 target
- 如果 query timeout:
  - re-resolve target
  - retry 一次
- 如果 retry 仍失败,再正式返回错误

【你要实现的最小功能】
1. 以 client 模式打开 Zenoh session
2. 解析 namespace 和 service_name
3. 生成 alive/control keyexpr
4. 通过 liveliness 确认目标在线
5. 向 control key 发 query
6. 把 reply payload 作为整行 `@response ...` 文本返回
7. 实现 timeout -> re-resolve -> retry 一次

【代码要求】
- 用 Rust
- 优先使用 `zenoh = 1.8.x`
- 使用清晰的小函数拆分:
  - `build_alive_key()`
  - `build_control_key()`
  - `resolve_target()`
  - `execute_control_query()`
- 为关键函数写中文注释
- 至少给出一个 `@ping` 示例和一个 `@cmd#id` 示例
- 如有错误,返回清楚的 `Result<T, E>`

【输出要求】
- 给出完整可编译 Rust 代码
- 给出 Cargo.toml 依赖片段
- 给出如何运行的示例命令
- 不要把裸 shell 行扩成传统 interactive shell; 如需 PTY,必须按 `@pty` session channel 协议实现
```

### 3.2 给 Rust 智能体的补充提示

如果你想让智能体更稳,建议再补一句:

```text
请优先按当前 `rdog` 已实现的行为来写,不要自行设计新协议,不要把 `@response` 改成 JSON 对象协议。
```

---

## 4. Unity 智能体实现提示模板

> 适用场景: 你要让编程智能体在 Unity 里,基于 `https://github.com/mhama/zenoh-unity-plugin` 对接 `rdog` daemon。

### 4.1 先说明当前 Unity 插件事实

基于插件仓库当前结构,已确认:

- 有高层 wrapper:
  - `Session`
  - `Publisher`
  - `Subscriber`
  - `KeyExpr`
  - `Bytes`
  - `Encoding`
- `SimplePubSubTest.cs` 展示了:
  - `Session.Open(conf)`
  - `Publisher.Declare(...)`
  - `Publisher.Put(...)`
  - `Subscriber.CreateSubscriber(...)`
- 但当前 README 也明确说了:
  - wrapper 很有限
  - 很多功能仍然要直接操作 unsafe/native binding
- 从仓库代码可见:
  - `ZenohNative.g.cs` 已生成出 queryable / querier / liveliness 相关底层绑定
- 也就是说:
  - **做 pub/sub 很方便**
  - **做当前 `rdog` 这种 control query/reply,大概率要直接基于 `ZenohNative.g.cs` 封装一层 querier wrapper**

### 4.2 可直接复制的 Prompt

```text
你现在要在 Unity 中,基于 `mhama/zenoh-unity-plugin` 实现一个对接 `rdog` daemon 的 control client。

请严格遵守以下约束:

【当前插件事实】
- 插件仓库: https://github.com/mhama/zenoh-unity-plugin
- 当前高层 wrapper 主要覆盖:
  - `Session`
  - `Publisher`
  - `Subscriber`
  - `KeyExpr`
  - `Bytes`
  - `Encoding`
- 但 query/reply 对应的高层 wrapper 目前并不完整
- 如需实现当前 `rdog` 的 control query/reply,请直接基于 `ZenohNative.g.cs` 中已有的 querier / get / reply 相关绑定封装最小 wrapper

【当前 `rdog` 协议】
- service_name = daemon_name
- member_id = service_name
- alive key:
  - `rdog/<namespace>/daemon/<service_name>/member/<member_id>/alive`
- control key:
  - `rdog/<namespace>/daemon/<service_name>/member/<member_id>/control`
- query payload 是整行 UTF-8 文本:
  - `@ping`
  - `@cmd#42:"printf READY"`
  - `printf READY`
  - `@key:"F11"`
- reply payload 是整行 UTF-8 文本 `@response ...`
- 当前不支持:
  - 不经 `@pty` 的传统 interactive shell over Zenoh
  - 把裸 shell 行升级成带 cwd 状态保持的长期 shell

【你要实现的最小功能】
1. 在 Unity 中打开 Zenoh session
2. 生成 alive/control keyexpr
3. 检查目标 service_name 是否 alive
4. 对 control key 发 query
5. 收到 reply 后,按 UTF-8 文本返回
6. timeout 时:
   - re-resolve target
   - retry 一次

【Unity 代码要求】
- 优先复用已有 wrapper:
  - `Session`
  - `KeyExpr`
  - `Bytes`
- 如果高层 wrapper 不够,新增一个最小 `Querier` wrapper,但只包当前需要的最少 native 调用
- 不要大面积包装整个 Zenoh C API
- 代码注释用中文
- 给出一个 Unity MonoBehaviour 示例,例如 `RdogZenohControlClient.cs`
- 给出一个最小 UI 或调试入口:
  - 输入 service_name
  - 点击按钮发送 `@ping`
  - 显示 reply 文本
- 再给一个发送 `@cmd#id:"printf READY"` 的例子

【输出要求】
- 先列出你要新建/修改哪些 Unity C# 文件
- 再给出关键 wrapper 代码
- 再给出 `RdogZenohControlClient.cs` 示例
- 明确说明哪些地方用了低层 native binding
- 不要把裸 shell 行扩成传统 interactive shell; 如需 PTY,必须按 `@pty` session channel 协议实现; 不要扩到 HA routing
```

### 4.3 给 Unity 智能体的补充提示

建议再加一句:

```text
优先参考仓库里的 `SimplePubSubTest.cs`、`Session.cs`、`Publisher.cs`、`Subscriber.cs` 的写法风格,只为 querier/get 增加最小必要封装,不要把整个 native API 全部重写成 wrapper。
```

---

## 5. 给智能体的共通补充约束

无论 Rust 还是 Unity,你都可以额外补上这段公共约束:

```text
请严格按当前 `rdog` 已实现的协议对接,不要自行发明第二套协议。
请把 timeout -> re-resolve -> retry 一次 实现为默认行为。
请把当前未开放能力视为错误,不要偷偷支持:
- 不经 `@pty` 的传统 interactive shell over Zenoh
- 把裸 shell 行升级成带 cwd 状态保持的长期 shell
```

---

## 6. 当前推荐使用顺序

### 如果目标是最快落地

1. 先用 Rust 模板实现一个最小 client
2. 再用 Unity 模板做 UI 包装

### 如果目标是先做 Unity

1. 先在 Unity 里只做 `@ping`
2. 再做 `@cmd#id`
3. 再考虑 `@key`

原因很简单:

- Unity 插件当前 query/reply wrapper 不完整
- 先从最小文本往返做起,最稳
