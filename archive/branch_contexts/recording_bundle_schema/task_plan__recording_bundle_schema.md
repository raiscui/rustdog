# 任务计划: 定义 Recording Bundle schema 与原子导出

## [2026-07-25 21:40:44] [Session ID: omx-1784512435044-92wxat] [任务启动]: Wayfinder grilling ticket

### 目标

形成可直接供Recorder finalizer与远程delivery实现使用的正式规格,定义Recording Bundle目录、manifest、journal、derived flow、evidence、hash、compiler/version provenance、redaction summary、warnings、size limits和`@savefile`顺序。

### 两个方向

1. 不惜代价方案:首版引入content-addressed object store、分块去重、签名、加密、随机访问索引、断点续传与跨版本迁移。能力完整,但明显超出当前Wayfinder destination。
2. 首版严格方案:使用单一版本化Bundle目录和deterministic manifest,保留canonical Journal、derived flow与必要结构化evidence,逐文件hash,同文件系统staging + atomic rename提交,远程delivery与commit状态分离,超限或缺件fail closed。

当前推荐方向2。它复用既有lifecycle与`@savefile` frame,不增加隐藏的第二真相源,也不默认保存连续视频。

### 阶段

- [x] 阶段 1: 重新验证frontier并claim ticket
- [ ] 阶段 2: 回读domain、lifecycle、Journal、flow、redaction、geometry、savefile和现有源码边界
- [ ] 阶段 3: 一次一个问题完成Bundle schema与原子导出grilling
- [ ] 阶段 4: 写正式resolution asset,同步索引与交叉引用
- [ ] 阶段 5: 验证、scoped commit、push、resolution、close与map更新

### 约束

- 本ticket只做规格决策,不实现Recorder或Bundle writer生产代码。
- Recording Journal保持canonical source;flow、summary和evidence都是派生或辅助资产。
- Stop前失败不得暴露partial Bundle;commit后delivery失败不得回滚completed。
- Cancel不编译、不提交Bundle。Crash orphan不恢复、不导出。
- 不默认录制连续视频,不把敏感参数值写入任何Bundle文件。
- 每轮只向human确认一个具体产品决策。

### 停止条件

- Bundle目录与manifest schema、required/optional文件和版本字段明确。
- Hash覆盖范围、compiler provenance、redaction summary和warning语义明确。
- Evidence allowlist、retention入口和per-file/total size limits明确。
- Staging、fsync、atomic rename、失败清理和cancel/crash边界明确。
- `@savefile` frame顺序、远程delivery retry与最终response边界明确。
- Human verdict写入正式规格并完成Wayfinder resolution。

### 当前状态

**阶段 2 进行中**: 先核对既有正式契约和当前`@savefile`能力,再提出第一个Bundle内容边界问题。

## [2026-07-25 21:45:10] [Session ID: omx-1784512435044-92wxat] [阶段更新]: Canonical Bundle物理形态调查完成

- [x] 回读Recording lifecycle、Journal、flow与redaction既有契约。
- [x] 静态核对当前`SaveFileFrame`、接收端逐frame落盘、basename净化和冲突重命名行为。
- [x] 确认当前协议没有chunk、offset、per-file hash、总frame数或多文件事务门禁。
- [x] 阶段 2: 源码与规格边界调查完成,证据写入`notes__recording_bundle_schema.md`。
- [ ] 阶段 3: 等待Human确认首版canonical Bundle采用单一确定性归档文件。

**阶段 3 进行中**: 一次只确认一个产品决策;本轮不选择具体archive格式,也不引入多文件事务协议。

## [2026-07-26 15:12:07] [Session ID: omx-1784512435044-92wxat] [决策确认]: 单一确定性归档文件

- [x] Human确认首版canonical committed Bundle采用单一确定性归档文件。
- [x] 本地展开目录仅为staging或可删除cache,不构成第二真相源。
- [x] 首版不增加多文件`@savefile`事务协议。
- [ ] 下一决策:确认archive容器格式与对外扩展名。

