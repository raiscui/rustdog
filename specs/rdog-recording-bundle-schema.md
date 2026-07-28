# rdog Recording Bundle schema 与原子导出

## Status

本规格是 Wayfinder ticket [定义 Recording Bundle schema 与原子导出](https://github.com/raiscui/rustdog/issues/9) 的 resolution asset。

它定义 Recording Session `completed` 后必须原子提交的 Recording Bundle 的物理形态、内部文件、manifest schema、完整性模型、size 边界、evidence 门禁和远程交付协议,不实现 Bundle writer 生产代码。

## Scope

本规格只定义:

- Recording Bundle 的物理形态 (单一确定性 TAR 归档)
- 内部必需文件清单 (`manifest.json` / `journal.jsonl` / `flow.json` / `evidence/`)
- `manifest.json` schema 与 provenance
- 完整性模型 (per-file SHA-256 + whole-archive SHA-256)
- TAR 字节确定性 (POSIX USTAR)
- JSON canonical bytes
- evidence allowlist 与 redaction gate
- manifest redaction summary
- manifest warnings
- Bundle / 单帧硬上限
- 显式不录制连续视频
- 远程交付协议 (单帧 `@savefile`)
- frame 顺序与最终 `@response`
- completed retry 幂等性
- connection ownership 与速率限制
- `delivery_failed` 语义

以下内容由其他规格负责:

- Recording Session lifecycle 与 state machine: `specs/rdog-recording-session-lifecycle.md`
- Recording Journal event schema (`rdog.recording.v1`): `specs/rdog-recording-journal-model.md`
- 输入脱敏与 Replay Parameter: `specs/rdog-recording-redaction-parameter-model.md`
- Semantic promotion 与坐标 fallback: `specs/rdog-recording-semantic-promotion-policy.md`
- Participating Window 与 Window Geometry Precondition: `specs/rdog-recording-window-geometry-policy.md`
- Replay Script (`rdog.flow.v1`): `specs/rdog-flow-control-plan.md`
- 窗口控制契约 (`@window-resize`): `specs/rdog-window-control-plan.md`
- 截图与 AX manifest 契约: `specs/rdog-ax-screenshot-manifest-control-plan.md`
- 多显示器截图坐标契约: `specs/rdog-multi-display-screenshot-coordinate-plan.md`
- Display scope / window identity: `specs/rdog-display-scope-control-plan.md`、`specs/rdog-display-aware-control-chain-plan.md`
- line-control 协议: `specs/control-line-protocol.md`

## Terms

- **Recording Bundle**: stop 成功后原子提交的正式资产,作为单一确定性 TAR 归档存在。
- **manifest**: Bundle 内 `manifest.json`,描述 Bundle 身份、provenance、内部文件清单与完整性。
- **journal**: Bundle 内 `journal.jsonl`,是 Recording Session 的 canonical source,byte-for-byte 复制 frozen Journal。
- **flow**: Bundle 内 `flow.json`,是 `rdog.flow.v1` Replay Script,不是第二真相源。
- **evidence**: Bundle 内 `evidence/<artifact_id>.<ext>`,由显式 `@record-mark` 或 `default_mark_evidence` 引入的资产。
- **lifecycle owner**: 持有 Recording Session 的 controller connection,拥有 Bundle delivery 接收权。
- **delivery_failed**: 接收端 sha256 校验失败的 transient 状态,不影响 `completed` 事实。
- **redaction segment**: Journal 内标记 sensitive 或 unknown 输入区间的不可变边界。
- **stash**: Bundle commit 前存放完整 Bundle 的临时 staging path。

## Invariants

1. Recording Bundle 是单一确定性归档文件,不是目录也不是多文件集合。
2. Bundle 是 canonical committed artifact;展开目录只是 staging 或 cache,不是第二真相源。
3. `journal.jsonl` 是唯一 canonical source;`flow.json` 与 `summary` 字段都是派生或辅助资产。
4. `manifest.json` 不内嵌自身哈希,避免 self-hash 递归。
5. Bundle commit 之前失败,Session 必须 `failed`,不得暴露 partial Bundle。
6. Bundle commit 之后 delivery 失败,Session 保持 `completed`,不回退状态机。
7. completed retry 必须返回相同归档字节、相同 `bundle_sha256` 与 `bundle_size_bytes`。
8. 远程 delivery 只发给 lifecycle owner,其它 connection 不能接收 Bundle 字节。
9. `delivery_failed` 不写入 lifecycle metadata 或 manifest,只在最终 `@response` 中以 stable reason code 暴露。
10. 超过 size 上限必须在 commit 前 fail closed,不能截断、压缩或分块。
11. 视频或音频流不进 Bundle,即便显式 opt-in 也不接受。
12. sensitive 或 unknown 区间覆盖的 evidence 必须整份抑制,不局部打码。
13. Reader 遇到不识别的 additive warning code 必须接受 Bundle 并原样展示。
14. 不支持的 manifest major version (`rdog.recording.bundle.v2`) 必须 fail closed。
15. Bundle commit 时间不写入归档,保留在 lifecycle metadata 与最终 `@response`。

## Physical form

### Canonical form

- 单个文件 `<recording_id>.rdogrec.tar`。
- 使用 POSIX USTAR,不压缩,不加密,无签名。
- 扩展名 `.rdogrec.tar` 表明 rdog Recording Bundle 容器,标准 `tar` 工具可直接读取。
- Bundle commit 失败前,完整归档位于 staging path;commit 通过同一文件系统的 `rename(2)` 完成。
- commit 成功后,源 staging path 不再可访问,只允许通过已提交归档路径读取。

### Staging

- staging path 是同文件系统的临时路径,不跨文件系统。
- staging 不创建展开目录,也不接受外部直接写入。
- staging 期间任何写失败或 fsync 失败,staging path 删除,Session 进入 `failed`。
- staging 期间不暴露 partial 路径给 controller,只在 commit 后返回最终归档路径与 `bundle_sha256`。

### Cache

- Bundle commit 后允许保留本地展开目录作为 cache,但 cache 只是派生资产,不能作为第二真相源。
- cache 在下一次相同 `recording_id` 的 completed retry 时可以被覆盖。
- cache 不参与 hash 校验,不参与 reader 验证。

## Internal files

### Required files

- `manifest.json`: Bundle schema、identity、provenance、files 清单与 redaction summary。
- `journal.jsonl`: byte-for-byte 复制 frozen Recording Journal。
- `flow.json`: 编译成功的 `rdog.flow.v1` Replay Script。

### Optional files

- `evidence/<artifact_id>.<ext>`: 由显式 `@record-mark` 或 `default_mark_evidence` 引入的 evidence asset。

### Files that must not exist

- `summary.json`、`evidence/index.json`、`evidence/manifest.json`。
- 任何运行日志、trace、video、audio、raw event dump。
- 任意未登记的 artifact 文件。

## Entry order

```
manifest.json
journal.jsonl
flow.json
evidence/<按 path 升序排列>
```

- 当 evidence 集合为空时,`evidence/` 目录条目不创建。
- Entry 顺序固定,不允许重排。
- 每个 entry 路径受控,必须是相对 ASCII 路径,无绝对路径、无 `\`、无 `.` 或 `..`、无空 segment、无重复路径。

## TAR determinism

### Format restrictions

- 仅 POSIX USTAR,不写 PAX 或 GNU 扩展。
- 仅允许 regular file entry,不写目录、符号链接、硬链接、ACL、xattr 或设备节点。
- entry 名称按 entry order 写入。

### Header normalization

```
mode: 0644
uid: 0
gid: 0
mtime: 0
uname: ""
gname: ""
```

- 未使用的 header 字段全部清零。
- File padding 必须填零。
- 归档以两个 512-byte 零 block 结束。
- 结束 block 后不允许附加数据。

### Path safety

- 拒绝绝对路径、`\`、`.`、`..`、空 segment、重复路径、非 ASCII 字符。
- 拒绝长度超过 100 字节的 entry 名称。
- 拒绝超过 `evidence/<path>` 形式的相对路径。

## JSON canonical bytes

### journal.jsonl

- 直接复制 frozen Journal 的原始字节。
- 不重新 parse、排序或 serialize。
- 保持现有 UTF-8、LF、无 BOM 规则。

### manifest.json 与 flow.json

- UTF-8,无 BOM。
- 使用 compact JSON,不写缩进或多余空白。
- Object key 在每一层按字典序排列。
- 有业务顺序的 array 保持原顺序。
- 表示集合的 array 必须先按 schema 指定的稳定 key 排序。
- Scalar 使用 `serde_json` 的稳定转义和数字表示。
- 拒绝 NaN、Infinity 等非 JSON 数值。
- 文件末尾固定只有一个 LF。

### evidence files

- evidence 文件保持捕获或生成完成后的原始字节。
- Bundle finalizer 不对 evidence 重新编码。

## Manifest schema

### Schema identity

```json
{
  "schema": "rdog.recording.bundle.v1",
  "archive_format": "posix-tar",
  "journal_schema": "rdog.recording.v1",
  "flow_schema": "rdog.flow.v1"
}
```

- `schema` 是 Bundle manifest schema 标识字符串。
- `archive_format` 固定为 `posix-tar`。
- `journal_schema` 与 `flow_schema` 引用当前 Journal 和 Replay Script schema。
- v1 只允许向后兼容的 additive 字段;Reader 可以忽略未知的可选字段。
- 不支持的 major version 必须 fail closed。
- 缺少必需字段、字段类型错误或内容 schema 与声明不一致时,必须拒绝 Bundle。
- 破坏性变更才升级 major version;首版不增加单独的 minor version 字段。

### Required fields

| 字段 | 类型 | 含义 |
| --- | --- | --- |
| `schema` | string | `rdog.recording.bundle.v1` |
| `archive_format` | string | `posix-tar` |
| `journal_schema` | string | `rdog.recording.v1` |
| `flow_schema` | string | `rdog.flow.v1` |
| `recording_id` | string | Session 索引字段,必须与归档文件名一致 |
| `started_at_unix_ms` | integer | Session 起始 wall-clock 时间,必须与 Journal 一致 |
| `producer` | object | 写入 Bundle 的 rdog 程序身份 |
| `compiler` | object | Replay 编译算法与策略身份 |
| `files` | array | 除 manifest 自身外的文件清单 |
| `redaction_summary` | object | redaction 聚合字段 |
| `warnings` | array | 非致命审计摘要 |

### Session identity

```json
{
  "recording_id": "rec-opaque",
  "started_at_unix_ms": 1784678400000
}
```

- 两个字段都必须与 `journal.jsonl` 完全一致。
- `recording_id` 还必须与归档文件名一致。
- 不写 `stopped_at`、`completed_at` 或 `duration_ms`。
- 结束时间和 duration 从 Journal monotonic timeline 推导。
- Bundle commit 时间保留在 lifecycle metadata 中,不写入归档。

### Producer provenance

```json
{
  "producer": {
    "name": "rdog",
    "version": "3.0.0"
  }
}
```

- `producer.version` 来自 Cargo package version。
- 首版不记录 Git commit、build timestamp、hostname、用户名、绝对路径或 target triple。

### Compiler provenance

```json
{
  "compiler": {
    "name": "rdog-replay-compiler",
    "version": "1"
  }
}
```

- `compiler.version` 是 Replay 编译算法和策略集合的独立版本。
- 只要相同 Journal 可能因为 compiler 或 promotion policy 变化而生成不同 `flow.json`,就必须递增 `compiler.version`。
- Provenance 只用于审计和问题定位,不替代 Bundle、Journal、flow schema 的兼容门禁。

### Files array

```json
{
  "files": [
    {
      "path": "journal.jsonl",
      "role": "journal",
      "media_type": "application/x-ndjson",
      "size_bytes": 12345,
      "sha256": "<64位小写十六进制>"
    },
    {
      "path": "flow.json",
      "role": "flow",
      "media_type": "application/json",
      "size_bytes": 6789,
      "sha256": "<64位小写十六进制>"
    }
  ]
}
```

- `files` 覆盖 `journal.jsonl`、`flow.json` 和全部 evidence。
- `manifest.json` 不记录自身哈希,避免 self-hash 递归。
- `files` 按 `path` 升序排列。
- 每个 entry 必须包含 `path`、`role`、`media_type`、`size_bytes` 和 `sha256`。
- 任意缺件、额外文件、size 或 hash 不匹配都必须 fail closed。
- `role` 允许值为 `journal` / `flow` / `evidence`。
- `evidence` entry 的 `path` 必须以 `evidence/` 开头。

### Redaction summary

```json
{
  "redaction_summary": {
    "segment_count": 2,
    "required_parameter_count": 1,
    "suppressed_evidence_count": 1,
    "runtime_clipboard_exposure": true
  }
}
```

- `segment_count`: 从 Journal redaction segments 重算。
- `required_parameter_count`: 从 `flow.json` 的 required parameters 重算。
- `suppressed_evidence_count`: 统计因 `sensitive`、`unknown` 或 active redaction 而未持久化的 evidence。
- `runtime_clipboard_exposure`: 只要 Replay 包含 paste parameter 或 clipboard mode,就必须为 `true`。
- 所有计数必须是非负整数。
- Manifest、Journal 和 flow 计算结果必须完全一致。
- 不复制 parameter id、descriptor、target、classification 明细或 reason 明细。
- 不保存值长度、hash、prefix、suffix、字符类别或其他可关联特征。
- Summary 不匹配时,Bundle validation 必须在 commit 前失败。

### Warnings

```json
{
  "warnings": [
    {
      "code": "guarded_coordinate_fallback_used",
      "count": 2
    },
    {
      "code": "optional_evidence_failed",
      "count": 1
    }
  ]
}
```

- `warnings` 是必需数组,没有 warning 时写 `[]`。
- 每个 warning 只包含稳定的 `code` 和正整数 `count`。
- 相同 code 必须聚合为一项。
- 数组按 code 升序排列。
- 不保存自由文本 message、路径、target、backend error 原文或其他不稳定细节。
- 首版只定义:
  - `optional_evidence_failed`
  - `guarded_coordinate_fallback_used`
- Reader 遇到未知 additive warning code 时应接受 Bundle,并原样展示该 code。
- Warning 不改变 Replay policy,也不能放宽验证门禁。

### Fatal cases

以下情况绝不能降级为 warning:

- Required lane failure 或不可恢复 gap。
- Journal、flow 或 schema 无效。
- Locator 歧义、stale 或无法验证。
- Display topology 或 geometry precondition 失败。
- Redaction 违规。
- 文件缺失、额外文件、hash 或 size 不匹配。
- Bundle 超过 size 上限。
- Atomic commit 失败。

## Integrity model

### Per-file SHA-256

- `manifest.files[*].sha256` 是对应文件内容的 SHA-256,以 64 位小写十六进制表示。
- 计算范围是该文件 entry 的全部 data bytes,不含 TAR header 或 padding。
- 计算顺序固定: `journal.jsonl` → `flow.json` → `evidence/*`。
- manifest self-hash 不计算,避免递归。

### Whole-archive SHA-256

- TAR 完成后,对整个归档字节计算 `bundle_sha256`,保存在 lifecycle metadata。
- `bundle_size_bytes` 是归档总字节数,包含 TAR header、data、padding 和两个结束 block。
- `bundle_sha256` 与 `bundle_size_bytes` 通过最终 `@response` 返回,不写回归档。
- Completed stop retry 必须返回相同的 `bundle_sha256` 和 `bundle_size_bytes`。

### Verification order

Reader 验证 Bundle 必须按以下顺序:

1. 解析归档路径中的 `recording_id`,确认与 `manifest.recording_id` 一致。
2. 解 TAR 并按 entry order 读取必需文件。
3. 校验 entry count、entry name 与必需/可选文件列表匹配。
4. 校验 entry path、header 字段与 TAR 结束 block 符合 determinism 规则。
5. 计算每个 entry 的 SHA-256,与 `manifest.files[*].sha256` 比对。
6. 计算整个归档的 SHA-256 与 size,与 lifecycle metadata 比对。
7. 校验 `manifest.redaction_summary` 与 Journal、flow 一致。
8. 校验 `manifest.warnings` 数组 schema。
9. 校验 `journal.jsonl` 与 `flow.json` 的 schema 标识。

任何步骤失败必须拒绝 Bundle。

## Evidence allowlist

### Allowed roles

```text
screenshot_image
screenshot_manifest
ax_snapshot
```

- Evidence 必须来自显式 `default_mark_evidence` 或 `@record-mark.evidence`。
- 不进行默认或连续后台采集。
- `screenshot_image` 使用现有截图格式。
- `screenshot_manifest` 和 `ax_snapshot` 使用 JSON。
- 成功的 Journal artifact reference 必须能在 Bundle 中找到唯一对应文件。

### Failed evidence

- Evidence 状态为 `failed`、`blocked` 或 `redacted` 时,只保留 Journal 状态和非敏感原因,不生成占位文件。

### Sensitive / unknown

- 捕获点位于 active redaction interval,或 focused target 被分类为 `sensitive` 或 `unknown` 时,整份 screenshot 或 AX snapshot 都不持久化。
- 首版不尝试局部打码,避免遮罩仍泄露密码长度或遗漏 AX value。

### Forbidden artifacts

- Video、音频、运行日志、trace、raw event dump 和任意未登记 artifact 一律不允许进入 Bundle。
- Recorder capture backend 不提供 video / audio channel;`@record-mark.evidence` 不接受 video role。

## Size limit

### Hard limits

- 单个 Bundle 解压后总字节数 `<= 256 MiB`。
- 对应 `@savefile` base64 帧总字节数 `<= 384 MiB`(≈256 × 1.5 + JSON 包装)。

### Commit-time enforcement

- Bundle commit 之前计算解压后总字节数。
- 超过 `bundle_too_large` 上限必须 fail closed,返回 protocol 层 `bundle_too_large`。
- 不允许压缩、截断、剔除 evidence、分块或运行时参数化上限。

### Receiver-time enforcement

- 接收端 base64 解码后字节数超过 256 MiB 时拒绝整个 Bundle。
- 接收端将 Session 标记为 `delivery_failed`,reason code `bundle_too_large`。
- 接收端保留已写入 staging 的部分文件,但不返回成功。

## Remote delivery

### Single-frame delivery

- 每个 Bundle 使用单个 `@savefile`,payload 为 `<recording_id>.rdogrec.tar` 的完整 base64 字节。
- `filename` 字段固定等于归档文件名。
- `mime` 字段固定为 `application/vnd.rdog.recording-bundle`。
- `encoding` 字段固定为 `base64`。
- 不拆分多帧、不发送 chunk,也不在帧内携带 offset、total 或 hash。
- Completed stop retry 重发时,必须保持完全相同的 payload 字节。

### Receiver-side validation

- 接收端按 `sanitize_filename` 与现有冲突命名规则处理落盘路径。
- 接收端 base64 解码后计算 SHA-256,与 daemon 提供的 `bundle_sha256` 比对。
- 接收端不计算 whole-archive SHA-256;`bundle_sha256` 由 daemon 在 commit 时计算。
- 接收端校验 `bundle_size_bytes` 是否等于 staging 落盘字节数。
- sha256 不一致时,接收端必须将 Session 标记为 `delivery_failed`,而不是返回成功。

### Frame order

- daemon 发送完 `@savefile` 后必须立即发出最终 `@response`,且二者之间不能夹杂其它 control frame。
- 顺序固定: `@record-stop` → `@savefile` → `@response`。
- 最终 `@response` 同时包含本次 Bundle 的 `bundle_filename`、`bundle_size_bytes`、`bundle_sha256`。
- 完成 retry 时,`@savefile` 与 `@response` 顺序不变,但 `@response` 内 `bundle_*` 字段保持与首次一致。
- `@record-status` 只在 Session 进行中可用,`completed` Session 不会返回 `recording` 或 `compiling` 等中间态。
- 客户端必须按 `@response` 中是否携带 `bundle_*` 三元组,作为本轮 Bundle 序列结束的唯一信号。
- 首版不允许通过另外的 channel、附件或 endpoint 拆分 Bundle,也不在 `@response` 中携带 `payload_base64`、file id 或 download URL。

### Final response

```json
{
  "bundle_filename": "<recording_id>.rdogrec.tar",
  "bundle_size_bytes": 67108864,
  "bundle_sha256": "<64位小写十六进制>"
}
```

- `bundle_filename`、`bundle_size_bytes`、`bundle_sha256` 三元组必须同时出现。
- 三元组在整个 completed retry 期间保持不变。
- 接收端完成 sha256 校验后才返回成功。
- 接收端 sha256 校验失败时,`@response` 同时携带 `delivery_failed` 与 `delivery_failed_reason_code`。

### Delivery failed

```json
{
  "delivery_failed": true,
  "delivery_failed_reason_code": "checksum_mismatch"
}
```

- `delivery_failed` 必须与 `delivery_failed_reason_code` 同步出现。
- 当前首版定义 `checksum_mismatch` 与 `bundle_too_large` 两个 reason code。
- 二者都出现在最终 `@response`,**不**写入 `@record-status` 的临时返回。
- 任何成功响应都不允许携带 `delivery_failed` 字段。
- 不复制错误原文、堆栈、底层错误细节或前端调试信息。
- Daemon 在写回 `delivery_failed` 后保持 lifecycle 状态为 `completed`,**不**重新进入 `frozen`、`failed` 或 `cancelling`。

## Completed retry

### Idempotency

- 同一个 `record_stop_request_id` 在 daemon 端绑定到固定 `recording_id` 的 `completed` Session。
- Daemon 检测到 `@record-stop` 携带的 `request_id` 已与现有 completed Session 绑定时,跳过 lifecycle、journal 与编译路径,直接重发同一个 `@savefile` payload 和最终 `@response`。
- 重发时使用原 request id 作为响应中的 `request_id`,**不**生成新 id。
- Daemon 不校验调用方是否持有原 request id,只要 Session 处于 `completed` 且 lifecycle metadata 中存在 `bundle_sha256`,就走重发路径。
- `@record-stop` 携带新 request id 但指向同一 `recording_id` 时,行为与原 request id 相同,同样走重发路径。
- Daemon 仍对 `@record-stop` 做一次权限与 capability 校验,但**不**重复校验 Recorder 状态机或冻结 Journal。
- Retry 期间不允许修改 lifecycle metadata、`bundle_sha256`、`bundle_size_bytes` 或 `bundle_filename`。

### Connection ownership

- `@record-stop` 只有发起该 Session 的同一 connection 被允许完成 delivery。
- 其它 connection 上的 `@record-stop`、`@record-status` 都被 daemon 拒绝,返回协议层 `permission_denied` 或 `not_owner`。
- 其它 connection 仍可以收到 `@record-status` 返回的 Session 状态,但**不**包含 `bundle_*` 三元组,也不会触发 `@savefile` 或收口 `@response`。
- 客户端在同一连接上 reconnect 后,只要 `record_stop_request_id` 仍然在 lifecycle metadata 中,就走 completed retry 路径并由该连接接收 `@savefile` 与 `@response`。
- 新的 connection 想接收 delivery 时,必须先建立新的 Recorder Session 并独立录制,旧 Bundle 不支持多 connection replay。
- Daemon 不在其它 connection 上推送 `@savefile` 或最终 `@response`,也不通过 broadcast、broadcast key 或 Zenoh 共享内存传播 Bundle 字节。

### Rate limit

- 客户端 retry `@record-stop` 由调用方决定节奏,daemon 不主动拒绝快速 retry。
- 单个 connection 在 1 秒内发送超过 5 次 `@record-stop` 时,daemon 视为速率越界,直接返回 `rate_limited`,**不**重发 `@savefile` 与 `@response`。
- 速率越界不影响 Session lifecycle,Session 保持 `completed`。
- 速率越界计数与 retry 计数不写入 lifecycle metadata,不进入 Bundle、Manifest 或 `@response`。
- 越界后 1 秒窗口不重置时,连接进入协议层 `cooldown`,持续到 1 秒空窗后自动恢复。
- 不引入指数退避、token bucket、最大 retry 计数或最长 cooldown 时间。
- Daemon 不在 `@record-status` 临时响应中暴露 `rate_limited` 计数或 cooldown 时间。

## Failure paths

### Pre-commit failures

- Required lane failure 或不可恢复 gap: Session 进入 `failed`,Bundle 不提交,staging 删除。
- Journal / flow / manifest schema 无效: Session 进入 `failed`,Bundle 不提交。
- Locator 歧义、stale 或无法验证: Session 进入 `failed`,Bundle 不提交。
- Display topology 或 geometry precondition 失败: Session 进入 `failed`,Bundle 不提交。
- Redaction 违规: Session 进入 `failed`,Bundle 不提交。
- 内部文件缺失、额外文件、hash 或 size 不匹配: Session 进入 `failed`,Bundle 不提交。
- Bundle 超过 size 上限: Session 进入 `failed`,Bundle 不提交。
- Atomic commit (`rename(2)`) 失败: Session 进入 `failed`,staging 删除,已写入的归档路径不可访问。
- Cancel 期间不编译、不提交 Bundle。

### Post-commit delivery failures

- 接收端 sha256 校验失败: Session 标记 `delivery_failed`,lifecycle 保持 `completed`。
- 接收端 base64 解码失败: Session 标记 `delivery_failed`,lifecycle 保持 `completed`。
- 接收端 size 上限超出: Session 标记 `delivery_failed`,reason code `bundle_too_large`。
- 接收端超时或断线: Session 保持 `completed`,客户端可重发 `@record-stop` 触发 retry。

### Crash

- Recorder 在 `recording` 阶段 crash:Journal 保留为 orphan,不重新进入 Bundle commit。
- Recorder 在 `finalizing` 阶段 crash:staging 删除,Session 进入 `failed`。
- Recorder 在 `completed` 阶段 crash:lifecycle metadata 保留,允许后续 completed retry 重放相同归档。
- Crash orphan 不被自动恢复、不被自动导出。

## Cross references

- `specs/rdog-recording-session-lifecycle.md`: Session lifecycle、state machine、commit / cancel 路径。
- `specs/rdog-recording-journal-model.md`: `rdog.recording.v1` Journal event schema。
- `specs/rdog-recording-redaction-parameter-model.md`: sensitive / unknown 分类与 Replay Parameter 模型。
- `specs/rdog-recording-semantic-promotion-policy.md`: Semantic promotion 与坐标 fallback。
- `specs/rdog-recording-window-geometry-policy.md`: Participating Window 与 Window Geometry Precondition。
- `specs/rdog-flow-control-plan.md`: `rdog.flow.v1` Replay Script 形态。
- `specs/rdog-window-control-plan.md`: `@window-resize` 与窗口状态契约。
- `specs/rdog-ax-screenshot-manifest-control-plan.md`: screenshot 与 AX manifest 契约。
- `specs/rdog-multi-display-screenshot-coordinate-plan.md`: 多显示器截图坐标。
- `specs/rdog-display-scope-control-plan.md`: display scope resolver。
- `specs/rdog-display-aware-control-chain-plan.md`: display / window / AX / verify 控制链。
- `specs/control-line-protocol.md`: line-control frame 形态与解析。

## Open questions

无。本规格已包含 ticket `#9` question 列出的所有边界。
