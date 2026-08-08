## [2026-08-05 22:58:24] [Session ID: omx-1785926019233-oohizd] 笔记: native screenshot capture tracing 诊断

### 现象

- macOS `capture_with_timeout` 会在 native SCK 或 xcap 调用长期不返回时,以 `TimedOut` 结束控制面等待,并用单 worker gate 避免无限创建线程。
- 现有路径没有结构化事件。daemon 日志无法区分 SCK 超时、正常 fallback、fallback 失败或 Screen Recording 权限拒绝。

### 静态证据

- `capture_primary_display_image` 与 `capture_all_display_images` 都先执行 Screen Recording preflight,再走 SCK,最后按非权限错误进入 xcap。
- `capture_with_timeout` 是唯一的 native deadline 与 in-flight gate 边界。它知道 backend、timeout 和 timeout 原因,但不知道请求是 primary 还是 all-display。
- `classify_capture_error` 已保证任一 backend 的 `PermissionDenied` 会覆盖为最终权限错误。权限不应继续 fallback。
- `Cargo.toml` 只有 `log` / `fern`,没有能输出 structured fields 的 tracing subscriber。

### 当前设计

- 新增 `tracing` 与 `tracing-subscriber`,让新增事件沿用 `RDOG_LOG_LEVEL` 和已有 stderr/hidden-file 目标,不迁移既有 `log` 调用。
- 共享 SCK -> xcap policy 负责 `fallback` 与终态事件。timeout helper 负责 timeout 原因,因为只有它能识别 worker deadline 和 in-flight gate。
- 权限拒绝是终态类别,用 `screenshot_capture_permission_denied` 代替泛化 `screenshot_capture_failed`,避免同一请求重复记录两个终态错误。

### 反证与边界

- 备选方案是在 `map_capture_error` 直接记录。该函数没有 capture kind 或 fallback 上下文,会把同一次失败拆成无关联的重复日志,因此不采用。
- 备选方案是只继续用 `log::warn!` 拼接文本。这无法按字段筛选 SCK timeout、fallback 与权限,不满足此次可观测性目标。

### 外部 API 证据

- `cargo info tracing@0.1.44`: 当前 crate 提供 application-level tracing。
- `cargo info tracing-subscriber@0.3.23`: `fmt` subscriber 可输出 events。源码 `fmt::writer::BoxMakeWriter` 支持运行时选择 stderr 或 file writer。

## [2026-08-06 15:06:56] [Session ID: omx-1785926019233-oohizd] 笔记: 全模型 macOS ops 与兼容性归因

### 动态证据

- 新 DeepSeek artifact: `/tmp/pi-rdog-macos-ops-deepseek-20260806-145902/suite-result.json`。`runCount:8`、`successCount:8`,8 个 case 均为 attempt 1 success。
- 该 suite 记录的 canonical skill 是仓库内 `.codex/skills/rdog-control/SKILL.md`,SHA-256 为 `129aa820edbedaed787d7dd9397c9b69ffeaf74140edbc19c3031207dc97f5d2`。
- 其余四个有效 suite 也均为 8/8。MiniMax-M3 与 MiniMax-M2.7-highspeed 的 `safari-new-tab-navigate` 为 attempt 2 success,其余 case 均首次成功。

### 可恢复错误审计

- 五个 suite 共记录多类非致命 `code:64`。所有 case 最终都有 real rdog call、fresh AX/window/URL verification 与 expected result,因此没有失败样本可归因为 rdog 兼容缺口。
- `@window-find:Calendar`、`@window-find:Terminal`、`@window-find:TextEdit` 合计出现 3 次。静态代码显示 `parse_compact_fields` 已把无前缀 token 放入 positional,而 `parse_window_find_payload` 只消费 `app:` 与 `pid:` named field,随后报多余字段。
- 最强备选解释是 canonical skill 已足以让模型快速改用对象请求。新 DeepSeek JSONL 支持该解释: `@window-find:Calendar` 报错后改用 `@window-find:{app:"Calendar"}` 并成功。