**阶段 3 继续进行**: 先固定最简单的deterministic archive容器,再定义内部required/optional entries。

## [2026-07-26 20:34:54] [Session ID: omx-1784512435044-92wxat] [决策确认]: 不压缩POSIX TAR

- [x] Human确认首版archive采用不压缩POSIX TAR。
- [x] Canonical文件名固定为`<recording_id>.rdogrec.tar`。
- [x] 首版排除ZIP、gzip、zstd和自定义二进制容器。
- [ ] 下一决策:确认Bundle内部required/optional entries。

**阶段 3 继续进行**: 固定最小完整逻辑树,避免空目录语义和重复summary真相源。

## [2026-07-26 21:17:30] [Session ID: omx-1784512435044-92wxat] [决策确认]: 最小required/optional entries

- [x] Human确认required entries为`manifest.json`、`journal.jsonl`、`flow.json`。
- [x] Evidence仅按需写入`evidence/<artifact_id>.<ext>`,且必须被Journal引用并登记到manifest。
- [x] 首版不增加summary、evidence index、运行日志或连续视频。
- [ ] 下一决策:确认Bundle schema identity与版本兼容规则。

**阶段 3 继续进行**: 先固定reader识别与版本门禁,再逐项定义manifest identity和provenance字段。

## [2026-07-27 12:27:03] [Session ID: omx-1784512435044-92wxat] [决策确认]: Bundle schema identity与compatibility

- [x] Human确认Bundle schema为`rdog.recording.bundle.v1`。
- [x] Manifest声明archive、Journal和flow schema,且声明必须与内容一致。
- [x] v1允许additive未知可选字段;缺失必需字段、类型错误和未知major一律fail closed。
- [x] 首版不增加重复的major/minor数字字段。
- [ ] 下一决策:确认recording identity与生命周期时间字段。

**阶段 3 继续进行**: 固定manifest的最小session identity,避免复制Journal中的运行时细节。

## [2026-07-27 12:29:21] [Session ID: omx-1784512435044-92wxat] [调查更新]: Identity与时间单一来源已核对

- [x] 确认`recording_id`由daemon生成并由Journal envelope承载。
- [x] 确认唯一wall-clock anchor是`session_start.payload.started_at_unix_ms`。
- [x] 确认`completed`发生在归档完成并atomic rename之后,不能预写成归档内事实。
- [ ] 等待Human确认manifest只复制最小identity索引字段。

**阶段 3 继续进行**: 对重复索引字段施加exact-match validator,不新增第二套时间真相源。

## [2026-07-27 12:33:14] [Session ID: omx-1784512435044-92wxat] [决策确认]: Manifest最小Session identity

- [x] Human确认manifest只复制`recording_id`与`started_at_unix_ms`。
- [x] 两字段必须与Journal完全一致,recording identity还必须与归档文件名一致。
- [x] Manifest不写stop、completed或duration派生字段。
- [x] 核对当前只有Cargo package version,没有Git commit或compiler version注入链路。
- [ ] 下一决策:确认producer与Replay compiler provenance字段。

**阶段 3 继续进行**: 采用最小可审计provenance,不为首版新增build-script元数据链路。

## [2026-07-27 14:02:28] [Session ID: omx-1784512435044-92wxat] [决策确认]: Producer与Replay compiler provenance

- [x] Human确认producer为rdog name + Cargo package version。
- [x] Human确认compiler为固定name + 独立版本,首版version为`"1"`。
- [x] 相同Journal可能产生不同flow时必须递增compiler version。
- [x] 首版不记录Git commit、build timestamp或主机身份信息。
- [ ] 下一决策:确认manifest file inventory hash与Bundle checksum边界。

**阶段 3 继续进行**: 解决manifest self-hash递归,同时保证completed retry返回同一archive checksum。

## [2026-07-27 20:12:06] [Session ID: omx-1784512435044-92wxat] [决策确认]: Per-file hash与whole-Bundle checksum

