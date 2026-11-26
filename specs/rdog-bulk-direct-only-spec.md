# bulk 传输原语与直连强制规格 (RAI-367)

> 状态: **设计已定, 实现未开始**
> 父票: RAI-302 P7 第 3 项「bulk 只允许直连」
> 前置: RAI-366 (直连可用的拓扑与判据), RAI-413 (iroh lane 落到 main)
> 本文只写协议与验收口径, 不含实现. 实现落地后须把本文里"待回填"的指针补成 `src/` 定义位置.

---

## 0. 这份规格要解决的问题

P7 第 3 项今天**既不可验证也不可证伪**: 候选没有任何 bulk 传输入口, 所以"relay 上 bulk 字节为 0"
这句话没有实测对象. 本规格落一个 bulk 原语, 并把"直连不可得时显式失败"写成可判定的条款.

三条不变量按优先级排列, 后面所有条款都服务于它们:

1. **bulk 字节永远不会经过 relay.** 直连不可得就失败, 不存在"先用 relay 顶上"的中间态.
2. **路径判定不可读时按失败处理.** 读不到路径类别 ≠ 直连.
3. **失败与成功的判定必须能在同一台机器、同一次运行里被证伪.** 计数器坏了要能被正对照抓出来.

---

## 1. 现状判定: 复用 `control_file_transfer`, 扩展网关, 不另起

结论: **数据面复用 `src/control_file_transfer.rs`, 网关扩展, 明确不复用 `SaveFileFrame`.**

### 1.1 为什么不复用 `SaveFileFrame`

`SaveFileFrame` (`src/control_frames.rs:45`) 的字段是 `filename/mime/encoding/data/quality/width/height`:
整文件塞一个 base64 字段、单向 (被控端 → 控制端)、带截图语义 (`quality/width/height`).
`specs/file-transfer-control-spec.md` 已在 2026-10-06 用四条硬约束否掉它 (单向、不分块、字段无上限、
无校验无续传) 并定下"另建传输 lane". 本规格不再重新论证, 只沿用该决策.

要补一句 10-06 没写到的地方: **给 `SaveFile` 加分块不算"扩展", 算改写.**
分块要求 offset/sha256/resume/多帧生命周期, 这些一旦加进 `SaveFileFrame`, 它就同时承担
"给你看这张图"和"搬一个 178 MB 的东西"两种生命周期, 而这两者的失败模式不同 ——
截图失败重拍即可, 传输失败要保住已收字节. 合并会让截图回传帧长出传输会话状态机.

### 1.2 为什么复用 `control_file_transfer`

`src/control_file_transfer.rs` (597 行) 今天已经提供 bulk 需要的几乎全部语义:

| 能力 | 落点 |
| --- | --- |
| 分块上传 (`@put-file` → `@file-chunk*` → `@file-end`) | `execute_put_file` / `execute_file_chunk` / `execute_file_end` |
| 分块下载 (拉取式 `@get-file` → `@file-read*`) | `execute_get_file` / `execute_file_read` |
| chunk 有界 (上限从帧上限推导, 不硬编码) | `CHUNK_BASE64_LIMIT` (推导式在 `src/control_file_transfer.rs:44`, 单一真相源 `src/control_line_read.rs:30`) |
| 摘要校验 + 不符即清理半成品 | `execute_file_end` |
| 续传 (partial 保留 + `resume:true`) | `execute_put_file` |
| 顺序写 (offset 必须等于已写长度) | `execute_file_chunk` |
| 路径穿越拒绝 | `validate_transfer_path` |
| 协议接线 | 解析 `src/control_protocol.rs:692-696`, 派发 `src/control_core.rs:274-286` |
| 与默认 executor 隔离 | `src/control_actions.rs:327` 明确声明 file 动词不走默认分支 |

这些全在 main 上, 有单测覆盖. 重建一套没有理由.

### 1.3 缺的是什么 (两个缺口, 都不是协议)

**G1 客户端驱动缺位.** 协议在 daemon 侧是完整的, 但没有任何 CLI 能驱动它. 今天的用法只能是
`rdog control <target> '@file-chunk:{...}'`, 而:

- chunk 载荷塞进命令行参数会撞 `E2BIG`;
- `send_control_lines_for_invocation` (`src/control_invocation.rs:320`) 收的是预构造的 `&[String]`,
  即"整文件先进内存再发" —— 178 MB 文件 base64 后约 238 MB 全部驻留, 直接与分块的初衷相反.

所以 G1 是"缺一个流式驱动", 不是"缺一个协议字段".

**G2 路径门禁缺位.** daemon 与 client 都不知道传输的路径类别, 于是 bulk 可以静默骑在 relay 上.
这是本票的核心. G2 的落点不在 `control_file_transfer` 内部, 而在**发起方向的那一端**.

---

## 2. bulk 原语的协议面

### 2.1 传输能力声明

路径类别是**传输的属性**, 不是控制协议的属性. 现有四个传输适配器里只有一个有路径概念:

**四格的定义与读者 (唯一登记处是 `src/bulk_path.rs` 的模块文档, 这里只引用)**: `path_aware` 是
**传输适配器**的拓扑属性 (这条链路上有没有 relay 跳); `bulk_path_pin` 是**传输层**能否把 bulk 连接
钉在直连上 (禁止迁移、排队与重传不得改走 relay); `bulk_path_checked_per_chunk` 是**应用层**会不会在
open 与每个 chunk 边界采样、读到非 `direct` 即拒; `path_class` 是**本次尝试此刻**观测到的类别, 由
传输入口在 open 与每个 chunk 边界读。

- `path_aware=false` ⇒ 放行并记 `not_applicable`;
- `path_aware=true` 且 **`bulk_path_pin=false` 且 `bulk_path_checked_per_chunk=false`** ⇒ **拒绝**
  (既不钉也不查, 采样到 direct 也不代表之后的字节);
- `path_aware=true` 且 `bulk_path_checked_per_chunk=true` ⇒ 按 `path_class` 判定 (见 §3.1), 但
  **不得**因此主张"连接被钉在直连上" (§3.5);
- 读到 `unknown` ⇒ **拒绝** (fail-closed, 永不强转 `direct`)。

| 传输 | `path_aware` | `bulk_path_pin` | `bulk_path_checked_per_chunk` | 说明 |
| --- | --- | --- | --- | --- |
| TCP (`ControlInvocation::Tcp`) | false | — | — | 单跳 socket, 无 relay 概念 |
| WebSocket (`ControlInvocation::WebSocket`) | false | — | — | 同上 |
| Zenoh (`ControlInvocation::Zenoh` / `ZenohLocal`) | false | — | — | router 是转发跳, 但不是 iroh relay, 没有 direct/relay 分类 |
| iroh 默认 lane (无 `policy_file`) | true | false | **false** | 不承载 bulk: 回 `iroh_bulk_path_pin_unavailable` (没有租约与主体判定的入口不放字节) |
| iroh 准入 lane (有 `policy_file`) | true | false | **true** | 逐 chunk 检查; **不是** pin, 见 §3.5 与 §3.1 最后两行 |

`path_aware=false` 的传输上, "relay 字节为零"是**拓扑上不存在**的命题 (没有 relay 跳),
快照记 `path_class:"not_applicable"`, 判定门禁不适用. 这一条必须写在验收里,
否则可以用回环 TCP 跑一次 bulk 就"证明"了 bulk 只走直连 —— 那是空真.

### 2.2 请求 / 响应字段 (相对今天的最小增量)

现有五个动词 (`@put-file` / `@get-file` / `@file-chunk` / `@file-end` / `@file-read`) 的形状不变 (见 `src/control_file_transfer.rs` 模块头注), 只加以下字段:

`@put-file` 新增可选字段:

| 字段 | 类型 | 语义 |
| --- | --- | --- |
| `attempt_id` | string, 可选 | 本次尝试的身份. 缺省时 daemon 按 `transfer_id` + 打开序号派生. 用于把"续传的第二次尝试"与"第一次尝试"在证据里分开 (§4.2) |
| `resume` | bool, **必填** | 语义见 §2.5 的 A4/A5/A6 与 D1. 不给默认值: 默认 `false` 会静默抹掉进度, 默认 `true` 会在调用方想重来时静默续上, 两个都错. 今天没有不兼容代价 —— 这条 lane 尚无正式客户端 |

响应 (五个动词的 `ok` 信封):

