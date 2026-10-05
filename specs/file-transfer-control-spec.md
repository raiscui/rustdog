# 文件传输通道规格

> 状态：**提案**（待拍板 4 项决策）
> 来源：2026-10-05 远程验证 Electron 自动更新链期间，跨机搬运全部依赖 scp

## 目标

让 rdog 控制面具备**双向文件传输**能力，使 agent 在远程作业中**不依赖第二套通道**
（今天的 SSH/SCP）。

这份文档是文件传输 lane 的需求与决策记录，不是施工说明。

---

## 需求来源（真实场景）

2026-10-05 远程验证期间跨机搬运全部靠 `scp`：

| 传输物 | 大小 | 方向 |
| --- | --- | --- |
| `cutout.exe`（Windows 抠图执行体） | 22 MB | mac → win |
| `rdog.exe` + 配置 | 43 MB | mac → win |
| `isnet-general-use.onnx`（推理模型） | 178 MB | mac → win |
| 辅助脚本 ×3 | < 10 KB | mac → win |
| 验收截图 ×6 | ~1 MB × 6 | win → mac |

这些全部绕过了 rdog —— 因为没有这条路。

---

## 现状：`SaveFile` 帧是为截图设计的，不是传输通道

`control_frames.rs:45` 的 `SaveFileFrame` 是项目里唯一的文件载荷能力：

```
SaveFileFrame {
  request_id, filename, mime, encoding,   // encoding 只支持 base64
  data,                                     // 整文件塞进一个字段
  quality, width, height,                   // ← 截图语义
}
```

四条硬约束，让它无法承担文件传输：

1. **单向**。只有「被控端 → 控制端」（截图回传），没有上传方向。
2. **不分块**。整个文件编码进单个 JSON 字段。178 MB 模型 base64 后约 237 MB
   —— 单行 JSON 不可行。
3. **协议层无帧大小上限**。`split_object_fields`（`control_frames.rs:1124`）
   只做 JSON 字段切分，**不对字段长度设限**。这不是"大文件传不了"，
   是"缺一个边界"—— 大文件会把读取侧撑进未定义行为。
4. **无校验、无续传、无进度**。传完不知完不完整（今天每次传完都在外部手动核对
   体积/哈希），中断只能整个重来。

**结论：复用 `SaveFile` 不合适，另建传输 lane。** `SaveFile` 的语义是
"我给你看这张图"（mime/quality/width/height），传输的语义是
"搬一个 178 MB 的东西"（offset/校验/续传/进度）—— 两者生命周期与失败模式
都不同，合并会让 `SaveFile` 变形。

---

## 相对 scp 的价值主张

不是"再实现一个 scp"，是补上 scp 在 agent 场景的三个不适配：

| scp 的问题 | rdog 的答案 |
| --- | --- |
| **两端都要 SSH**。今天 Windows 侧得确认 sshd 开着、配好 key | 只有被控端跑 daemon，控制端用现有 rdog 通道 |
| **大文件无进度**。178 MB 传多久、传没传完，ssh 客户端不反馈 | 协议内建 `@file-progress`，agent 能向用户汇报 |
| **完整性靠自觉**。传完要不要校验，全凭操作者 | 协议内建 sha256 断言，不符即失败并清理 |
| **路径是位置参数**。跨 shell/跨平台引号与转义规则各异 | 路径走 JSON 字段，无 shell 解析层 |

**同时说清不替代 scp**：交互式人工传文件、延迟敏感的小文件、跨多机批量
搬运，scp 仍更顺手。本 lane 的定位是**让 agent 在远程作业中不依赖第二套通道**。

---

## 待拍板的决策

### 决策 1：chunk 载荷编码

| 方案 | 评价 |
| --- | --- |
| **A. 沿用 base64**（与 `SaveFile` 一致，推荐） | 零新依赖、纯文本帧、与现有解析器同构；代价 33% 膨胀 |
| B. 十六进制 | 100% 膨胀，更差 |
| C. 新增二进制帧类型 | 最优，但 line-based 文本传输层要长出二进制旁路，改动面大 |

agent 场景下带宽不是瓶颈，正确性与可恢复才是 —— 33% 膨胀换零协议改动是划算的。