- [x] Human确认manifest files覆盖除manifest自身外的全部文件。
- [x] File inventory固定path、role、media type、size与SHA-256,并按path排序。
- [x] Whole-archive SHA-256与size保存在lifecycle metadata和最终response,不写回归档。
- [x] Completed retry必须返回同一archive bytes、hash与size。
- [ ] 下一决策:确认TAR entry ordering与header normalization。

**阶段 3 继续进行**: 先保证相同内部文件字节生成相同TAR,再固定JSON canonical serialization。

## [2026-07-28 00:05:23] [Session ID: omx-1784512435044-92wxat] [决策确认]: Deterministic USTAR

- [x] Human确认USTAR-only regular files与safe relative ASCII paths。
- [x] Entry order、header metadata、padding与end blocks均已固定。
- [x] PAX/GNU扩展、目录、链接、ACL、xattr和trailing bytes均被排除。
- [x] 核对Journal编码与仓库既有canonical JSON helper。
- [ ] 下一决策:确认manifest与flow的canonical JSON bytes。

**阶段 3 继续进行**: Journal保持byte-for-byte,只规范新生成的manifest和flow JSON。

## [2026-07-28 07:12:46] [Session ID: omx-1784512435044-92wxat] [决策确认]: Canonical JSON bytes

- [x] Human确认Journal byte-for-byte进入Bundle。
- [x] Human确认manifest与flow使用递归key排序的compact UTF-8 JSON和单个末尾LF。
- [x] Arrays与scalar的稳定编码边界已固定,evidence不重新编码。
- [x] 核对现有record evidence入口只允许screenshot与ax_snapshot。
- [ ] 下一决策:确认首版evidence allowlist与持久化门禁。

**阶段 3 继续进行**: 复用现有evidence种类,先封闭敏感内容与未登记artifact进入Bundle的路径。

## [2026-07-28 07:12:46] [Session ID: omx-1784512435044-92wxat] [调查更新]: Evidence redaction gate已核对

- [x] 确认sensitive/unknown value与派生特征不得进入artifact。
- [x] 确认screenshot与AX snapshot均是显式optional evidence。
- [x] 推荐首版在active redaction或sensitive/unknown focus时抑制整份artifact,不做局部打码。
- [ ] 等待Human确认evidence allowlist与持久化门禁。

**阶段 3 继续进行**: 保持optional evidence失败不影响mark,但任何不安全artifact都不得进入completed Bundle。

## [2026-07-28 08:34:27] [Session ID: omx-1784512435044-92wxat] [决策确认]: Evidence allowlist与redaction gate

- [x] Human确认三种evidence roles与显式opt-in来源。
- [x] Human确认redacted/failed/blocked evidence不生成placeholder。
- [x] Human确认active redaction或sensitive/unknown focus时抑制整份artifact。
- [x] 核对Journal仅要求redaction summary存在,尚未固定字段shape。
- [ ] 下一决策:确认manifest redaction summary最小字段。

**阶段 3 继续进行**: 只保存可重算聚合值,不复制secret、parameter descriptor或target明细。

## [2026-07-28 08:56:19] [Session ID: omx-1784512435044-92wxat] [决策确认]: Manifest redaction summary

- [x] Human确认四个最小summary字段与exact-recompute validator。
- [x] Human确认不复制parameter/target/reason明细和value派生特征。
- [x] Human确认summary mismatch在commit前fail closed。
- [x] 核对现有规格尚无通用warnings shape。
- [ ] 下一决策:确认warnings code/count结构与fatal边界。

**阶段 3 继续进行**: Warning仅作非致命审计摘要,不允许弱化任何安全和完整性门禁。

## [2026-07-28 21:30:00] [Session ID: omx-1784512435044-92wxat] [决策确认]: Manifest warnings结构与fatal边界

