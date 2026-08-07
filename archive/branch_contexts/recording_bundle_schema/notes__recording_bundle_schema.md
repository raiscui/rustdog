# Recording Bundle schema 与原子导出笔记

## [2026-07-25 21:45:10] [Session ID: omx-1784512435044-92wxat] 笔记: Canonical Bundle 物理形态

### 已验证事实

- `specs/rdog-recording-session-lifecycle.md` 已固定 stop 顺序:冻结并排空 Journal,从 frozen Journal 编译 Replay Script,在 staging 路径组装并验证 Bundle,再通过同文件系统 atomic rename 提交。
- `completed` 只表示 Bundle 已原子提交。Commit 前失败不得暴露 partial Bundle;commit 后 delivery 失败不得回滚 `completed`。
- Completed recording 的重复 stop 只是既有 Bundle 的只读 delivery retry,不得重新编译,并且必须返回相同 Bundle checksum。
- Recording Journal 是 append-only canonical source。Replay Script、summary 与 evidence 都是派生或辅助资产。截图、AX snapshot 等大对象只通过 artifact reference 关联。
- Sensitive、unknown 与 paste 的真实输入值不得写入 Journal、Replay Script、manifest、evidence 或其他持久化 Bundle 资产。
- 当前 `SaveFileFrame` 只有 `request_id`、`filename`、`mime`、`encoding`、`data` 和可选图片元数据。Payload 必须是完整 base64,没有 chunk、offset、file hash 或总 frame 数字段。
- 当前接收端按 frame 到达顺序立即保存文件,最后等 `@response` 收口。`sanitize_filename` 只保留 basename,目录层级会丢失;同名文件会自动生成 `name-2.ext` 一类新名字。
- 因此,当前多个 `@savefile` 不能安全表达一棵有路径、固定文件名和整体完整性门禁的 Bundle 目录树。
- `Cargo.toml` 与 `Cargo.lock` 当前没有直接的 `zip`、`tar` 或 `zstd` 依赖。这只说明格式尚未选定,不是反对归档文件的产品约束。

### 当前候选决策

- 推荐首版把 canonical committed Bundle 定义为一个确定性归档文件。
- 归档内部逻辑树至少包含 `manifest.json`、`journal.jsonl`、`flow.json` 和 `evidence/`。
- 这样能复用单个 `@savefile`,保留内部路径,把 checksum、atomic rename、completed stop retry 和远程 delivery 都绑定到同一个不可变对象。
- 本地若额外保留展开目录,只能是可删除 cache 或 staging,不能成为第二真相源。

### 最强备选解释

- 备选方案是 canonical Bundle 目录 + 多个 `@savefile` 逐文件交付。
- 该方案若要保持 manifest path、固定文件名和整体完整性,必须先扩展 `@savefile` 为多文件事务协议。这与首版“不增加 chunk/事务协议”的约束冲突。

### 待 Human 确认

- 首版 canonical Bundle 是否采用单一确定性归档文件,而不是 canonical 目录配合多个 `@savefile` 逐文件交付。

## [2026-07-26 15:12:07] [Session ID: omx-1784512435044-92wxat] 决策: Canonical Bundle物理形态已确认

- Human 确认首版 canonical committed Bundle 是单一确定性归档文件。
- 归档内部继续表达版本化逻辑目录树。单个归档文件是 checksum、atomic commit、completed stop retry 与远程 delivery 的共同对象。
- 展开目录只允许作为 staging 或可删除 cache,不得成为第二真相源。
- 首版不扩展 `@savefile` 为多文件事务协议。
- 下一项只决定归档容器格式与扩展名,不在同一问题中混入 manifest schema 或 size limit。

## [2026-07-26 20:34:54] [Session ID: omx-1784512435044-92wxat] 决策: TAR容器与文件名已确认

- Human 确认首版使用不压缩的 POSIX TAR 归档。
- Canonical 文件名为 `<recording_id>.rdogrec.tar`。
- 首版不使用 ZIP、gzip、zstd 或自定义二进制容器。
- 不压缩避免压缩器版本或参数影响 Bundle checksum。截图等已压缩 evidence 不做二次压缩。
- TAR header 归一化、entry ordering 与 checksum 细节仍需后续单独固定,不能仅凭“使用 TAR”声称字节级确定性。
- 下一项只固定 required/optional entries,不同时决定 manifest 字段。