| 字段 | 语义 |
| --- | --- |
| `evidence` | 见 §4.3 的快照子对象 |
| `resume_honored` | 只有 `@put-file` 会出现. `true` 当且仅当本次是 `resume:true` **且** 起始 offset > 0. `false` 就代表重来了, 不管原因是窗口过期 (A5)、被 sweep 提前删掉 (C3), 还是根本没有半成品. **`state` 必须与它一致**: `@put-file` 的 `state` 只有在 `resume_honored=true` 时才是 `"resumed"`, 否则是 `"open"` —— 两个字段说同一件事, 不能一个说续上一个说重来 |
| `stale_partial_discarded` | 只有 `@put-file` 会出现. `true` 表示本次 open **亲手删掉**了一份超过 `file_transfer.resume_window_ms` 的半成品 (§2.5 A5). 它比 `resume_honored` 更窄: 只覆盖"删除发生在这里", 供排障区分是谁清的 |

#### 稳定码总表 (唯一登记处)

所有稳定码在这一张表登记, 别处只引用不重复定义.

| 码 | 命名空间 | 触发 | 终态 | 可重试 |
| --- | --- | --- | --- | --- |
| `iroh_direct_path_unavailable` | 路径 (`iroh_*`) | open 时观测到没有直连, 当前走 relay (§3.1) | 是 | 路径变直连后可 (D2) |
| `iroh_direct_path_lost` | 路径 | 直连曾建立, 中途丢失 (§3.1) | 是 (本次尝试) | 可, 半成品按 §2.5 B3 保留到窗口末 |
| `iroh_path_unknown` | 路径 | 路径类别读不出 (§3.1) | 是 | 可, 但先修路径可观测性 |
| `iroh_bulk_path_pin_unavailable` | 路径 | 该入口既不声明传输层 pin, 也不做逐 chunk 检查 (§3.5); 今天只有默认入口 (`serve_ping`) 返它 | 是 | 不可, 换到逐 chunk 检查的准入 lane, 或等传输层 pin (RAI-501) |
| `file_transfer_id_in_use` | 会话 | open 时该 `transfer_id` 已有活跃会话 (§2.5 A2) | 是 | 可, 先 `@file-cancel` 再重开 |
| `file_transfer_path_in_use` | 会话 | open 时另一条活跃会话已持有同一暂存路径 (§2.5 A2b) | 是 | 可, 等那条完成或取消它 |
| `bulk_session_limit_reached` | 会话 | 活跃会话数超过 `file_transfer.max_concurrent_sessions` (§2.4) | 是 | 可, 等已有会话收尾 |
| `file_transfer_staging_path_reserved` | 会话 | 目标路径包含保留目录 `.rdog-partial` 组件 (§2.5) | 是 | 不可, 换一个目标路径 |

两个命名空间的分界是**事实类型**, 不是严重程度: `iroh_*` 只回答"路径是什么", 其余一律用
`file_transfer_*` / `bulk_*`. 混着放会让"见到 `iroh_*` 就是路径问题"这条排查直觉失效.

新增动词:

| 动词 | 载荷 | 语义 |
| --- | --- | --- |
| `@file-cancel` | `{transfer_id}` | 释放会话并删除 partial. 幂等: 未知 id 返回 `state:"absent"`, 不算错误 |

`@file-cancel` 不是"顺手加的功能". 今天的会话表 (`TRANSFERS`) 没有任何过期机制,
一次被拒或中断的 178 MB 上传会在盘上留一个暂存文件并且在 `HashMap` 里留一条永久条目.
对"被直连门禁拒绝"的传输尤其致命 —— 拒绝本来应该什么都不留下.

### 2.3 分块与校验和

- 分块大小与上限**不重述**数值: 权威是 `CHUNK_BASE64_LIMIT` 的推导式
  (`src/control_file_transfer.rs:44`) 与帧上限 `MAX_CONTROL_MESSAGE_BYTES` (`src/control_line_read.rs:30`).
  任何载体 (含本文) 不得手抄这两个数.
- 校验和: 单文件 sha256, 由控制端在 `@put-file` 声明、在 `@file-end` 复述; daemon 两处都要比对
  (`execute_file_end` 已实现双校验). 下载方向 `@get-file` 返回源文件 sha256, 由**客户端**在拼装完后比对 ——
  客户端不得直接相信 daemon 的 `state:"committed"`.

### 2.4 大小上限 / 并发 / 取消 / 超时

四项都进 daemon 配置的 `[file_transfer]` 段 (配置段形状沿用 `src/config.rs` 里既有的
`ZenohConfig` / `ObservationConfig` 模式, 见 `src/config.rs:211`、`:173`):