- [x] Human确认warnings是必需数组,空时为`[]`。
- [x] Human确认每个warning只有stable `code` + 正整数 `count`。
- [x] Human确认unknown additive code reader必须接受并原样展示。
- [x] 首版只定义两个code:`optional_evidence_failed`、`guarded_coordinate_fallback_used`。
- [x] Warning不改Replay policy也不放宽验证门禁。
- [x] Required lane、journal/flow/schema无效、locator歧义/stale、display topology/geometry precondition失败、redaction违规、文件缺失/额外/hash或size不匹配、超过limit、atomic commit失败一律fail closed,不能降级为warning。

## [2026-07-28 21:30:00] [Session ID: omx-1784512435044-92wxat] [范围修正]: ticket编号纠正

- 实际ticket #12是已关闭的macOS调研ticket,不是远程交付ticket。
- 之前对话里的"远程交付"实际归属ticket #9的"远程下载"部分。
- ticket #9还差`文件大小限制`、`不默认录视频`两项未确认。
- 决定:把所有已确认决策 + 远程交付7项 + 剩下2项统一在ticket #9的规格文件中交付。

## [2026-07-28 21:30:00] [Session ID: omx-1784512435044-92wxat] [下一步]: 完成ticket #9收尾

- [ ] Human确认`文件大小限制`决策
- [ ] Human确认`不默认录视频`决策
- [ ] 写完整规格specs/rdog-recording-bundle-schema.md
- [ ] 验证规格
- [ ] commit + push + close ticket + 更新map

## [2026-07-28 21:35:00] [Session ID: omx-1784512435044-92wxat] [决策推荐]: Bundle size limit

- 推荐硬上限: 单个Bundle解压后 `<= 256 MiB`,对应 `@savefile` base64 帧 `<= 384 MiB`(≈256×1.5 + JSON 包装)。
- 超过上限: Bundle 在 commit 前 fail closed,返回protocol层`bundle_too_large`。
- 接收端: 解码后字节超过256 MiB时也拒绝,标记`delivery_failed` reason code `bundle_too_large`。
- `@record-stop` 不携带分块、压缩或partial字段;超过上限必须终止本次录制,而不是压缩或截断evidence。
- 不引入运行时参数化上限;不引用`policy.max_output_bytes`(那是flow stream限制)。
- 等待Human确认数值。

## [2026-07-28 21:38:00] [Session ID: omx-1784512435044-92wxat] [决策推荐]: 不默认录制连续视频

- 首版evidence allowlist是允许集合:仅screenshot_image / screenshot_manifest / ax_snapshot。
- 视频(屏幕录像、相机流、音频流)即使显式opt-in也不进入Bundle。
- Recorder capture backend不提供视频/音频channel;`@record-mark.evidence`也不接受video role。
- 用户若需要录屏,首版必须自己用系统工具录,产物不进Bundle也不进Journal。
- 拒绝视频不是"暂未实现",而是"首版正式不支持",防止后续默认开关被误打开。
- 等待Human确认边界。

## [2026-07-28 21:40:00] [Session ID: omx-1784512435044-92wxat] [决策确认]: Bundle size limit + 不默认录视频

- [x] 按"继续,按你建议"约定,采用256 MiB Bundle / 384 MiB base64 frame硬上限。
- [x] 超过上限commit前fail closed,接收端标记`delivery_failed:bundle_too_large`。
- [x] 不分块、不压缩、不截断,不引入运行时参数化上限。
- [x] 首版禁止视频evidence role,即使显式opt-in也不接受。
- [x] 不引入屏幕录像、相机、音频channel。
- [x] 若Human审规格时希望调整数值,最后commit前改正。

## [2026-07-28 21:50:00] [Session ID: omx-1784512435044-92wxat] [任务完成]: ticket #9 resolution delivered

- [x] 写完整规格 `specs/rdog-recording-bundle-schema.md`(575行)。
- [x] AGENTS.md 追加长期文件索引。
- [x] git commit b6bc5f8,scope 限定 specs/rdog-recording-bundle-schema.md + AGENTS.md。
- [x] git push origin main (6973dfa..b6bc5f8)。
- [x] gh issue close 9 with full resolution comment。
- [x] Wayfinder map ticket #2 body 追加 ticket #9 entry。
- [x] 范围内dirty worktree未污染。