### 决策

- 不在本轮为上述自愈错误扩展 parser,避免改变 canonical skill hash 后重跑五模型完整矩阵。
- `@window-find:APP` 作为低风险候选记录到 `LATER_PLANS.md`;若后续目标是降低每 case 的 recoverable protocol error,应在 parser 消费唯一 positional app 后添加回归测试,再完整重跑五模型矩阵。

## [2026-08-07 00:19:24] [Session ID: omx-1786061963768-e7in9l] 笔记: macOS ops 交互步数优化工作流

### 已确认的工作流目标

- 主指标是每个 macOS ops 用例中的 agent 决策点与 rdog control 交互次数。
- 真实 rdog 调用和新鲜 AX/window/URL 验证仍是成功成立的前提,不能为了减少步数而删除证据链。
- 全部 8-case suite 的总运行时间不是当前主指标,只可作为后续观察项。
- 第一版同时覆盖现有 8 个用例的基线优化,以及 runner 对未来用例的低交互路径约束。
- 统计所有 agent 发出的 `rdog control` 请求,并将每个请求分类为必要证据或可消除开销;每个成功仍必须保留动作后的新鲜验证。
- 禁止在 canonical skill 或 runner 中为特定 app、特定操作写固定序列。优化必须改善 rdog 的通用兼容性和控制难度,不能以局部 case 的技巧替代通用能力。
- 遇到高交互轨迹时,先用真实控制记录确认通用摩擦点;优先复用或改进 rdog 共享 parser、协议或高密度 primitive。只有协议已足以表达意图时,才缩短 canonical skill 的通用决策路径。runner 只能测量和门禁,不能补偿协议缺口。
- 开发阶段允许用定向样本诊断,但每次优化的最终认证必须重跑全部 5 个活动模型 × 8 个 case。全矩阵必须保持成功和新鲜证据,总 `rdog control` 请求数下降,并单列可恢复协议错误。
- 矩阵总请求数下降时,个别 case 默认不得增加请求。唯一例外是新增请求被证明为不可替代的通用验证证据,并在比较报告中逐项说明。
- 优化证据的单一事实来源是自动从每个 case 的原始 JSONL 与 suite result 生成的机器可读 interaction ledger。ledger 按模型、case、attempt 和请求记录命令、分类、证据路径与协议错误;Markdown 只能从 ledger 生成摘要。
- 仅在共享 rdog control parser、协议、通用 primitive、canonical skill，或 macOS ops case 本身变更时触发完整 5 × 8 live matrix。普通无关改动不运行。
- 自动完成基线采集、轨迹分类和通用改动候选分析。任何会改变 rdog 协议行为或 canonical skill 控制策略的补丁之前,必须向用户提供一次决策 brief;确认后再自动实现和完整矩阵认证。

## [2026-08-07 00:19:24] [Session ID: omx-1786061963768-e7in9l] 笔记: interaction ledger 可用证据

### 静态证据

- 外部 runner 的每次尝试已保存 `pi-events.jsonl`、`pi-summary.json` 和 `run-result.json`。`pi-summary.json` 包含 agent 的 `toolCalls`、`toolResults`、`rdogCommands`、`rdogResponses` 与 `rdogResponseErrors`。
- runner 自己的 setup、before/after verification 也通过 `run_rdog()` 发送控制请求,但它们不属于 agent 发起的请求,必须从 ledger 主指标排除。
- 现有 summary 只收集含 `rdog control` 的 bash command 文本;为统计真正请求次数,ledger 实现必须从原始 tool call 确认每条 shell 命令中实际执行的 rdog invocation,而不能把 bash call 数直接当作请求数。

### 尚待决定