### 决策 2：chunk 默认大小

1 MB（base64 后约 1.33 MB/行）是自然起点。但**必须先给帧字段加显式上限**
（超出即协议错误，而非静默撑爆缓冲），chunk 取上限的 80%，
保证"chunk 永远塞得进一行"。

**顺序很重要**：先定上限，再定 chunk。

### 决策 3：接收侧路径策略

写入被控端任意路径意味着 agent 能在被控机任意写文件。这与 `@cmd`
（同在 remote shell 权限面内）是同一量级能力，但传输 lane 容易被当成
"安全的文件投递"而放宽审查。

**推荐**：写入前规范化路径，拒绝 `..` 穿越与符号链接逃逸；不强制限定
只能写某目录（会挡掉大量真实用途），但**必须拒绝穿越**。

### 决策 4：目录打包传输

今天传的 `.ps1`/配置是一堆零散小文件，逐个传啰嗦。

**推荐**：本期**不做**目录打包（YAGNI，未来真需要再评估 zip 通道），
但 chunk 协议预留 `relative_path`，使上层将来能组包而无需改协议。

---

## 建议的协议形态

```
@put-file:{path, size, sha256?, resume?}   控制端 → 被控端：宣告"我要写这个文件"
@get-file:{path, offset?}                  控制端 → 被控端：请求读取（可带断点）
@file-chunk:{transfer_id, offset, data}    双向复用
@file-progress:{transfer_id, sent, total}  任一方向
@file-end:{transfer_id, sha256, ok}        结束 + 校验
```

`transfer_id` 由发起侧生成，双方据此配对，天然支持并发多传输。

`@file-progress` 的帧型已存在（`ControlFrame::TaskProgress`，`control_frames.rs:35`），
传输 lane 复用同一形状即可，无需新帧型。

---

## 实现约束

1. **帧大小上限是前置项**。`split_object_fields`（`control_frames.rs:1124`）
   不限字段长度，chunk 落地前必须先补边界，否则"传大文件"会把读取侧
   打进未定义行为。
2. **续传需要接收侧记住 offset**。`@put-file` 宣告后，被控端建立临时文件并
   记住已写长度；重连时 `@put-file` 带 `resume:true` 续写。
3. **校验失败要清理**。sha256 不符时删除临时文件并报错 —— 否则坏文件会以
   "看起来装好了"的形态留在盘上。
4. **大文件要能观测**。协议内建进度，agent 才能向用户汇报而不是干等。

---

## 需求

### 文件传输 SHALL 在控制通道上支持双向

- 控制端 `@put-file`（path/size/可选摘要）→ 被控端备好临时目标并回报就绪
  → 后续 `@file-chunk` 按声明的 offset 写入
- 控制端 `@get-file` → 被控端以 `@file-chunk` 流回 → `@file-end` 带摘要收尾
- 传输 SHALL NOT 需要 SSH 之类的第二通道

### chunk 载荷 SHALL 有界，不得超过协议帧上限

- 超过上限的 chunk 请求 SHALL 被拒绝
- 上限 SHALL 在读取侧同样强制，使超大帧产生**协议错误**而非无界缓冲
- 超限时 daemon SHALL 保持可用，后续请求不受影响

### 传输 SHALL 可续传

- 中断后，控制端以 `@put-file` + `resume:true` 重发
- 被控端回报已持久接收的 offset，传输从该 offset 继续，不重传已收字节

### 完成时 SHALL 校验摘要，不符 SHALL 清理

- 摘要不匹配 → 报失败并**删除临时文件**，不得留在原地
- 校验通过才 SHALL 报告成功

### 接收路径 SHALL 规范化并拒绝穿越

- 目标路径含 `..` 且解析后越出目标位置 → 拒绝，且**不创建任何文件**
- 符号链接逃逸同样拒绝

---

## 相关

- [远程被控端实战反馈](remote-control-blocked-command-pitfalls.md) —— 同一场作业暴露的
  控制面问题
- [CDP 远程通道规格](cdp-remote-lane-spec.md) —— 共享"读取侧无界缓冲"这个前置项
