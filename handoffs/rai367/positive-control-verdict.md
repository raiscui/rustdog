# 正对照判读（RAI-367 §4.4）

> 判据：RUST-14。执行：RUST-5（RAI-366 真机拓扑：云机 × rais-razer）。
> 被测二进制：分支 `capy/rai-367-bulk-gate` head `93089d842`；
> 云机客户端 sha256 `5be64ec1ac313915651b34a782c8a0a8e9318cf3df17a6f0086a3b9d50eb5598`（debug）、
> razer daemon sha256 `3658f78f1650f7c7b9100b95c8b619272e4137044490dc9db61d9fda4b43657a`（release, `--no-default-features`）。

## 结论（一句话）

**门禁成立，仪表未成立。** 真机上 bulk 动词被**动词级**拒于能力码 `iroh_bulk_path_pin_unavailable`，且
该拓扑上数据跳确实可得直连；但**正对照没拿到计数**，所以"bulk 在 relay 上的字节为 0"这一条**仍不做结论**。

## 逐条判读（对着单子的判据表）

| 项 | 原始证据 | 判定 |
| --- | --- | --- |
| 前置：daemon 起得来且目录注册成功 | `11:08:46.679Z … home is now relay https://use1-1.relay.n0.iroh.link./` + `iroh_directory_path=relay` + `iroh daemon registered pty=ping-only`，均早于 11:09:15Z 的客户端读数 | **成立** |
| S2 直连可得 | `s2a.err`: `iroh_path=direct`；`s2b.err`: `iroh_path=direct`；两次调用各自的 `date -u` 是 `11:09:15Z` / `11:09:17Z`，即**两个独立进程各一次**，不是一次 ping 重复打印 | **成立（2/2）** |
| S2 的目录跳与数据跳无关 | 第 2 次同一次调用里 `iroh_directory_path=relay` 与 `iroh_path=direct` 并存 | 成立，符合"目录一跳、数据一跳"的预期 |
| S3 门禁拒绝 | 三个动词各回 `error: iroh_bulk_path_pin_unavailable`、`exit=1`、**没有一条返回 `@response`**（`@file-list` 也没有空列表成功） | **成立** |
| S3 是**动词级**拒绝而非参数错 | 三条动词（含无载荷的 `@file-list`）拿到**同一个**能力码；代码上 `ping_only` 按命令变体判定，早于任何载荷解析（`src/control_iroh.rs` 的 `is_bulk_command`） | **成立** |
| S3 无字节流动 | `accepting`/`reading` 计数 0；daemon 日志 `grep -i pc.bin` 计数 0；任务目录 `.rdog-partial` 计数 0 | 成立（三条独立证据一致） |
| S1 正对照计数 | 未取得（客户端无 relay 句柄，`relay_sample_available=false`；按 RAI-476 待办） | **未成立** |

## 按单子的判读规则得出的结论

单子里写死的那条规则是：**拿不到正对照计数时，S3 的"零"不得用于任何结论。** 因此：

- **可以下的结论**：「bulk 不经过 relay」这条不变量今天由**能力拒绝**保证 —— 真机上 bulk 动词在
  动词级被 `iroh_bulk_path_pin_unavailable` 拒绝，一个字节都没发出去。这不是"默认只允许直连"，
  而是"这条通道根本没有 bulk"。
- **不能下的结论**：「bulk 在直连上完成且 relay 字节为 0」。它需要三件今天没有的东西 ——
  iroh 适配器的 bulk 数据面、`bulk_path_pin=true`、daemon 侧 relay 采样（RAI-476）。

## 记两条方法

1. **"打不通"与"跑不了"在同一份回执里长得一样。** 上一轮失败的真因是二进制来自 #157 之前的
   tree（daemon 起不来、目录没注册），失败串却像网络问题。这条前置检查现在是单子的第一项。
2. **判据不能只看 exit code。** 三个动词都 `exit=1` 还不够 —— 要看**有没有 `@response`**、
   被拒的是**动词**还是**参数**。前者是能力拒绝（我们要的），后者是"参数写错所以拒"，不是证据。