- 请求角色应仅按通用协议语义和相邻结果分类,未知项保持未知;禁止根据 app 名称、case id 或用户任务文本推断可消除性。
- ledger 仅根据通用协议 verb、错误响应以及请求前后顺序标记 `query`、`action`、`post_action_evidence`、`recovery` 或 `unknown`。`unknown` 不是冗余的同义词,不得自动删除或降权。
- 已认证 baseline ledger 必须不可变,并记录 rustdog commit、canonical skill SHA-256、runner/config/case 文件 hash、模型标识与运行时间。候选使用相同输入重跑完整矩阵,只有通过认证的候选才能成为新的 baseline。
- 补丁前的唯一 brief 必须包含候选共享摩擦点、静态代码位置、跨模型/case 的触发轨迹数、按角色分组的请求差额、协议错误差额、预计影响面、原始 ledger 链接和批准/拒绝/暂缓决定。
- 共享摩擦点必须同时具有共享 rdog/skill 层的静态代码证据,并在至少两个独立 `(模型, case)` 样本中重现同一通用意图或失败模式。单次样本只进入 ledger,不进入补丁候选。
- canonical skill 已规定每个 bash 调用以 `rdog control` 开始。ledger 应将违反该通用调用契约的样本判为不可计量,并使认证 fail-closed,而不是用 shell/app 特例猜测请求数量。
- 新增或语义变化的 macOS ops case 先独立建立 baseline,不得为全局请求数下降记功。未变化的 case 必须继续逐项对比旧 baseline。
- immutable baseline 存入 `../pi-rdog-calculator-eval/results/macos-ops-interaction/<baseline-id>/`,包含 ledger、comparison manifest、摘要和每个 case 的原始 JSONL/source artifact 副本,并随外部评测仓库提交。

## [2026-08-07 09:40:21] [Session ID: omx-1786061963768-e7in9l] 笔记: interaction ledger 动态验证修正

### 现象

- 完整 5 模型 x 8 case artifact 中出现普通 shell、`sleep ...; rdog control ...` 和一个工具调用前失败的 retry。
- 5 个 suite 共计 40 个成功 case、41 个 attempt、260 个 agent 决策、252 个 rdog 请求和 8 个 supporting shell。

### 上一假设不成立

- 上一条“每个 bash 调用必须以 `rdog control` 开头,否则不可计量”的假设不成立。
- 动态证据是归档 ledger 中 qwen3.7 的 1 个零成本失败 attempt,以及 8 个 supporting shell;若沿用旧规则会丢失真实成本或错误拒绝合法通用 shell 组合。

### 当前结论

- 所有 Pi bash tool call 是 `agentDecisionCount`;每个 bash 中唯一可识别的 `rdog control` invocation 是 `requestCount`。
- 无 control 的 bash 是 `supporting_shell`;多个 control invocation 或不可解析 shell 仍失败关闭。
- 规则只依赖 shell token、通用 verb、错误响应和调用顺序,未读取 app、case、prompt 或预期结果。

## [2026-08-07 10:00:19] [Session ID: omx-1786061963768-e7in9l] 笔记: baseline 候选的静态安全筛选

### 动态证据

- `@cmd` 裸 payload 的同类 `code:64` 在 5 个 `(model, case)` 样本出现 6 次;`@window-find:APP` 在 2 个样本出现 2 次。
- `@key.target` 在 4 个样本出现 4 次,`@ax-press.action` 在 2 个样本出现 3 次。

### 静态证据与结论

- `src/control_protocol.rs` 的 `cmd` 分支直接调用 `parse_quoted_payload`,但 `@cmd` 本身仍路由到既有 `ControlCommand::Script` shell lane。候选只处理 raw 单行文本,不引入新执行器。
- `src/control_window.rs::parse_window_find_payload` 的 compact 分支只消费 named `app` / `pid`,然后 `ensure_empty`。唯一 positional atom 可无歧义映射为 app;查询仍是只读。
- `src/control_protocol/parsers/key.rs` 将 targeted delivery 固定为显式 `delivery + pid/window_id`;不能把 heterogeneous `target` 猜成全局或定向输入。
- `src/control_ax.rs::parse_ax_press_payload` 固定构造 AXPress,而 `parse_ax_action_payload` 对其它 allowlist action 做独立验证。不能无提示改变 command 语义。