## [2026-07-26 21:17:30] [Session ID: omx-1784512435044-92wxat] 决策: Bundle最小文件集合已确认

- Human 确认首版只有 `manifest.json`、`journal.jsonl`、`flow.json` 三个必需文件。
- Evidence 按需保存为 `evidence/<artifact_id>.<ext>`,没有 evidence 时不创建空目录。
- 每个 evidence 必须同时有 canonical Journal artifact reference,并登记到 `manifest.json` 文件清单。
- 首版不增加 `summary.json`、`evidence/index.json`、运行日志或连续视频文件。
- `flow.json` 对 completed Bundle 必需,因为 lifecycle 已规定编译失败发生在 commit 前,不得产生 completed Bundle。
- 下一项只固定Bundle schema identity与reader compatibility,不同时展开manifest全部业务字段。

## [2026-07-27 12:27:03] [Session ID: omx-1784512435044-92wxat] 决策: Bundle schema与兼容规则已确认

- Human 确认 `manifest.json` 使用 `schema: "rdog.recording.bundle.v1"`。
- Manifest 必须声明 `archive_format: "posix-tar"`、`journal_schema: "rdog.recording.v1"` 和 `flow_schema: "rdog.flow.v1"`,并与实际内容一致。
- v1 只做 additive evolution。Reader 可以忽略未知可选字段,但不得忽略缺失必需字段、类型错误或声明与内容不一致。
- 不支持的 major version 必须 fail closed。破坏性变更才升级 major version。
- 首版不增加独立的 major/minor 数字字段,避免与 canonical schema 字符串形成重复版本源。
- 下一项只决定 recording identity 与生命周期时间字段,compiler provenance 和文件哈希后续分开确认。

## [2026-07-27 12:29:21] [Session ID: omx-1784512435044-92wxat] 笔记: Identity与时间字段现有来源

- `recording_id` 已由 daemon 在 required lanes ready 后生成,是 Recording Session identity。Journal 每条 envelope 都携带它,且同一 Journal 必须一致。
- Canonical wall-clock 只存在于 `session_start.payload.started_at_unix_ms`。其余事件时间由 monotonic delta 推导,不能成为 ordering 或 dedupe key。
- `session_terminal:frozen` 只结束 capture,不代表 Bundle committed。Lifecycle 的 `completed` 发生在 atomic rename 之后,且不得回写 Journal。
- 因此 manifest 可以复制 `recording_id` 与 `started_at_unix_ms` 作为轻量索引,但 validator 必须要求它们与 Journal 完全一致。
- 不应把 `completed_at` 写入归档:manifest 必须在 atomic rename 前完成,归档内部无法诚实记录 rename 已经发生。
- `duration_ms` 和 stop wall-clock 都能从 Journal monotonic timeline 派生,首版无需复制到 manifest。

## [2026-07-27 12:33:14] [Session ID: omx-1784512435044-92wxat] 决策: Manifest最小Session索引已确认

- Human 在推荐方案后回复"继续",确认 manifest 只复制 `recording_id` 与 `started_at_unix_ms`。
- 两个字段必须与 `journal.jsonl` 完全一致,且 `recording_id` 必须与归档文件名 identity 一致。
- Manifest 不写 `stopped_at`、`completed_at` 或 `duration_ms`。
- 结束时间与 duration 从 Journal monotonic timeline 派生。Bundle commit 时间属于 lifecycle 元数据,不进入归档。

## [2026-07-27 12:33:14] [Session ID: omx-1784512435044-92wxat] 笔记: Compiler provenance现状

- 当前 Cargo package version 是 `3.0.0`,可以作为 producer version 的构建期单一来源。
- 仓库当前未发现 `GIT_COMMIT`、`build_commit` 或 compiler version 注入链路。
- 首版若强制 Git commit provenance,就必须新增构建脚本或环境变量链路,超出本ticket的简化边界。
- 推荐分别记录 rdog package version 与 Replay compiler policy version。后者在相同Journal可能产生不同flow时递增。

