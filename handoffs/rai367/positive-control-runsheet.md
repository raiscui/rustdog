# 正对照跑批单（RAI-367 §4.4）

> 判据与判读：RUST-14。执行：RUST-5（RAI-366 的真机拓扑与 profile 在它那边）。
> 回执放 `handoffs/rai367/receipts/<YYYYMMDD>-<step>/`，每次执行一个目录。

## 0. 先说清楚这份单子能证明什么、不能证明什么

写这份单子时我核了当前代码，两条事实必须先摆出来，否则执行出来的"零"会被误读：

1. **iroh lane 今天不承载 bulk。** 它的准入面只有 `@ping` / `@pty`，bulk 动词一律以
   `iroh_bulk_path_pin_unavailable` 拒绝（`src/control_iroh.rs` 的 `ping_only`）。所以在那条通道上
   **不存在**一次"成功的 bulk 尝试"，也就不存在 A 类（直连可用且 bulk 完成）的读数。
2. **daemon 侧还没有 relay 计数。** `rdog.bulk-path.v1` 快照目前只有**客户端那一半**
   （`rdog control --bulk-path-json`）。两端 join 需要 daemon 侧也能采样，那要等 iroh 适配器承载
   bulk 数据面之后。

因此这份单子执行的是**仪表验证 + 门禁验证**，不是 A 类验收：

| 步骤 | 能证明 | 不能证明 |
| --- | --- | --- |
| S1 经 relay 的 `@ping` delta 非零 | 计数器接上了、relay 侧字节可观测 | bulk 行为 |
| S2 同拓扑 `@ping` 直连路径 | 这对真机上直连确实可得（A 类的前置） | bulk 能走直连 |
| S3 bulk 动词被拒且零字节 | 门禁真的在拒，不变量由能力保证 | bulk 走直连能完成 |

**A 类（bulk 在直连上完成）的剩余缺口是 iroh 适配器的 bulk 数据面**，不在本单子里；它是另一张票
（下面的 §5 记了它需要的接口）。

## 1. 前置（执行前打勾，缺一项就停）

- [ ] `rdog` 二进制 sha256 记进回执（`shasum -a 256 <binary>`）。
- [ ] razer 上没有上一次执行遗留的 rdog 进程：`pgrep -af rdog` 输出进回执；**若只看到业主自己的
      `lab` daemon，不要动它**，并把它的 PID 写进回执说明"存在但不是本次的"。
- [ ] 两份 profile 路径与内容摘要（key / CA 不进回执，只写路径与 sha256 的**前 8 位**）。
- [ ] 时间戳：`date -u +%FT%TZ` 记在每份回执首行。
- [ ] 收尾约定：只按 PID 停自己起的进程并回读 `pgrep -af rdog`，**禁止按映像名杀**。

## 2. S1 正对照：经 relay 的控制面交互，delta 必须严格 > 0

目的：证明"读数为零"不是"计数器没接上"。这一步**必须**在读完 S2/S3 的零之前或同时做。

```bash
# 1) 让 daemon 走 relay（用 RAI-366 已验证的 profile；daemon 侧日志里应有 relay home 建立）
<binary> daemon --transport iroh --iroh-config <daemon-profile.toml> > s1-daemon.log 2>&1 &
echo $! > s1-daemon.pid

# 2) 客户端强制走 relay 拨号，发一条普通控制面请求
<binary> control --transport iroh --iroh-config <client-relay-profile.toml> \
  <NAMESPACE/NAME> '@ping' > s1-ping.txt 2> s1-ping.err
```

抓哪些字段（逐行原文抄进回执，不要转述）：

- `s1-ping.txt` 的 `@response` 全文；
- 客户端 stderr 里的 `iroh_path=relay` 行（若为 `direct`，本步无效 —— 说明它没走 relay，重排 profile 再来）；
- daemon 日志里的 `iroh_directory_path=` 与 relay home 建立行；
- **客户端 relay 计数**：若当前二进制还没有 `--bulk-path-json` 的 relay 采样（见 §0 第 2 条），
  本步的"计数非零"由 iroh 端点自报的 relay 统计提供 —— **拿不到就如实写"本步未取得计数"**，
  并**直接判定：正对照未成立，S3 的"零"不得用于任何结论**。

## 3. S2 直连可用：这对真机上直连确实可得

```bash
<binary> control --transport iroh --iroh-config <client-direct-profile.toml> \
  <NAMESPACE/NAME> '@ping' > s2-ping.txt 2> s2-ping.err
```

判据：`s2-ping.err` 或 `s2-ping.txt` 里出现 `iroh_path=direct`（原文）。**两次**如此才算成立
（RAI-366 的读数是两次）。

## 4. S3 门禁：bulk 动词被拒，且不产生任何字节

```bash
# 三个动词各发一次；每一个都必须拿到同一个稳定码
for verb in 'put-file:{"transfer_id":"pc","path":"/tmp/pc.bin","size":1,"resume":false}' \
            'get-file:{"transfer_id":"pc","path":"/tmp/pc.bin"}' \
            'file-list'; do
  <binary> control --transport iroh --iroh-config <client-direct-profile.toml> \
    <NAMESPACE/NAME> "@$verb" >> s3-bulk.txt 2>&1
done
```

判据（三条全中才算成立）：

1. 每一条都报 `iroh_bulk_path_pin_unavailable`；
2. `/tmp/pc.bin` 与 `/tmp/.rdog-partial/` **都不存在**（`ls -la /tmp/pc.bin /tmp/.rdog-partial` 进回执，
   预期两次 `No such file`）；
3. `s3-bulk.txt` 里没有任何 `state:"accepting"` / `state:"reading"` 之类表示字节流动过的回执。

## 5. 判读规则（我按这个读，执行方不需要判断）

| 观察 | 结论 |
| --- | --- |
| S1 拿到非零 delta，S3 三条全中 | 门禁与仪表都成立：**"relay 上 bulk 字节为 0"今天由能力拒绝保证**，且这个"零"不是空真 |
| S1 拿不到计数 | **正对照未成立**。S3 的零**不得**用于结论，只记"门禁拒绝了 bulk 动词"这一半 |
| S3 里有任何动词返回了非 `iroh_bulk_path_pin_unavailable` | 门禁回归：按代码缺陷处理，不进结论 |
| S2 没出现 `iroh_path=direct` | A 类前置不成立；与 bulk 无关，但要写进回执 |

## 6. A 类还缺什么（不在本单子范围，留给下一张票）

要在真机上得到"bulk 在直连上完成且 relay 字节为 0"，还缺三件：

1. iroh 适配器的 **bulk 数据面**（客户端与 daemon 都能在 lane 上收发分块流）；
2. **路径钉住**（`bulk_path_pin=true`）：禁止迁移，排队与重传的字节不得改走 relay；
3. **daemon 侧 relay 采样** + 两端快照 join 的验收脚本（`relay_bytes_zero_own_half()` 两端都为真）。

这三件齐了，本单子再加一个 S4「bulk over direct 完成 + 两端快照 join」即可升级为 A 类验收。