## [2026-08-07 10:10:51] [Session ID: omx-1786061963768-e7in9l] 笔记: parser 兼容实施的 raw payload 边界

### 静态证据
- `src/control_protocol.rs` 先以 `trim_end_matches(['\r', '\n'])` 保留正常输入行语义,但随后曾对拆出的 payload 执行全局 `trim()`。
- `parse_cmd_payload()` 已拒绝 `\r` / `\n`,但没有原始输入就无法区分前导换行与正常空白。

### 动态证据
- 新增前导换行断言后,`rtk cargo nextest run --package rustdog --bin rdog -E 'test(parse_should_accept_raw_single_line_cmd_and_reject_ambiguous_payloads)' --no-capture` 在未修复时失败。
- 将 `raw_payload` 只传给 `@cmd` 后,该测试通过。`@window-find:APP` 正向测试和 named/positional/multi-atom 拒绝测试也通过。

### 结论
- 已验证结论: 上游 payload trim 曾覆盖 raw `@cmd` 的单行校验,现已在共享分派入口修复。
- 正常文本行末尾的 `\n` 仍在 `parse_control_line()` 入口剥离,这符合“按行解析”约定;命令 payload 内或前导的物理换行继续拒绝。
- 改动不触及 shell executor、targeted key delivery、AX action 语义或特定 App 操作序列。

### 全量验证
- `cargo fmt -- --check`、`git diff --check` 通过。
- `cargo nextest run --package rustdog --bin rdog`: 685 passed, 1 skipped。
- `cargo build --package rustdog --bin rdog`: 0 errors, 17 warnings。warning 对应既有 cfg/dead-code 边界,不纳入本次共享 parser 修复。

## [2026-08-07 10:46:53] [Session ID: omx-1786061963768-e7in9l] 笔记: interaction ledger 的 heredoc 计量缺口

### 现象
- candidate archive 在 MiniMax Safari retry 中停止,错误是 `shlex` 的 `No closing quotation`。

### 验证
- 原始 Pi bash tool call 使用 `<<'EOF'` heredoc 写入 handoff 文本;body 包含普通 apostrophe 和 `rdog control` 字样。
- `bash -n` 对同一命令返回 0,Pi 的 tool result 不是 error。

### 结论
- 这是 ledger shell parser 的语法覆盖缺口。应泛化地跳过有明确 delimiter 的 heredoc body,而不是按 App、case 或固定 handoff 文本处理。
- 计量器必须继续拒绝无法识别的复杂 heredoc,防止 body 内的文本伪造 control invocation。

## [2026-08-07 10:53:54] [Session ID: omx-1786061963768-e7in9l] 笔记: Pi 与 runner 的 rdog 二进制 provenance

### 现象

- candidate artifact 中,本应被当前 parser 接受的 raw `@cmd` 与 positional `@window-find` 仍返回旧版 `code:64`。

### 静态证据

- `runner/run_macos_ops_eval.py::build_pi_env()` 只设置 `PI_CODING_AGENT_DIR` 与 `PI_TEST_MODE`,没有固定 `rdog` 可执行文件或 PATH 优先级。

### 动态证据

- 无 GUI 副作用的 Pi probe 实际执行 `command -v rdog && rdog --version && shasum -a 256 "$(command -v rdog)"`。
- tool result 返回 `/Users/cuiluming/.cargo/bin/rdog` 与 SHA-256 `57eae7f8660c16c1abf2584f8072d8c083ab77219e1434e94cf774cbbf04c9ac`。
- current `/Users/cuiluming/local_doc/l_dev/my/rust/rustdog/target/debug/rdog` 的 SHA-256 为 `db5cb9fde3afd4e6d7c54c1375af1578e450994e457ae72eb6c174fe9d0f39c7`。