## [2026-07-27 14:02:28] [Session ID: omx-1784512435044-92wxat] 决策: 最小producer/compiler provenance已确认

- Human 确认 `producer` 固定包含 `name: "rdog"` 与 Cargo package `version`。
- Human 确认 `compiler` 固定包含 `name: "rdog-replay-compiler"` 与独立的整数版本字符串,首版为 `"1"`。
- 当相同 canonical Journal 可能因 compiler 或 promotion policy 改动而生成不同 `flow.json` 时,必须递增 compiler version。
- Provenance 只用于审计和问题定位,不替代Bundle、Journal或flow schema compatibility gate。
- 首版不记录Git commit、build timestamp、hostname、用户名、绝对路径或target triple。
- 下一项只决定per-file hash与whole-archive checksum边界。

## [2026-07-27 20:12:06] [Session ID: omx-1784512435044-92wxat] 决策: 两层SHA-256完整性模型已确认

- Human 确认 `manifest.json.files` 登记除 manifest 自身以外的全部 regular files。
- 每项固定包含 `path`、`role`、`media_type`、`size_bytes` 与64位小写十六进制 `sha256`,并按 path 升序排列。
- `manifest.json` 不记录自身哈希,避免self-hash递归。
- TAR完成后计算exact archive bytes的`bundle_sha256`与`bundle_size_bytes`;二者保存于lifecycle metadata并通过最终`@response`返回,不写回归档。
- Completed stop retry必须重放既有归档,并返回相同的bundle hash与size。
- 缺件、未登记额外文件、重复path、size mismatch或hash mismatch都必须fail closed。
- 下一项只固定TAR entry顺序与header normalization,内部JSON canonical bytes后续单独确认。

## [2026-07-28 00:05:23] [Session ID: omx-1784512435044-92wxat] 决策: Deterministic USTAR规则已确认

- Human 确认首版只使用POSIX USTAR regular file entries,不使用PAX/GNU扩展、目录、链接、ACL、xattr或设备节点。
- Entry顺序固定为`manifest.json`、`journal.jsonl`、`flow.json`,随后是按path升序排列的evidence。
- Archive path只允许受控相对ASCII路径;绝对路径、反斜杠、`.`、`..`、空segment与duplicate path必须拒绝。
- Header固定`mode=0644`、`uid=0`、`gid=0`、`mtime=0`、空`uname/gname`;未使用字段与file padding清零。
- Archive以两个512-byte零block结束,不允许trailing bytes。
- 这保证相同内部文件字节生成相同TAR bytes;内部JSON如何生成仍由下一项决定。

## [2026-07-28 00:05:23] [Session ID: omx-1784512435044-92wxat] 笔记: 现有JSON canonical边界

- Journal规格已固定UTF-8 JSON Lines、LF、无BOM、每行一个object,且object key order没有语义。
- `journal.jsonl` 应保留frozen Journal原始字节,不能为了Bundle重新parse/reserialize。
- 仓库已有`canonical_json` helper:scalar使用`serde_json`表示,arrays保序,objects按key递归排序并输出compact JSON。
- `flow.json`当前没有正式的canonical on-disk encoding规格。Bundle规格需要补齐,否则相同结构可能因key order或pretty printing产生不同hash。

## [2026-07-28 07:12:46] [Session ID: omx-1784512435044-92wxat] 决策: Canonical JSON bytes已确认

- Human 确认 `journal.jsonl` byte-for-byte复制frozen Journal,不重新parse或serialize。
- `manifest.json`与`flow.json`固定UTF-8、无BOM、compact JSON、递归object key排序,并以恰好一个LF结束。
- 有业务顺序的array保持原顺序;表示集合的array必须先按schema指定stable key排序。
- Scalar使用`serde_json`稳定表示,拒绝NaN与Infinity等非JSON数值。
- Evidence保留捕获或生成完成后的原始字节,Bundle finalizer不重新编码。
- 首版不引入RFC 8785或新序列化依赖。