| 配置项 | 语义 | 默认值来源 |
| --- | --- | --- |
| `file_transfer.max_bytes` | 单文件上限. 在 `@put-file` 按声明 size 拒一次, 在 `@file-chunk` 按累计写入**再**拒一次 (声明可以撒谎) | 本规格定义, 待实现落地 (最小要容得下 10-06 规格里记录的 178 MB 模型件) |
| `file_transfer.max_concurrent_sessions` | daemon 侧并发会话上限, 超出返回稳定码 `bulk_session_limit_reached` | 本规格定义 |
| `file_transfer.idle_timeout_ms` | 会话空闲过期: 超过该时长无 `@file-chunk` / `@file-read` 进展, 把会话移出注册表. **是否连带删半成品见 §2.5** | 本规格定义. `idle_timeout_ms` / `sweep_interval_ms` 两项由 RAI-417 实现, 位于分支 `capy/rai-417-transfer-residue` (PR #144), **未合并**; 合入 main 前本行不构成对 `src/` 的指针 |
| `file_transfer.resume_window_ms` | 可续传半成品的保留窗口, 必须 **大于** `file_transfer.idle_timeout_ms` | 本规格定义. 跨字段校验照 `validate_observation_config` (`src/config.rs`) 的同族写法; 承载它的 `validate_file_transfer_config` 随 RAI-417 落地 (分支 `capy/rai-417-transfer-residue`, PR #144, **未合并**) |
| `file_transfer.chunk_ack_timeout_ms` | 客户端侧单块应答超时, 超时即 `@file-cancel` 并失败 | 本规格定义 |

取消链路有两条, 互补, 都要实现:

1. **显式取消**: 客户端调 `@file-cancel` (`@cancel#seq` 取消的是 in-flight 命令, 取消不了已经提交的传输会话).
2. **隐式过期**: `file_transfer.idle_timeout_ms` 的 sweep. 覆盖客户端进程被杀、网络断掉、机器睡眠这些没人来
   调 cancel 的情况.

### 2.5 半成品与会话: 按阶段分四块, 不留未定义的转移

凡是会碰到半成品或会话表的地方只有五个阶段 —— **open 的准入、中止、空闲、窗口到期、提交成功**. 上一版把
它们混成一张按"中止原因"排的表, 于是 open 阶段的三种情形 (同 id 活会话、同 id 换路径、半成品
已过期) 一直没定义. 现在按阶段分开, 每块自己穷举.

全篇只有一条定性判据:

> **半成品里的字节是不是有效数据?** 是 ⇒ 留到窗口末; 不是 (或调用方明确不要) ⇒ 删.

#### A. open 的准入 (先于任何落盘动作)

| # | 情形 | 判定 | 稳定码 / 报告字段 |
| --- | --- | --- | --- |
| A1 | 门禁不通过 (`iroh_direct_path_unavailable` / `iroh_path_unknown` / `iroh_bulk_path_pin_unavailable`) | **拒绝**, 不建会话, 不碰任何半成品 | 见 §3.2 |
| A1b | 目标路径包含保留目录 `.rdog-partial` 组件 (任意位置, 含最后一段) | **拒绝**, 不建会话 | `file_transfer_staging_path_reserved` |
| A2 | `transfer_id` 与一条**活跃**会话相同 (未过期、未中止) | **拒绝**. 否则这次 open 会替换掉那条会话, 而 `resume:false` 会把它正在写的文件删掉 | `file_transfer_id_in_use` |
| A2b | 另一条**活跃**会话已持有同一暂存路径 (不同 id) | **拒绝**. 两条会话写同一个暂存文件会让先到的 `@file-end` 把混合字节的哈希算过、把损坏的文件 rename 到目标上 | `file_transfer_path_in_use` |
| A3 | `transfer_id` 与一条**已中止或已过期**的会话相同 | 允许: 旧条目已被移出, id 复用合法 | — |
| A4 | `resume:true`, 半成品存在且 **mtime 在窗口内** | 允许: offset = 文件长度 | — |
| A5 | `resume:true`, 半成品存在但 **mtime 超过窗口** | 允许, 但**删**该过期半成品, offset = 0, 并在响应里回报 | `stale_partial_discarded: true` |
| A6 | `resume:false`, 存在半成品 | 允许: 显式声明从头来, **删**该半成品, offset = 0 | — |
| A7 | 无既有半成品 | 允许: 只登记, 首个 `@file-chunk` 才落盘 | — |

三条容易读错的:

- **A1 必须先于一切落盘**: open 路径上现有代码是"先 `remove_file(temp_path)` 再登记"
  (`src/control_file_transfer.rs:146-148` 一带). 门禁若放在它之后, 一次被拒的重试就会把上次收到的
  178 MB 抹掉 —— 拒绝有了副作用, 而且是最坏的那种.
- **A5 的 mtime 检查在 open 时做, 不能只靠 sweep.** 索引是内存的 (D 节), daemon 重启后 sweep
  不再认识那份半成品; 但客户端一旦回来 open, mtime 就在眼前, 这一处检查不依赖任何索引.
- **A5 必须回报, 不能静默.** "过期半成品被丢掉、从 0 开始"与"续上了"在回执上必须能分开, 否则
  A5 就是另一个版本的静默重传.

#### 暂存区: 一个保留目录 + 带哈希的名字, 不是"目标名加后缀"

A2b 挡的是"两条活会话写同一个暂存文件". 要让这条门禁成立, 暂存路径先得满足两条:

1. **由目标路径单射得到** —— 两个不同目标不能映射到同一个暂存文件;
2. **与目标命名空间不重叠** —— 不能出现"某个目标名恰好等于别人的暂存名".

早先版本用"目标名 + `.yino-partial` 后缀"同时做这两件事, 结论是**做法本身不成立**: 暂存名就长在
目标的命名空间里, 于是"恰好撞上"的拼法可以无限枚举, 每封一条就冒出下一条. 实测踩到的顺序是:
大小写变体 → Win32 结尾的点/空格 → NTFS 备用数据流 (`x.yino-partial:s`) → 8.3 短名.

**规定一: 保留目录.** 暂存文件一律放在 `<目标目录>/.rdog-partial/`, 目标路径不得包含
`.rdog-partial` 这个组件. 注意是**任何位置都拒, 包括最后一段** —— 一个名叫 `.rdog-partial` 的
普通文件落在某目录里之后, 该目录下所有传输都会因为要建同名目录而失败. 拒绝走稳定码
`file_transfer_staging_path_reserved` (§2.2 总表). 这样第 2 条由"目录不同 ⇒ 命名空间不重叠"给出,
不再依赖枚举平台别名规则.

**这个组件比较不是字面相等**, 否则这条规则在本段开头点名的别名上就是空的:

| 归一 | 理由 |
| --- | --- |
| 去掉结尾的点与空格 | Win32 忽略它们: `.rdog-partial.` 与 `.rdog-partial` 是同一个目录项 |
| 折叠大小写 | APFS / NTFS 默认不敏感: `.RDOG-PARTIAL` 是同一个目录 |
| Windows 上剥掉从 `:` 开始的备用数据流部分 | `x:s` 指向 `x` 的流, 目录项仍是 `x` |

归一后相等即拒. 这三条与"是不是同一个文件"的判定口径**不同** (后者只在 Windows 折叠大小写,
因为在 Linux 上折叠会把两个真实存在的不同文件混成一个) —— 这里是无条件归一, 因为它是命名策略:
拒绝一个 `.RDOG-PARTIAL` 目标在任何平台都不冤枉人, 漏拒却会让目标命名空间与暂存命名空间重叠.

**规定二: 名字里带目标串的短哈希, 并且名字是可复现的.** 文件名形如
`<截断目标名>.<哈希前 16 位>.partial`. 三个要素必须钉死, 否则"同一目标串得到同一名字"这句在跨构建
时不成立, 续传会在升级后找不到自己的半成品:

| 要素 | 规定 |
| --- | --- |
| 哈希函数 | **SHA-256**, 十六进制**小写**, 取**前 16 个字符** (= 8 字节 / 64 位) |
| 哈希输入 | **词法归一后的目标路径字符串** (UTF-8): 去掉 `.` 组件, 其余原样. **不**折叠大小写 |
| 截断 | 目标名在暂存名里截断到**至多 200 字节**, 且**只能切在 UTF-8 字符边界上** (中文名按字节切会切进半个字符, 产生非法文件名甚至 panic). 200 这个预算要让"截断名 + `.` + 16 + `.partial`"留在 `NAME_MAX` 内 (ext4/APFS 按字节计 255, NTFS 按 UTF-16 单元计 255) |

**不要用 `std` 的 `DefaultHasher`**: Rust 文档明确它不保证跨版本稳定, 换了编译器续传就会失效.

哈希不是装饰: 没有它, `Model.bin` 与 `model.bin` 在**大小写不敏感**的卷 (macOS 默认 APFS、
Windows NTFS) 上会得到两个文本不同、却指向同一个 inode 的暂存名 —— 按文本比较的准入看不见这种
重合, 于是第二条 upload 的 `resume:false` 会删掉第一条**正在写**的半成品.

**哈希的保证要说准**: 64 位截断让"同目录下两个不同目标撞同一个暂存名"需要**构造性的碰撞搜索**
(生日界约 2^32 次求值), 不是"永远不会撞". 规格把它定位成防**偶然**重合, 不防一个能预先算出哈希、
并且已经能在被控机上任意写文件的客户端 —— 那种对手不需要这条路径.

**哈希输入先归一是同样必须的**: 直接哈希原始拼写会让 `./a.bin` 与 `a.bin` 得到两个暂存名 ——
它们是同一个文件却互不相认, 续传找不到半成品、占用检查也看不见冲突. 归一去掉 `.` 组件但**不**
折叠大小写: 折叠会把 `Model.bin` 与 `model.bin` 又合回去, 而那两个必须保持不同. 这两条约束同时
成立, 用例要打两侧 —— 别名拼写共享同一暂存路径, 大小写变体仍不同.

两条合起来的判据很干脆: **单射性与隔离性交给"位置 + 名字里的哈希", 不交给"平台怎么算同名".**
用例也要按这个判据写: 断言两个只差大小写的目标拿到的暂存名在**折叠大小写后仍不同**, 而不是只比较
原始字符串.

**已知边界 (不假装解决)**:

- **目的地别名**: 两个目标若被文件系统视为同一个**目的地**, 它们各有自己的暂存文件与各自的
  sha256 校验, 两次提交对同一目的地 rename —— 后者覆盖前者. 这是"两个写者对同一路径"的正常文件
  系统语义; 要彻底避免要给目的地加锁, 本规格不做. 这一类**包括** `x.bin` 与 `x.bin.`、8.3 短名
  与长名, **也包括大小写变体** —— 哈希只消掉了"两个暂存文件在同一条路径上重合"这条会毁数据的
  路径, 它**不**阻止两次提交写同一个目的地. 别把这两件事当成一件: 前者是数据损坏, 后者是覆盖.
- **8.3 短名指向保留目录**: NTFS 开启 8.3 短名生成时, `.rdog-partial` 也有短名别名 (形如
  `RDOG-P~1`); 客户端**故意**按短名拼出目标路径仍可落进暂存区. 堵它需要解析路径的真实身份
  (`GetFinalPathNameByHandle`), 而暂存目录此刻可能还不存在. **不拿"默认关闭"当理由**: 8.3 生成是
  卷级设置, 系统卷上它可能仍然开着, 而用户主目录就在系统卷. 所以这一条是"能力不足 + 攻击需要主动
  构造", 不是"默认不会发生".
- **保留目录里的手工文件**: 该目录在 rdog 写入的目录里视作 rdog 所有 —— 别人手工放进去的
  `<名字>.partial` 会被当作暂存文件回收. 这一条写进代码注释, 不靠"用户不会这么干"糊过去.

用例还要打**前导** `.` 的写法 (`./a.bin`): 内部 `.` (`dir/./a.bin`) 会被 `Path::components` 自己
归一, 纯 Path 相等也能过, 那样写出来的回归用例在修复前就是绿的, 等于没测.

#### B. 中止时的定性

| # | 中止原因 | 会话 | 半成品 | 判据 |
| --- | --- | --- | --- | --- |
| B1 | 显式 `@file-cancel` | 移出 | **删** | 调用方明确表示不要了 |
| B2 | 摘要/字节数不符 (`@file-end`) | 移出 | **删** | 字节确实无效 |
| B3 | 中途路径丢失/不可读 (`iroh_direct_path_lost` / `iroh_path_unknown`) | 移出 | **保留**到窗口末 | 已收字节有效; 续传 offset 就从这份长度推导 |
| B4 | 空闲超过 `file_transfer.idle_timeout_ms` | 移出 | **保留**到窗口末 (上传; 下载本就无半成品) | 空闲只说明"没人来了", 不说明"数据无效" |

B3 与 B4 是同一条判据的两种触发: 只要中断不是调用方主动放弃、也不是数据坏了, 就保留. 上一版把
B3 写成"唯一可续传的中止"是错的 —— 客户端被杀、断网、机器睡眠都归 B4, 那才是最常见的真实中断;
按"仅 B3 可续传"处理, 最常见的反而最不可续传.

#### C. 索引的身份与回收

| # | 事实 | 规定 |
| --- | --- | --- |
| C1 | **索引的身份是半成品路径**, 不是 `transfer_id` | `transfer_id` 由客户端自选且可复用; 盘上的半成品是路径唯一的 |
| C2 | 同 id 换目标路径重开 (A3 的一种) | 旧半成品**保留**, 按**它自己的路径**继续索引, 由 C3 回收. 它的字节仍然有效, 删掉就是拿"id 复用"当理由销毁另一个文件的有效进度 |
| C3 | 超过 `file_transfer.resume_window_ms` 仍未续传 | **删**. 由索引 sweep 执行, 并在 open 时按 A5 兜一次. sweep 删过之后 open 已看不到任何痕迹, 所以"是不是重来了"只能由 `resume_honored` 回答, 不能靠 `stale_partial_discarded` |
| C4 | 提交成功 (`@file-end` 通过校验并 rename 完成) | **移出注册表**. 否则这条已完成的条目仍算"活跃", 同目标的**下一次**上传会被 A2b 判成路径占用而拒绝 —— 一条早该消失的记录把正常重传挡在门外 |

**C2 的过渡状态要写明**: RAI-417 (分支 `capy/rai-417-transfer-residue`, PR #144, **未合并**) 今天
在"同 id 换路径"时是**回收旧半成品**的. 那是因为它既没有路径索引、也没有续传窗口 —— 在它的世界里
旧半成品没人回收, 删是当时唯一的正确选择. C2 与 C3 落地后 (RAI-367 实现阶段) 这条过渡行为失效,
换成"按路径留着、由窗口回收".

#### D. 两条顺序要求与一个必填字段

1. **`resume` 是 `@put-file` 的必填字段, 不给默认值.** 默认 `false` 会静默抹掉进度, 默认 `true`
   会在调用方其实想重来的时候静默续上 —— 两个默认值都错. 今天收紧零代价 (这条 lane 尚无正式
   客户端), 以后再收就要破兼容. CLI 侧对应规则: 检测到窗口内的半成品时默认发 `resume:true`.
2. **索引在内存里, 这是已知边界.** daemon 重启后索引丢失, C3 的 sweep 不再认识那些半成品; A5 的
   open 时 mtime 检查只能救"客户端还会回来"的那部分. 要覆盖"永远不回来的半成品"需要持久化
   partial 索引 (真相源: 文件 mtime + 路径), 已开 **RAI-424**, 本规格不展开.

**续传窗口用文件长度 + mtime, 不用额外记账**: offset 的真相源是半成品自身的长度
(`execute_put_file` 的 resume 分支), 窗口到期判定的真相源是它的 mtime. 两者都在盘上, 不引入
第二份"已收字节"状态.

---

## 3. 直连强制语义

### 3.1 判定表 (穷举, 按尝试的开头与每个 chunk 边界各判一次)

| `path_aware` | 采样到的 `path_class` | 时机 | bulk 判定 | `error_code` |
| --- | --- | --- | --- | --- |
| true | `direct` | open / 任意 chunk 边界 | **允许** | — |
| true | `relay` | open | **拒绝**, 未写任何字节 | `iroh_direct_path_unavailable` |
| true | `direct` → `relay` | 中途 chunk 边界 | **中止**, 保留 partial 以便续传 | `iroh_direct_path_lost` |
| true | `unknown` / 读取失败 / 未建立 | open / 任意 chunk 边界 | **拒绝** (fail-closed) | `iroh_path_unknown` |
| true | `relay` → `direct` | 中途 | 不可达: relay 在 open 已被拒, 没有字节可以"切回" (见 §8 的 D2) | — |
| true | 任意 | open, **`bulk_path_pin=false` 且 `bulk_path_checked_per_chunk=false`** | **拒绝**, 该传输不得承载 bulk (见 §3.5) | `iroh_bulk_path_pin_unavailable` |
| true | `direct` / `relay` / `unknown` | open / 任意 chunk 边界, **`bulk_path_checked_per_chunk=true`** | 与上四行**完全一致**: 逐 chunk 检查只改变"谁来判", 不改变判据 | 同上三码 |
| false | `not_applicable` | open | **允许**, 证据记 `not_applicable` | — |

读表时最容易错的两条, 一条关于 `unknown`, 一条关于"检查"与"钉住"的区别:

- **`bulk_path_checked_per_chunk=true` 不等于 `bulk_path_pin=true`**. 前者是"每个边界查一次",
  后者要求"边界之间的排队、重传与路径迁移都不改走 relay" (§3.5). 判据相同, 保证不同; 报告里
  只能用前者措辞。

三条读表时容易读错的:

- `unknown` **永远不许**被强转成 `direct`. 一个"读不到就当直连"的门禁就是伪装成门禁的静默回退.
- `path_aware=true` 且 `path_class` 采样缺失 (老 iroh 版本、字段被裁掉、解析失败) 归入 `unknown`, 不归入 `not_applicable`.
- `not_applicable` 是**声明出来的**能力缺失 (适配器自己声明 `path_aware=false`), 不是"没读到", 两者不得混用.

### 3.2 原因码命名空间

原因码走既有 `iroh_*` 前缀, 与父票措辞对齐 (`direct_path_unavailable` / `direct_path_lost`).
**码的登记处是 §2.2 的稳定码总表**, 这里只解释路径族四个码为什么这样分:

四个码按"路径层的哪一个事实不成立"分, 触发/终态/可重试三列一律以 §2.2 总表为准
(这里不再抄第二份 —— 抄两份的代价是下一次改一处、另一处留成旧合同):

- `iroh_direct_path_unavailable` —— 从来没有直连;
- `iroh_direct_path_lost` —— 曾经有直连, 中途丢了;
- `iroh_path_unknown` —— 根本问不到路径类别 (fail-closed);
- `iroh_bulk_path_pin_unavailable` —— 该入口既不钉也不查, 因此不放字节 (见 §3.5).

**刻意不加**第四个"bulk 被门禁拒绝"的码: 门禁相对路径层没有增加任何语义, 多一个码只会让
"relay" 有两种写法, 逼调用方同时匹配两个. 会话级资源拒绝用 `bulk_session_limit_reached`,
它**不进** `iroh_*` 命名空间 —— 它不是路径事实, 混进去会让"看到 `iroh_*` 就是路径问题"这条
排查直觉失效.

### 3.3 明确禁止项 (写成条款, 用来否掉后续"临时可用"提案)

| 禁止 | 理由 |
| --- | --- |
| `--allow-relay` / `file_transfer.allow_relay` / 任何形式的 relay 放行开关 | 一旦存在, "bulk 只允许直连"就退化为"默认只允许直连". 要证明的是不变量, 不是默认值 |
| 直连不可得时降级为控制面内联 (把文件塞进 `@run` / `@cmd` 的 shell 参数) | 同一条字节流换了动词而已, 仍然过 relay, 而且绕开了本规格全部超时与上限 |
| 直连失败后自动重试 N 次再报错 | 重试可以在路径层做 (见 D2), 但**不能**在 bulk 原语内偷偷改路径类别. 每次尝试的路径类别都要单独入账 |
| 把路径判定结果缓存超过一次尝试 | 缓存是"上一次是直连"的证据, 不是"这一次是直连"的证据 |

### 3.4 门禁落在哪一层 (两条方向各一个承重点)

不变量是"bulk 字节不经过 relay", 而**只有产生字节的那一端能保证这一点**:

| 方向 | 字节从哪来 | 承重门禁 | 检查点 |
| --- | --- | --- | --- |
| 上传 (`@put-file`/`@file-chunk`) | 控制端 (客户端) | **客户端**拒发 | open 前 + 每个 chunk 发送前 |
| 下载 (`@get-file`/`@file-read`) | 被控端 (daemon) | **daemon**拒读 | open 时 + 每个 `@file-read` 读取前 |

推论: **只在 CLI 里做门禁等于没做下载方向的门禁** —— 任何老客户端发一条 `@get-file`,
daemon 都会把 178 MB 从 relay 推出去. 所以两端都要, 且两端都要在 chunk 边界复查 (不是只查 open,
否则中途 direct→relay 的切换会漏过去).

### 3.5 应用层采样是必要条件, 不是充分条件

§3.1 的判据是**应用层采样**: open 与每个 chunk 边界各读一次路径类别. 这必要但不充分 ——
一个 chunk 通过检查之后可能仍排在发送队列里, 或者需要重传; 如果路径随后切到 relay,
那批已经放行的字节会从 relay 走, 而下一个 chunk 边界的检查只是"知道得太晚".

所以不变量的落点必须有一部分在**传输层**, 而不是全部靠应用层轮询:

| 要求 | 归属 | 不满足时 |
| --- | --- | --- |
| **逐 chunk 检查**: open 与每个 chunk 边界采样路径, 读到非 `direct` 即拒 | 承载 bulk 的入口 (应用层, 适配器声明 `bulk_path_checked_per_chunk=true`) | 该传输不得承载 bulk, 或只按路径类别判定并**如实登记它没做到的那一半** |
| **传输层钉住**: 禁止路径迁移, 排队中与重传的字节不得改走 relay | RAI-413 传输适配器 (`bulk_path_pin=true`) | 该传输**拒绝**承载 bulk (`iroh_bulk_path_pin_unavailable`), 或只申报逐 chunk 检查 |
| 排队中与重传的字节计入 `relay_tx_bytes` / `relay_rx_bytes` | RAI-413 传输适配器 | 零值无法区分"没走 relay"与"没记账", 观测口径失效 |
| `path_aware=true` 且 `bulk_path_pin=false` 且 `bulk_path_checked_per_chunk=false` | 本规格 | **拒绝** bulk: 既不钉也不查, 采样到 direct 也不代表之后的字节 |

四条读法 (第二与第三行是两条**独立**义务, 不能互相顶替):

- `path_aware=false` 说的是**拓扑里没有 relay 跳** (TCP/WebSocket/Zenoh), 所以钉路径这件事不存在;
- `bulk_path_checked_per_chunk=true` 说的是**每个边界都查过**, 边界之间的排队、重传与路径迁移仍未覆盖;
- `bulk_path_pin=false` 说的是**没做到传输层那两条义务**, 所以不得用"检查过了"去主张"钉住了";
- 把第三种当第一种, 就得到一条"看起来有门禁、其实只在边界上有"的路径; 把第二种当第三种, 则白扔了
  它确实做到的那一半。两种错都在这一格上发生过, 所以能力是**三格**而不是一个布尔。

**当前落地形态 (2026-11-26 RAI-476 步骤 1+2, 按评审裁定回填).** iroh **准入 lane** 承载 bulk 数据面:
七个 bulk 动词与 `@ping` 共用同一条连接, 每条 chunk 是一条独立请求行 (`serve_admitted`, 见
`specs/rdog-iroh-admission-contract.md`); 未开准入策略的默认 lane 仍拒绝 bulk (`readonly_request` →
`iroh_bulk_path_pin_unavailable`)。适配器声明是 `path_aware=true` / `bulk_path_pin=false` /
`bulk_path_checked_per_chunk=true` (`src/control_iroh.rs` 的 `ADMITTED_LANE_PATH_CAPABILITY`):

- open 与每个 chunk 边界各采样一次当前连接路径 (`route(&conn, routable)` → `PathClass`), 读不出先按
  20 × 50ms 有界重试, 穷尽才 fall back `unknown`;
- `direct` 放行; `relay` 拒 `iroh_direct_path_unavailable`; 读不出拒 `iroh_path_unknown`;
- 本次尝试**曾**观测到 `direct`、之后掉成 `relay` ⇒ 拒 `iroh_direct_path_lost` (「曾直连」按
  `transfer_id` 记录, **open 动词无条件复位**, 判定失败即移除该条记录);
- `@file-list` / `@file-cancel` 不搬字节, 不过路径门禁: 拒一条 cancel 只会把半成品留在盘上.

**这一版没有做到的部分必须说准**: `conn.paths()` 是**事后快照**, 它既禁不了 QUIC 的路径迁移, 也看
不见排队与重传里改走 relay 的字节; `iroh_bulk_path_pin_unavailable` 今天的含义是「这个入口既不钉也不
查」—— 只有默认入口 (`serve_ping`) 返它, 准入 lane 靠逐 chunk 检查放行, 与 pin 无关. 真做传输层 pin
另开票 (RAI-501). 结论的措辞上限是"**每个 chunk 边界上路径是直连**", 不得写成"连接被钉
在直连上"、"relay 上 bulk 字节为 0" 或"迁移与重传已覆盖"。

**门禁粒度 (上传方向尤其要写清)**: 上传 chunk 的载荷**内联在请求行里**, daemon 要先读到
`transfer_id` 才能判 —— 那一 chunk 的字节在判定之前**已经过线**。所以本层的不变量是 **at most one
chunk in flight**: 每个被拒的边界保证的是"这一 chunk 之后不再有字节", 不是"一字节都没上线"。
"上传方向线上一字节为零"只能由**发送端的 pre-send 门禁**保证 (§3.4 的上传行), 不在本层。

**验收边界 (别读过头)**: 上述事实的运行时证据只有**回环** (§8 的用例与 `src/control_iroh/bulk_tests.rs`)
—— 回环拓扑只会给出 `direct`, 所以 `relay` / 读不出 / 中途掉路径三种读数由测试钉住喂进门禁, 不是
真拓扑采样. 真机 (RAI-476 步骤 3+4) 的采样与两端快照 join 都还没做。

上一版这一段写的是"bulk 在 lane 上根本不被接受, 不变量由**能力拒绝**保证", 再上一版把逐 chunk 检查
登记成了 pin —— 两版都不再成立, 但各自当时都指出了真实的一半: 前者说明"没发字节"是最不空真的保证,
后者说明"边界上都查过"确实是可观测的进步. 现在这一版把两半分开记, 不再用一半冒充另一半.


---

## 4. 请求级观测口径

### 4.1 计数在哪一层

计数点必须落在**传输适配器的 relay 传输句柄**上: 即该端点经 relay 的累计发送/接收字节数
(`relay_tx_bytes` / `relay_rx_bytes`, 由 iroh 的 relay 传输暴露). 两条硬约束:

- **不得由配置推断**. "我的配置说 direct"不是观测, 是假设.
- **不得在 relay 服务端做**. relay 转发的是不透明 QUIC 数据报, 它无法把字节归属到应用层请求;
  在 P7 的拓扑里 relay 还是不归我们运营的 ECS 主机.

这一条是已知限制, 要写进证据里而不是藏起来: **relay 侧字节数只能由端点自报, 不能由 relay 自证.**

### 4.2 怎么归属到单个请求: 括号采样

一次尝试的生命周期内, 在两端各采样两次累计量: 打开前 (`before`) 与结束/中止后 (`after`).
该尝试的 relay 归属字节 = `after - before`.

括号采样只有在**同一连接上没有其他并发流量**时才是干净的单次归属. 因此:

- bulk 尝试 SHALL 独占其控制会话/连接 (不与 PTY 会话或其他 one-shot 共用).
  不独占时读数会被同连接的无关流量污染, 只产生"该为零而不为零"的假阳性, 不产生假阳性安全 ——
  所以这一条是**证据洁净度要求**, 不做成硬拒绝.
- 若 daemon 检测到同一连接上存在并发流量, 快照里 `concurrency_suspected:true` 必须置位,
  该条证据不得单独用于"relay 字节为 0"的结论.

### 4.3 快照格式与判定式

两端各发自己那一半, 由验收工具 join, **任何一端都不得替对端填数**:

```json
{
  "schema": "rdog.bulk-path.v1",
  "attempt_id": "<string>",
  "transfer_id": "<string>",
  "direction": "put | get",
  "path_aware": true,
  "bulk_path_pin": false,
  "bulk_path_checked_per_chunk": true,
  "path_class": "direct | relay | unknown | not_applicable",
  "direct_addr": "<脱敏后的对端直连地址, 仅 direct 时存在>",
  "bulk_bytes": 123,
  "wire_bytes": 164,
  "chunks": 3,
  "concurrency_suspected": true,
  "endpoint": "client | daemon",
  "relay_tx_before": 0, "relay_tx_after": 0,
  "relay_rx_before": 0, "relay_rx_after": 0,
  "relay_sample_available": true,
  "aborted": false,
  "verdict": "<见 BulkPathSnapshot::verdict()>"
}
```

`bulk_bytes` 是载荷字节 (base64 解码后), `wire_bytes` 是实际压上控制行的 base64 字节 ——
两者都要, 因为"relay 上为 0"证的是后者, 而用户关心的是前者. 两者都只记**被对端确认过**的字节:
记账在发送之前会把从未落盘的字节算进去, 两端 join 就对不上账.

**四个计数与 `concurrency_suspected` 都可以是 `null`**: `null` 表示**本端没有采样能力**, 不是
"测到了 0"/"没有并发". 类型定义是 `Option<u64>` / `Option<bool>` (`src/bulk_path.rs`), 不是
`u64` + 一个"记得同时设"的布尔. `relay_sample_available` 是这两件事在 JSON 上的落点.

`verdict` 的取值与判定**不在这里枚举** (枚举两处必然分叉): 唯一真相是
`BulkPathSnapshot::verdict()`, 它今天会给出 `aborted` / `not_applicable` / `relay_sample_unavailable` /
`no_bulk_bytes` / `relay_bytes_zero` / `relay_bytes_nonzero` 之一. 其中前四个都**不是结论**.

**判定式** (可执行实现: `BulkPathSnapshot::relay_bytes_zero_own_half()`, `src/bulk_path.rs`;
验收脚本按此逐字实现, 不做主观解读. 正对照那一条的判据在 §4.4 的表里, 仓库内的可执行参考是
`bulk_path::tests::positive_control_uses_a_strict_inequality` —— 它今天**没有生产消费者**
(正对照要一条真走 relay 的拓扑, 见 §4.4 与 RAI-366); daemon 侧的 relay 计数已经接线 (RAI-476
步骤 3), 但"计数接上了"与"正对照跑过了"是两件事):

**本端**构成 `relay_bytes_zero` 的四个必要条件 (缺一即是别的 verdict):

1. 不是中止 (`aborted == false`);
2. **本次确有字节移动** (`chunks > 0 || bulk_bytes > 0`) —— 一次"续传时半成品已经完整"的尝试
   一个字节都不用发, 它的零与"没走 relay"数值上完全一样, 这是最纯的空真。
   **这条扣掉的只是路径结论, 不是传输结论**: 空文件 / 无需搬字节的尝试**成功了**这件事由正常回执
   表达 (`@file-end` 的 `state:"committed"`、`@file-read` 的 `eof:true`、客户端 `transfer` 收据),
   它只是拿不到"relay 字节为 0"这句话。两份都这样的快照 join 起来是 `joined` + `relay_bytes_zero:null`
   (`bulk_path_inconclusive_no_bulk_bytes`), 不会被读成"传输失败"或"relay 上有字节";
3. `path_class == "direct"`;
4. 四个计数**都采到了** (`relay_sample_available == true`) 且本端 `relay_tx` 与 `relay_rx` 的
   `after - before` **都为 0** (减法可失败: `after < before` 是"读数作废", 见下节第 3 条)。
   计数为 `null` 表示未采样, 不是零.

不满足时按 `BulkPathSnapshot::verdict()` 的顺序给出 `aborted` / `not_applicable` /
`relay_sample_unavailable` / `no_bulk_bytes` / `relay_bytes_nonzero` 之一 —— **前四个都不是结论**,
尤其 `not_applicable` (没有 relay 跳, 空真) 与 `relay_sample_unavailable` (采不到计数) 不得被读成
"relay 字节为 0". 跨端结论必须两端各自的 `relay_bytes_zero_own_half()` 都为真 (`§4.3` 的 join).

#### daemon 半快照落在哪条响应里 (不新开传输)

daemon 这一半**不另起通道**: 它作为终态 ok 信封里的 `evidence` 字段返回, 形状与本节的快照
schema 完全一致 (`endpoint:"daemon"`). 三个终态各挂一处, 除此之外不挂:

| 方向 / 终态 | 承载响应的位置 |
| --- | --- |
| 上传 (`put`) | `@file-end` 的应答 (提交成功与失败都算终态; 失败那次 `aborted:true`) |
| 下载 (`get`) | `eof:true` 的那一帧 `@file-read` 的应答 |
| 中止 | `@file-cancel` 的应答, 以及路径门禁拒绝时那条拒绝响应 (`aborted:true`) |

非终态的应答 (`@put-file` / `@file-chunk` / 非 EOF 的 `@file-read`) **不带** `evidence`:
半快照在括号还没闭合时没有意义, 提前发一份只会让人以为那就是最终结论.

括号两端是**这条连接上被归类为 relay 的 path 的累计收发字节** (`conn.paths()` 的每个 `Path`:
`remote_addr()` 判 direct/relay, `stats().udp_tx/udp_rx.bytes` 取字节). `before` 在 open 执行
**前**取, `after` 在终态请求执行**后**取. 读不到就是四个 `null`, 绝不写 0, 具体三条:

1. 连接上一条 path 都枚举不到 (读到空快照) → `null`;
2. `before` 时在场的 relay path 在 `after` 时不见了 → `null` (`PathList` 只保留未关闭的 path,
   关闭那条的字节已经不在快照里, 相减会得到一个假的零). **在场判据是 `PathId` + 地址, 不是地址**:
   relay path 关闭后以同一 `RelayUrl` 重建是 iroh 的正常事件 —— 新 path 是新 `PathId`、计数从 0 起,
   地址字符串却一模一样, 只看地址会把"换了条 path"读成"同一条还在";
3. `after < before` (计数倒退) → `null`: 相减必须 `checked_sub`, 不许 `saturating_sub` ——
   后者把 `1000 → 100` 算成一个漂亮的 0, 而那个 0 会被读成"测到了零";
4. 测试面显式给 `None` → `null`.

**已知边界 (不假装解决)**: 一条 relay path 若在括号**内部**开了又关, 两端快照都看不到它, 这条
检测不出来. 它需要订阅 `PathEvent::Closed` 并按连接累计关闭字节 (iroh 的 `PathList` 文档明说关闭
的 path 不保留). 本版不做, 因此这里能主张的只是"括号两端可见的 relay 字节增量为 0",
不是"这条连接上从未有字节经过 relay".

#### join 工具与原因码

`rdog bulk-path join <PATH>...` 读若干 `rdog.bulk-path.v1` 文件 (或目录里的 `*.json`), 先按
`transfer_id` 分桶、**桶内再按 `attempt_id` 分**, 左右由每份快照自己的 `endpoint` 决定 (不看文件名、
不看顺序 —— 按顺序配会在少给一个文件时把两份客户端快照当成两端). 分组键必须是两者合起来:

- 同一个 `transfer_id` 上的**两次尝试** (续传重开、同 id 幂等重试) 是两次尝试, 各自成一条报告;
  只按 `transfer_id` 分组会把它们塞进一组、报"重复端"并整组作废, 连那次完整尝试也读不出来。
- 反过来, 一次尝试若被交成两个 `attempt_id` 各一半 (桶里恰好两组、各只有一端、两端互补),
  真因是"两端对不上号", 报 `bulk_path_attempt_mismatch`, 不报成"缺一端"。

输出 `rdog.bulk-path-join.v1`:

```json
{
  "schema": "rdog.bulk-path-join.v1",
  "transfer_id": "<string> | null",
  "attempt_id": "<string> | null",
  "status": "joined | unknown",
  "reason": "<原因码> | null",
  "client": { "endpoint": "client", "verdict": "...", "relay_bytes_zero_own_half": false,
              "relay_sample_available": false, "path_class": "direct", "aborted": false,
              "concurrency_suspected": null } | null,
  "daemon": { ...同上, endpoint:"daemon" } | null,
  "relay_bytes_zero": true | false | null
}
```

`relay_bytes_zero` 是**唯一允许的跨端结论**, 并且是**三态**, 不是布尔:

| 值 | 什么时候 | 读法 |
| --- | --- | --- |
| `true` | 两端 `relay_bytes_zero_own_half()` 都为真 | 两端各证了自己那一半 |
| `false` | **至少一端** `verdict() == "relay_bytes_nonzero"` | 真的测到了 relay 上有字节 |
| `null` | 其余全部: 任一端 `no_bulk_bytes` / `relay_sample_unavailable` / `not_applicable` / `aborted`, 或任一端 `concurrency_suspected:true` | **没证出来**, 不是证否 |

把第三格压成 `false` 是把"未采样 ≠ 0"做对之后**反向**漏掉的同一类谎: 读到 `false` 的人会以为
"测到了 relay 流量", 真相却是"这次没有结论". 退出码: `0` 每组都给得出结论 (`true` 或 `false`),
`1` 至少一组配上了但没结论、或根本没配上, `2` 参数 / IO 错误. "配上了但没结论"必须落在 `1`,
不能混进 `0`.

`joined` 但 `relay_bytes_zero` 为 `null` 时, `reason` 给 `bulk_path_inconclusive_<verdict>`
(两端 verdict 不同且都不是零时按本端 → 对端用 `+` 串起来, 如
`bulk_path_inconclusive_no_bulk_bytes+relay_sample_unavailable`). 这一族**不复用**下面 `unknown`
那套码 —— "没配上"与"配上了没结论"是两件事, 混用会让排障的人先去找缺失的文件.

| 原因码 | 触发 |
| --- | --- |
| `bulk_path_endpoint_missing` | 同一 `transfer_id` 只有一端 (`client` 或 `daemon` 缺一) |
| `bulk_path_endpoint_duplicated` | 同一 `transfer_id` 出现两份同 `endpoint` 的快照 |
| `bulk_path_attempt_mismatch` | 两端的 `attempt_id` 不一致 |
| `bulk_path_schema_mismatch` | `schema` 不是本版 / 缺键 / 计数与 `relay_sample_available` 互相矛盾 |
| `bulk_path_concurrency_suspected` | 任一端 `concurrency_suspected:true` (§4.2: 该证据不得单独用于结论) |
| `bulk_path_inconclusive_<verdict>` | `joined` 但没有可用结论: 任一端 `no_bulk_bytes` / `relay_sample_unavailable` / `not_applicable` / `aborted` |

这些**不是**控制面稳定码, 不进 §2.2 的码表: 只有验收工具与人在读, 命名空间仍是 `bulk_path_*`.
前五个描述"这两半为什么没有组成报告", `bulk_path_inconclusive_*` 描述"报告组成了但没有结论" ——
后者不改变 `status` (仍是 `joined`).

`attempt_id` 由客户端在 `@put-file` / `@get-file` 载荷里交给 daemon (§2.2 的可选字段); 缺省时
daemon 按 `<transfer_id>#<该 id 被 open 的序号>` 派生. 派生的那个与客户端自选的值不会相等, 所以
**两端都交身份之前 join 只会得到 `attempt_mismatch`** —— 那是实话, 不是缺陷.

### 4.4 正对照 (本规格最容易被做坏的地方)

一次"直连尝试 delta 为 0"的读数, 与"计数器根本没接上"的读数,**在数值上完全一样**.
所以每一次验收运行里都必须包含正对照:

| 对照 | 构造 | 期望 |
| --- | --- | --- |
| 正 | 同一条 relay 路径上发一条普通控制面请求 (`@ping`), 同一仪表、同一次运行 | 两端 `relay_tx/rx` delta **严格 > 0** |
| 负 | 同一条直连路径上的一次 bulk 尝试 | 两端 delta **== 0** |

断言必须是**严格不等式** `> 0`. `>= 0` 的断言在计数器坏掉时照样通过, 等于没有正对照.

> 实现状态 (2026-11-26 更新): 判定表、四格语义与快照格式在 `src/bulk_path.rs`; iroh **准入 lane** 以
> `path_aware=true` / `bulk_path_pin=false` / `bulk_path_checked_per_chunk=true` 承载 bulk, 并在 open
> 与每个 chunk 边界读路径 (RAI-476 步骤 1+2, 按评审裁定回填)。**正对照仍未执行** —— 它需要一条真的
> 走 relay 的拓扑 (RAI-366), 在那之前"relay 上 bulk 字节为 0"这条不做结论; 而且本版连传输层 pin 都
> 没有, 所以那句结论的资格比上一版更低: 能主张的只有"每个边界上路径是直连"。
>
> **`established_direct` 现在有了生产者, 但只在 daemon 侧**: `BulkPathGate` 在读采样的同时记录
> 「本次尝试是否曾观测到 direct」, 所以 §2.5 D1 的「中途 direct → relay」(`iroh_direct_path_lost`)
> 在 lane 上真的会走到, 并有回环用例 (钉住读数) 与门禁单测覆盖. **客户端门禁仍恒传
> `established_direct = false`** (`src/transfer_cli.rs`), 因为客户端还没有采样源 —— 所以
> "上传方向的中途路径丢失已处理"**不得主张**: 上传字节由客户端发出, 那一侧的记录还没接上.
>
> `PathClass::Relay` 上那行带撤除条件的 `#[allow(dead_code)]` 已按约定撤掉: 采样生产者到位后,
> 它是门禁里会走到并有用例覆盖的分支.
>
> **未验的一条要说清**: `src/main.rs:503` 仍恒传 `NOT_PATH_AWARE`, `rdog --put` / `--get` 根本走不到
> 这条 lane —— 今天只有手写的控制行 (或用例) 能触发它。所以"真机上 lane 会拒 relay"这件事**没有被
> 证明**, 只有回环 + 钉住读数.
>
> 跑批单 (`handoffs/rai367/positive-control-runsheet.md`) 是 RAI-367 当时的形态记录 (那时 lane
> 不承载 bulk), 不再是当前行为的描述; 新的回环用例在
> `src/control_iroh/bulk_tests.rs`.
>
> **daemon 侧的括号采样已接线 (RAI-476 步骤 3)**: `@file-end` / `eof:true` 的 `@file-read` /
> `@file-cancel` 与门禁拒绝响应里带 `evidence` (形状与 §4.3 的 schema 一致, `endpoint:"daemon"`),
> 括号两端的读数取 `conn.paths()` 上 relay path 的累计字节. 但**它只在回环 + 钉住读数上验过**:
> 回环拓扑只有 direct, relay 读数由 `PathSampler::Script` 钉住喂进去, 真机上一次"确实有 relay 流量
> 的括号"没有被执行过. 所以实况是 —— 仪表接上了, 正对照 (需要 RAI-366 的 relay 拓扑) 仍未跑:
> **拿不到正对照读数时, 门禁那个"零"不得用于任何结论**.
>
> **两端 join 的工具已就绪** (`rdog bulk-path join`, §4.3), 但客户端驱动 `rdog control --put/--get`
> 仍恒传 `NOT_PATH_AWARE` (`src/main.rs`), 它写出的客户端那半永远是 `path_class:"not_applicable"`
> 且计数为 `null`; 于是跨端 `relay_bytes_zero` 在当前 CLI 路径上不可能为真. 这不是缺陷而是缺口:
> 客户端要走到 iroh lane 才能产出可用的一半.

---

## 5. CLI 入口

形状沿用 `rdog control` 现有的 flag 风格 (`--pty`、`--ui-script` 就是这么加的, 见 `src/input.rs:124-153`):

```
rdog control <TARGET> --put <LOCAL_PATH> --remote-path <REMOTE_PATH> [--resume]
rdog control <TARGET> --get <REMOTE_PATH> --local-path <LOCAL_PATH> [--resume]
```

| 项 | 规定 |
| --- | --- |
| 目标解析 | 复用 `resolve_control_invocation` (`src/control_invocation.rs:95/131/207`), 与其它控制面调用**完全同一套** target/namespace/entry-point/transport 解析. 这是选 flag 而不是选 `rdog transfer` 独立子命令的理由: 独立子命令要把这段解析再写一遍 |
| 互斥 | 与 `--pty` / `--pty-close` / `--pty-detach` / `--pty-attach` / `--ui-script` / 尾部 `@<one-shot-line>` 全部互斥, 用 clap `conflicts_with_all`, 与 `src/input.rs:128-145` 一致 |
| stdout | 只有一行结果 JSON (与其它控制响应的打印方式一致) |
| stderr | 进度行 (每 N 字节一条), 不污染 stdout |
| 退出码 | 沿用现有约定: 成功 0, 失败 1. **不新造退出码** —— 细分原因读结果 JSON 里的 `error_code` |
| 下载侧校验 | 客户端拼装完必须自己算 sha256 与 `@get-file` 返回的比对, 不符即失败并删本地半成品 |
| 直连门禁 | 无开关. 由 §3 判定表决定, 默认且唯一行为 |
| `--resume` | 检测到未过期的半成品时**默认按续传发** (`resume:true`), 把"从头来"留给调用方显式选择. 理由同 §2.5: 静默抹掉 178 MB 进度是比多传一遍更坏的默认 |

远端路径与本地路径**不做 shell 解析** (走 JSON 字段, 这是 10-06 规格里列出的相对 scp 的价值之一).

---

## 6. 实现落点清单 (供实现票拆用, 本步不写代码)

| # | 落点 | 内容 |
| --- | --- | --- |
| 1 | `src/control_file_transfer.rs` | `attempt_id` 字段、`@file-cancel`、`file_transfer.max_bytes` 双次校验、会话 TTL sweep、并发上限 |
| 2 | 传输适配器 (iroh: RAI-413) | 暴露 `path_aware` / `bulk_path_pin` / `path_class` 采样 + `relay_tx_bytes` / `relay_rx_bytes` 累计量 (含排队与重传字节). 这是本规格对 RAI-413 的硬要求, 是**依赖接口**不是本票实现 |
| 3 | `src/control_core.rs` 派发 | daemon 侧下载方向门禁 (open + 每次 `@file-read` 前) |
| 4 | 新客户端驱动 (与 `src/control_invocation.rs` 同层) | 流式: 读一块 → 发一块 → 等应答 → 推进 offset, 不整文件驻留 |
| 5 | `src/input.rs` + `src/main.rs` | 四个新 flag + 互斥 + 调用驱动 |
| 6 | `src/config.rs` | `[bulk]` 段四个配置项 |
| 7 | 验收工具 | 两端快照 join (`rdog bulk-path join` → `src/bulk_path.rs::join_inputs`) + §4.3 判定式 + §4.4 正对照 (未执行) |

---

## 7. 未决 / 依赖 (实现前必须收口)

1. **RAI-366 未交付时, 验收 A 类不可执行.** "直连可用"需要一个真实的、可复现的直连拓扑;
   今天 iroh lane 的直连从未在任何拓扑上打通 (P7 累计结论: 第 1 项"部分"). 本规格不替它下结论.
2. **RAI-413 未落地时, 第 2 条落点不存在.** `src/control_iroh/` 今天不在 main、不在任何远程分支
   (已逐分支核过). 本规格里所有 `iroh_*` 码与 `path_class` 采样都是**在本规格里首次定义的契约**,
   实现落地后须回填到 `src/` 的具体位置, 并按 `docs/agents/knowledge-carriers.md` 的约定
   把"待回填"改成指针.
3. **`file_transfer.*` 默认值**由实现票给具体数值; 本文只固定语义与判定, 不固定数字 (避免再造第二份真相源).

---

## 8. 验收矩阵

四类都必须有原始回执. 每行的证据形式统一, 便于逐行对账.

### A 类: 直连可用

| 项 | 内容 |
| --- | --- |
| 输入 | `rdog control <TARGET> --put <178MB 文件> --remote-path <REMOTE>` |
| 前置 | RAI-366 给出 direct 可用的拓扑; 双方 `@ping` 返回同 id 且 `pong:true`; 路径层报 `direct` |
| 预期 | 传输完成; 响应 `state:"committed"`; 返回 `size` 与 `sha256` 与本地一致; 快照 `path_class=direct`, 两端 delta 均 0, `verdict=relay_bytes_zero` |
| 证据 | 两端快照 JSON; 传输前后的 `sha256sum`; 二进制 sha256; 原始响应行 |

### B 类: 直连不可得 (只有 relay)

| 项 | 内容 |
| --- | --- |
| 输入 | 同上, 在已知只能走 relay 的拓扑 (如 CGNAT 侧) |
| 前置 | 路径层报 `relay`; `@ping` 仍能 `pong:true` (证明控制面可用, 只是没直连) |
| 预期 | **请求失败**, 一个 bulk 字节都不发; `error_code:"iroh_direct_path_unavailable"`; daemon 侧无 partial 残留; 不需要等到超时, open 阶段即拒 |
| 证据 | 两端快照 (`verdict=aborted`); 失败响应原文; **同一路径上 `@ping` 快照的 delta 严格 > 0** (正对照, §4.4); 失败后目标路径与临时路径均不存在的 `ls` 回执 |

### C 类: relay 存在但直连失败

| 项 | 内容 |
| --- | --- |
| 输入 | 同上, 拓扑上双方都能到 relay, 但打洞失败 |
| 前置 | 路径层报 `relay`, 且能区分"根本没试 direct"与"试了失败". 后者才有区分价值 |
| 预期 | 同 B 类结果 (`iroh_direct_path_unavailable`), 但证据里要能读到"直连尝试存在且失败"这一层, 不能只有"relay" |
| 证据 | 同 B 类 + 路径层的尝试记录 |

B 与 C 的区别不在结果 (两者都必须失败), 而在**失败原因的可归因性**: C 类如果读到与 B 类
一模一样的证据, 说明路径层没有区分"没试"和"试了失败", 那 RAI-366 的排障会瞎.

### D1 类: 中途 direct → relay

| 项 | 内容 |
| --- | --- |
| 输入 | 大文件上传, 传输进行中把路径打断 (断 UDP / 切网) |
| 前置 | 起始 `direct`, 传输已过一个或多个 chunk |
| 预期 | **中止**, `error_code:"iroh_direct_path_lost"`; 已写 partial **保留到 `file_transfer.resume_window_ms`** (续传 offset 就是从它推导的, 删了就退化成重传); 快照 `verdict=aborted` 且 delta 非 0 |
| 证据 | 两端快照; 中止时的 offset (证明不是从头开始也不是静默完成); partial 存在与大小; 续传一次到 `direct` 后完成的回执, 且续传的起始 offset **不等于 0** (证明是真续传而不是静默重传); **在途字节的归属**: 中止前已放行但未确认的字节数与其最终路径类别 (验证 §3.5 的**逐 chunk 检查**在路径变化后确实拒下一 chunk, 而不是"查到得太晚"; 传输层 pin 属 RAI-501, 不在本条验收面) |

### D2 类: relay → direct

| 项 | 内容 |
| --- | --- |
| 输入 | 先在被拒的 relay 状态下发起 (B 类现场), 修好直连后**重新发起**同一文件 (`--resume`) |
| 前置 | 第一次尝试已在 open 被拒, 零字节 |
| 预期 | 第一次 `iroh_direct_path_unavailable`; 路径改善后第二次 `direct` 并完成; 两次 `attempt_id` **不同**, 各自快照独立 |
| 证据 | 两次尝试的两端快照 (attempt_id 不同); 两次的 `error_code` / `state` |

D2 的形状与票面字面不同, 这里要说明原因: **relay 上开头的 bulk 尝试已在 open 被拒, 没有字节,
所以 bulk 内部不存在一个 "relay → direct" 的切换可观测.** 唯一可观测的 relay→direct 转移是
"拒 → 重试成功". 把它写成"中途从 relay 切到直连完成"会验收一个不可能发生的状态,
反而掩盖了"拒绝是不是粘性的、续传能不能接上"这两个真问题.

### 每条回执的固定字段

所有类别的回执都带: 两个端点的二进制 sha256、拓扑描述 (哪些段跨网)、脱敏后的地址、
判定式计算结果、`attempt_id`. 缺任一项按未验收处理.