### 结论

- 已验证: candidate 的 Pi bash tool 调用旧安装版。相同版本字符串 `rustdog 3.0.0` 不能证明二进制来源。
- 修复应在 runner config / Pi child environment 上形成单一真相源,然后把解析到的真实路径和 SHA 写入每次 run artifact。不得修改 `~/.cargo/bin/rdog`,不得按模型、App 或 case 分支。

## [2026-08-07 10:57:30] [Session ID: omx-1786061963768-e7in9l] 笔记: 修复后的 Pi binary probe

### 验证命令

- 通过 `run_macos_ops_eval.py::build_pi_env()` 启动与正式 runner 相同参数的 Pi,仅执行 `command -v rdog && rdog --version && shasum -a 256 "$(command -v rdog)"`。

### 关键输出

- `command -v rdog` 返回 `/Users/cuiluming/local_doc/l_dev/my/rust/rustdog/target/debug/rdog`。
- shell 计算出的 SHA-256 是 `db5cb9fde3afd4e6d7c54c1375af1578e450994e457ae72eb6c174fe9d0f39c7`,与 config 指向的 current binary 一致。

### 结论

- 已验证: PATH 前置在 Pi bash runtime 中生效。上一轮 candidate 的二进制来源问题已被修复,但 interaction 改善仍须以新的完整 matrix 为准。

## [2026-08-07 11:22:58] [Session ID: omx-1786061963768-e7in9l] 笔记: ledger 归档的二进制 provenance 门禁

### 静态证据

- `run_macos_ops_eval.py` 已把 config `rdogBinary` 的绝对路径和 SHA-256 写入每个 `run-plan.json`。
- 原 `macos_ops_interaction.py` 只读取该字段,不验证其与 archive 时的 config 相同,所以旧 binary artifact 理论上仍能进入新 candidate。

### 验证

- 归档器现在从 config 重新计算 `rdogBinary` identity,并在构建每个 run ledger 前比较 source `run-plan.rdog` 的 path/SHA-256。
- 纯文件回归覆盖正常归档、缺少 provenance 和 hash mismatch。`python3 -m unittest test_macos_ops_interaction.py` 为 10 passed;`python3 -m unittest test_run_macos_ops_eval.py` 为 27 passed;`ruff check runner` 通过。

### 结论

- provenance 是评测输入的一部分,不是只读展示字段。缺失或不一致时归档必须失败关闭,否则 requestCount 不能证明属于当前 rdog build。

## [2026-08-07 16:39:13] [Session ID: omx-1786061963768-e7in9l] 笔记: parser compatibility 独立重复采样

### 验证命令与关键输出

- 两次独立 `runner/eval-macos-ops.sh all` 均完成 5 x 8 live matrix。归档器对每份 source `run-plan.json` 检查 current binary path 和 SHA-256。
- repeat A: 40/40 成功,272 agent decisions,252 rdog requests,40 attempts,31 recovery,30 response errors。
- repeat B: 40/40 成功,354 agent decisions,340 rdog requests,44 attempts,67 recovery,71 response errors。
- current reference 与 A/B 的 rdog SHA-256、canonical skill SHA-256 和所有输入内容 hash 一致。manifest 中 skill 的绝对/相对 path 表示不同,对应 SHA-256 相同,不构成内容差异。

### 结论

- current reference 的 `243` requests 低于历史 baseline 的 `252`,但 A 回到 `252`,B 升至 `340`。三个 current-binary 样本的中位数为 `252`,没有严格低于历史 baseline。
- repeat B 的高成本主要由 MiniMax-M3 (`101` requests) 和 MiniMax-M2.7-highspeed (`106` requests) 贡献。当前只有动态成本分布,没有足够证据把它归为 parser regression 或授权新的 parser/skill 兼容分支。
- 已验证结论是: shared parser compatibility 的单轮收益不能表述为稳定交互效率改善。保留实现和原始 artifacts,不升级新的效率 baseline。