## [2026-07-28 07:12:46] [Session ID: omx-1784512435044-92wxat] 笔记: Evidence现有协议入口

- `@record-start.default_mark_evidence`与`@record-mark.evidence`当前只允许`screenshot`和`ax_snapshot`。
- Screen Recording权限只在显式请求screenshot evidence时需要。Optional evidence失败不回滚mark或Recording Session。
- Journal只内嵌evidence status/ref summary;截图和AX snapshot等大对象只保存artifact reference。
- 首版Bundle evidence allowlist应复用这两个既有类型,不能通过Bundle规格暗中增加video、raw event dump或运行日志。

## [2026-07-28 07:12:46] [Session ID: omx-1784512435044-92wxat] 笔记: Evidence持久化安全门禁

- Redaction规格明确禁止sensitive/unknown真实值及其长度、hash、prefix、suffix、字符类别等派生特征进入任何artifact。
- Screenshot与AX snapshot都可能泄露值或长度。首版不做局部打码推断;捕获点处于active redaction interval,或focused target分类为sensitive/unknown时,应抑制整份evidence artifact。
- 被抑制或采集失败的optional evidence只在Journal记录status与非敏感reason,不得生成placeholder file。
- 正常evidence也必须来自显式`default_mark_evidence`或`@record-mark.evidence`,不能默认持续采集。
- 推荐manifest evidence roles只允许`screenshot_image`、`screenshot_manifest`与`ax_snapshot`;其他role在Bundle validation阶段拒绝。

## [2026-07-28 08:34:27] [Session ID: omx-1784512435044-92wxat] 决策: Evidence allowlist与持久化门禁已确认

- Human 确认首版roles只允许`screenshot_image`、`screenshot_manifest`和`ax_snapshot`。
- Evidence必须来自显式start default或record mark请求,不允许默认连续后台采集。
- Successful Journal artifact reference必须唯一解析到Bundle文件;failed、blocked或redacted evidence不生成placeholder。
- Active redaction interval或sensitive/unknown focused target下,整份screenshot/AX snapshot均不得持久化。
- Video、音频、运行日志、trace、raw event dump与未登记artifact均不允许进入Bundle。

## [2026-07-28 08:34:27] [Session ID: omx-1784512435044-92wxat] 笔记: Redaction summary边界

- Journal terminal要求`redaction_summary`,但现有规格没有固定其内部字段形状。
- Redaction规格另行要求发生paste parameter时,Bundle manifest声明`runtime_clipboard_exposure:true`。
- Manifest summary应仅保存从Journal和flow可精确重算的非敏感聚合值,validator要求exact match。
- 不应复制parameter id、descriptor、target、reason明细或分类分组;这些已经由Journal/flow作为各自真相源承载。
- 推荐最小字段为`segment_count`、`required_parameter_count`、`suppressed_evidence_count`和`runtime_clipboard_exposure`。

## [2026-07-28 08:56:19] [Session ID: omx-1784512435044-92wxat] 决策: Manifest redaction summary已确认

- Human 确认`redaction_summary`只包含`segment_count`、`required_parameter_count`、`suppressed_evidence_count`与`runtime_clipboard_exposure`。
- 所有计数必须为非负整数,并能分别从Journal、flow与evidence statuses精确重算。
- Paste parameter或clipboard mode存在时,`runtime_clipboard_exposure`必须为true。
- Summary不复制parameter id、descriptor、target、classification/reason明细或任何value派生特征。
- Summary mismatch必须在Bundle commit前fail closed。

## [2026-07-28 08:56:19] [Session ID: omx-1784512435044-92wxat] 笔记: Warning语义候选

- 现有Recording规格没有通用manifest warnings schema。
- Warning只能表达仍满足completed Bundle全部不变量的非致命退化,不能把fatal validation降级。
- 推荐warnings固定为按code排序的`{code,count}`集合,不允许自由文本message、path、target或backend error原文。
- 首版可验证的非致命codes只有`optional_evidence_failed`与`guarded_coordinate_fallback_used`。
- Unknown additive warning code应被reader接受并原样展示;warning本身不改变Replay policy或compatibility gate。