## [2026-08-08 01:02:49] [Session ID: omx-1786061963768-e7in9l] 笔记: 稳定共享摩擦候选筛选

### 动态证据

- `@key` 对象 `target` 未知字段: 11 次,7 个独立 `(model, case)` 样本,3 轮;8 次紧接 recovery。
- `@key` 对象 `keys` / `shortcut` 未知字段: 9 次,5 个独立样本,至少 2 轮;4 次紧接 recovery。
- `@ax-press` 顶层 `action` 未知字段: 9 次,4 个独立样本,3 轮;9 次紧接 recovery。
- 三类合计 29 个 response errors,21 次紧接 recovery。21 是暴露的可避免 recovery 上限,不是已验证收益。
- App selector 多窗口歧义: 10 次,6 个独立样本,每次都进入 recovery;这是现有 fail-closed 唯一窗口不变量的正常保护。
- AX/window locator stale: 11 次,6 个独立样本,每次都进入 fresh re-query 或 recovery;不能静默重绑。

### 静态证据

- `src/control_protocol/parsers/key.rs:41-175` 明确定义 `@key` 对象字段,没有 `target`、`keys`、`shortcut`;targeted delivery 需要显式 `delivery` 与 `pid` 或 `window_id`。
- `src/control_ax.rs:1231-1325` 的 `@ax-press` parser 固定构造 AXPress;其它 action 由独立 `@ax-action` parser 处理。
- `src/control_window.rs:718-760` 要求 app selector 的 fresh 查询返回唯一且可交互窗口;`src/control_window/macos.rs:1681-1690` 对过期 window index 明确 fail closed。
- `.codex/skills/rdog-control/references/protocol.md:35-56` 有基础 `@key` 示例,但第 633 行引用 canonical `SKILL.md` 中不存在的 `Local Key Chords` 章节。

### 判断

- 建议批准的第一步是通用契约澄清: 在 canonical skill 中补齐 `@key` 对象字段、targeted delivery 约束,并明确 `@ax-press` 只表达 AXPress、其它 action 使用 `@ax-action`;同时修正 protocol reference 的断链。该方向不接受歧义字段,不改变 target、权限或 action 语义,也不写 App/case 操作序列。
- 不建议批准 `target`、`keys`、`shortcut` 或通用 `action` parser alias。它们可能改变投递范围、把数组当 chord/sequence 猜测,或把 AXConfirm/AXShowDefaultUI 错路由为 press。
- 暂缓自动修复 app 多窗口歧义和 stale locator。后续只能基于显式 durable selector/ref + `auto_refind` policy 设计,不能根据 app/case 文本猜目标。

## [2026-08-08 01:48:00] [Session ID: omx-1786061963768-e7in9l] 笔记: key contract candidate 动态认证

### 现象

- 首次执行 matrix 在 setup 的 `prepare-open` 失败,`rdog control @ping` 显示没有 active managed local-default registry。
- 使用仓库当前 binary 和 `rdog_macos.toml` 启动 daemon 后,同一 `@ping` 返回 `pong`,完整 matrix 随后可运行。

### 验证

- 5 个活动模型各完成 8/8,合计 40/40,全部为 attempt 1。
- ledger 为 213 decisions、209 requests、20 response errors;current reference 为 258、243、25。
- candidate 原始命令中 `@key target`、`@key keys`、`@key shortcut`、`@ax-press action` 的命中数均为 0。
- 对输入兼容 current reference 做逐 `(provider, model, case)` 聚合后,有 9 个 case 请求数增加。

### 结论

- 已验证: 本轮样本消除了 decision brief 关注的两类语法漂移,并保持全部正确性证据。
- 未验证: 单轮总请求下降不能证明稳定收益,因为逐 case 门禁已失败。
- 按 workflow 拒绝 baseline promotion,保留 artifacts 供后续跨轮观察。该结论不授权新增 parser alias 或模型/App 特例。

## [2026-08-08 23:50:00] [Session ID: omx-1786201921174-cvveb1] 笔记: outcome 三态 macOS live smoke + preexisting 中间档真实出现

### 现象 (live evidence)

第一次跑 `smoke_computer_act_verify.sh` test 3 (`verify:"best_effort" + wait 0ms`):

真实 wire response (简化):
```json
{
  "ok": true,
  "outcome": "worked",
  "verification": {
    "method": "ax_diff",
    "ax_diff": {
      "changed": 0,
      "elements_added": 1,
      "elements_removed": 1,
      "elements_modified": 0,
      ...
    },
    "status": "preexisting",
    "report": {
      "elements": {
        "pid:538/window:0/path:3.0.0": {"kind": "Added", "role": "AXGroup"},
        "pid:86138/window:3/path:11.8.0.0": {"kind": "Removed", "role": "AXStaticText", "value": "速度"}
      },
      ...
    }
  }
}
```

### 综合发现

#### outcome 三态 decision table 真实有效

| dispatch_ok | verify_requested | verify_ran | verify_passed | outcome | 真实出现? |
|-------------|------------------|------------|---------------|---------|-----------|
| true | false | - | - | worked | test 1 / test 2 (默认 verify=None) |
| true | true | true | true | worked | test 4 (verify=always, changed=12) / test 3 第一次跑 (status=preexisting) |
| true | true | true | false | didnt | test 3 第二次跑 (verify_passed=false) |
| true | true | false | - | unknown | (未出现, 需要 verify timeout) |
| false | - | - | - | unknown | test 5 (verify:"bogus" → invalid_verify error envelope, outcome 不写入) |

#### verification.status 三档真实出现

- "failed": test 3 第二次跑 (verify_passed=false, ax_diff 全 0)
- "preexisting": test 3 第一次跑 (changed=0 + morphed=2, OS 背景变化)
- "verified": (未直接出现, 需要 action 真生效的 case; wait 0ms 不会改 GUI, 但 verify=always 触发了 method="full" 路径不走 status)

#### smoke 期望错误 (设计 bug)

`smoke_computer_act_verify.sh` test 3 期望 `outcome:"didnt"`, 但真实 macOS 上 wait 0ms 期间 OS 背景会让 AX diff 出现 1 add + 1 remove → verify_passed=true → outcome:"worked" + status:"preexisting".

Phase F-2 时代假设 "wait 0ms + verify best_effort = GUI 完全不变 = outcome:didnt", 实际 wait 0ms 不保证 AX tree 不变 (其他进程增减 element).

修法: smoke 改成枚举匹配 (outcome 三态之一 + verification.status 三档之一), 锁 wire contract 不锁特定值. 特定值 regression 由 outcome.rs 7 个单测抓.

#### daemon TCC 权限 OK

daemon 日志 (`/var/folders/.../computer-act-verify-smoke-XXXX*/computer-act-verify-smoke-daemon.log`) 无 TCC warning. verify_ms=955-1023ms 不是 0, AX diff 真出来 (pid:538 加 element, pid:86138 减 element). Accessibility TCC 权限已授权.

### 结论

- outcome 三态 decision table 5 行全部实证, 包括 "preexisting" 这一之前设计时顾虑会不会真实出现的中间档. 现在确认 "preexisting" 是真实有效的 wire 档位, 让 client 能区分 "动作真生效" vs "AX 拓扑变了但 field 没变".
- smoke 期望锁 wire contract 不锁特定值是更鲁棒的设计哲学.
- macOS TCC 权限 (accessibility) 已授权, AX capture 真实跑.
