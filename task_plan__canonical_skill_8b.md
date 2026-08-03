# 任务计划: canonical rdog-control skill on Bonsai 8B

## [2026-07-27 17:27:38] [Session ID: omx-1784789038072-clve0o] 目标与阶段

### 目标

只使用并优化 canonical `.codex/skills/rdog-control/SKILL.md`,让 Pi 通过正式 `rdog-control-bash` profile 在 `models/ternary-gguf/8B` llama-server 上完成多领域 rdog control 任务.

### 已纠正口径

- 旧 v2/v3 runner删除profile的`skills`字段,再通过`--append-system-prompt`注入专用Calculator文件.
- 旧 v3 `5/5`只证明专用prompt有效,不能作为canonical skill成绩.
- 后续禁止创建或注入新的任务专用skill;canonical文件是唯一skill真相源.

### 两个方向

- 最佳方案:Pi使用正式`~/.pi/agent/models.json`与`rdog-control-bash` profile,由`skills:["rdog-control"]`加载canonical符号链接;从事件和usage证明实际加载.
- 备选方案:若Pi native loader无法提供可验证证据,仍只读取canonical文件,修复Pi/profile loader本身;不回退到专用skill.

### 阶段

- [ ] 阶段1:证明8B llama-server模型路径、Pi profile、canonical symlink和native skill加载链.
- [ ] 阶段2:建立不含专用skill的canonical基线,测首轮input tokens与真实tool loop.
- [ ] 阶段3:根据跨任务失败证据直接优化canonical文件,每次只改变一个通用变量.
- [ ] 阶段4:冻结canonical hash,运行多领域holdout并完成回归/文档收口.

### 验收边界

- 模型必须是`models/ternary-gguf/8B/Ternary-Bonsai-8B-Q2_0.gguf`对应的`bonsai-8b-ternary-q2`.
- Pi必须通过正式profile加载canonical skill,不能使用`generic-skill-v*.md`.
- GUI成功要求真实rdog calls、准确窗口归属、performed timeline、fresh AX/window/URL证据和cleanup.
- Calculator只是其中一个case;至少再覆盖liveness/capability与browser/window lane.

**目前在阶段1:追踪Pi实际命令与native skill loader,建立可证伪加载证据.**

## [2026-07-27 17:32:01] [Session ID: omx-1784789038072-clve0o] [行动]:验证 native skill 加载链

- [ ] 读取 Pi `context-preview`、profile 和 skill loader 源码,确认 `read` tool 与 `skills` 字段如何进入 system prompt.
- [ ] 用正式 `PI_CODING_AGENT_DIR=/Users/cuiluming/.pi/agent` 生成 context preview,查验 canonical skill 的名称、版本和独有命令.
- [ ] 检查 8080 监听进程及完整启动参数,确认实际 GGUF 是 `models/ternary-gguf/8B/Ternary-Bonsai-8B-Q2_0.gguf`.
- [ ] 证据成立后再为 runner 增加 native-profile 模式;若证据不成立,先修 loader/profile,不注入专用 prompt.

### 遇到的非致命错误

- 一次性 `ls` 查询尚未创建的支线上下文文件时退出码为 1.这些文件按懒创建规则本就允许不存在,当前只有 `task_plan__canonical_skill_8b.md` 含实质记录,因此不创建占位文件.

**目前仍在阶段1:先证明真实加载链和模型进程,暂不采信旧 v3 `5/5`.**

## [2026-07-27 17:33:43] [Session ID: omx-1784789038072-clve0o] [状态]:模型与 loader 证据已确认

- [x] 静态链:正式 model entry 解析到 `rdog-control-bash`;profile 限制工具为 `bash,read`;`extend_with_model_skills` 从 `profile.skills` 解析 `~/.pi/agent/skills/rdog-control`;启用 `read` 后 `format_skills_for_prompt()` 进入 system prompt.
- [x] 模型进程:PID 26636 的完整命令使用目标 `models/ternary-gguf/8B/Ternary-Bonsai-8B-Q2_0.gguf`,alias 为 `bonsai-8b-ternary-q2`,backend 为 repo 内 `bin/mac/llama-server`.
- [x] canonical identity:Pi skill 是 canonical `SKILL.md` 的符号链接,两端 SHA-256 均为 `a396ced376662c40e90c62bef0dad3b2d974e4bb7fb904428cc9f8269df9c392`.
- [x] 可证伪实验:`pi context-preview` 返回 `provider_calls:0` 的语义 bundle,不包含 system prompt.因此撤回"用 context-preview 证明 skill 注入"的假设;它不能作为该证据.
- [ ] 动态 native-profile 证据:通过 runner 生成并执行不含专用 prompt、不禁用 skills、保留 `bash,read` 的真实 Pi agent loop.

### 新发现的命令错误

- `rtk grep ... src/context*.rs` 被 zsh 在无匹配文件时提前拒绝,报 `no matches found`.该命令没有读取任何文件,也没有影响程序状态;后续使用已知文件或先枚举文件,不再传未引用 glob.

**目前在阶段1尾部:先写 runner 命令合同 RED 测试,再实现 native-profile 模式.**

## [2026-07-27 17:47:49] [Session ID: omx-1784789038072-clve0o] [状态]:阶段1完成,进入canonical优化

- [x] 动态 native-profile smoke:真实 `bash` tool call执行 `rdog control @ping`,收到`pong`,第二轮完成简短报告.
- [x] 首轮 usage 已取证:input 18,317 tokens;全局`~/.pi/agent/AGENTS.md`经同一llama tokenizer计为16,841 tokens,确认为主要prefill来源.
- [x] runner命令合同先RED后GREEN:16个测试通过,禁止专用prompt、`--no-skills`和`--tools bash`;正式agentDir/profile是唯一加载路径.
- [x] 修正前静态事实:Pi skill目录只被写成可用skill目录,正文没有自动内联;profile旧文案与实现不一致.
- [ ] 将profile改为首次`read` canonical正文,并把canonical入口压缩为通用核心循环+既有references导航.
- [ ] tokenizer复测后运行一次隔离global AGENTS的canonical-read smoke,要求出现`read -> bash -> fresh result`多轮链.

### 状态

**阶段1已完成.目前在阶段2/3交界:先减少无关prefill并确保模型真实读取canonical skill,再建立有效基线.**

## [2026-07-27 17:51:25] [Session ID: omx-1784789038072-clve0o] [状态]:canonical-read基线通过

- [x] canonical入口从3,284降到1,588 tokens;profile规则从359降到119 tokens.
- [x] native agent loop按`read canonical -> bash rdog -> final`完成,未使用任何`--append-system-prompt`或专用skill.
- [x] 首轮input从18,317降到1,181 tokens,减少93.6%;首轮时延从约340秒降到13.3秒.
- [x] 第二轮自主选择`rdog control @ping`,input 3,782/cache read 1,177,工具耗时173ms并返回`pong`.
- [x] 第三轮input 3,885/cache read 3,778,provider streaming 2.585秒并正确报告.
- [ ] 同步canonical源码中的通用`app:APP`、`@ax-press-sequence`能力,补parser/window ownership测试并编译.
- [ ] 完成Calculator与browser/window holdout,冻结新canonical hash.

### 状态

**阶段2已完成.目前在阶段3:先消除skill文档与canonical源码能力漂移,再跑GUI holdout.**

## [2026-07-27 17:53:43] [Session ID: omx-1784789038072-clve0o] [纠偏与行动]:只优化 canonical skill

### 用户确认的唯一方向

- [x] 只使用 `/Users/cuiluming/local_doc/l_dev/my/rust/rustdog/.codex/skills/rdog-control/SKILL.md`.
- [x] 只使用 `models/ternary-gguf/8B/Ternary-Bonsai-8B-Q2_0.gguf`、正式 Pi `rdog-control-bash` profile 和 `llama-server`.
- [x] 禁止为 8B、Calculator 或单个测试任务创建、注入、拼接专用 skill/prompt.
- [x] 旧任务专用 prompt 的 `5/5` 不计入 canonical skill 成绩.

### 当前行动

- [ ] 逐文件审查临时验证树与 canonical rustdog 的差异,只提取通用 `app:APP`、`window_id` ownership 和 `@ax-press-sequence` 能力.
- [ ] 先补 parser RED 与唯一可交互窗口归属 RED 测试,再移植实现.
- [ ] 运行精确单测、相关 nextest、check 和 clippy,任何 error/warning 都必须处理.
- [ ] 从 canonical rustdog 重建并验证 Calculator compact 命令的真实 performed timeline 与 fresh AX 结果.
- [ ] 用 canonical-only Pi runner 跑 Calculator 和 browser/window holdout,冻结 skill hash并收口支线记录.

### 防绕路门禁

- runner 命令中不得出现 `--append-system-prompt`、`--system-prompt`、`--skill`、`--no-skills` 或覆盖 profile 工具集的 `--tools bash`.
- Calculator 示例只允许作为 canonical skill 中的通用语法示例和 holdout 输入,不得成为另一份知识源.
- 若程序能力与 skill 声明不一致,先修 canonical rustdog 程序与测试;不得用更长提示词掩盖程序缺口.

**目前在阶段3:先完成通用程序能力同步和回归验证,再进行 canonical-only 8B GUI 评测.**

## [2026-07-27 18:00:30] [Session ID: omx-1784789038072-clve0o] [状态]:parser 与窗口归属 RED 已确认

- [x] parser RED:`@ax-find:app:Calculator,AXStaticText` 在 canonical parser 中失败,nextest 退出码 100.
- [x] ownership RED:`select_unique_interactable_window_id` 尚不存在,测试编译以 E0425、退出码 101 失败.
- [x] 已确认不需要移植临时树的 `WindowFindResponseMode`、compact `@window-find` 或 `control_window/macos.rs` 改动.
- [ ] 实现共享 shell-safe compact selector、AX target window ownership、fresh exact-app 唯一窗口解析和有序 press sequence.

### 遇到的命令错误

- `cargo nextest run ... --exact` 被 nextest 拒绝,退出码 2;当前版本应使用 filter expression.
- `cargo nextest run --lib ...` 因本包没有 library target 退出 101;精确单测应针对 `rustdog::bin/rdog` 测试目标且不加 `--lib`.
- 编译同时显示 8 条来自其他未完成 computer-act 修改的既有 warning.本支线不改动其他 Session 的源码;最终验证会区分本次新增 warning 与既有 warning.

**目前在阶段3实现期:RED 已固定,开始移植最小通用能力.**

## [2026-07-27 18:14:06] [Session ID: omx-1784789038072-clve0o] [状态]:通用程序能力局部 GREEN

- [x] 实现共享 shell-safe compact selector,支持 `app:APP` 与 canonical `pid:PID/window:INDEX`.
- [x] `AxTarget.window_id` 参与窗口归属匹配;语义动作有 window ID 时只抓目标窗口 AX snapshot.
- [x] `app:APP` 使用 fresh exact query,仅接受唯一且可交互窗口;sequence 在首个 side effect 前只解析一次 app.
- [x] `@ax-press-sequence` 最多 32 项,保持顺序,首个失败停止;每个已尝试步骤都有 `performed` timeline与可选 error.
- [x] AX 分组 37/37 通过;protocol/window 分组 105/105 通过;parser RED 与 ownership RED 均已转 GREEN.
- [x] canonical skill、protocol reference、line-control spec 和 README 已同步通用合同,未增加任务专用 skill.
- [x] runner 禁用参数搜索无匹配,确认没有 `--append-system-prompt`、`--system-prompt`、`--skill`、`--no-skills` 或 `--tools` 覆盖.
- [ ] 运行全量 nextest、check、clippy并审计 warning.
- [ ] 从 canonical repo 重建/安装 `rdog`,执行 live Calculator compact 命令和 fresh AX 验证.

**目前仍在阶段3:局部 GREEN 完成,进入全量回归和 live program verification.**

## [2026-07-27 18:19:46] [Session ID: omx-1784789038072-clve0o] [用户纠偏后的收口]:canonical skill 是唯一优化对象

### 已确认方向

- [x] 承认此前任务专用 prompt 路线走偏.旧专用 runner 的结果不纳入 canonical skill 成绩.
- [x] 唯一 skill 真相源固定为 `/Users/cuiluming/local_doc/l_dev/my/rust/rustdog/.codex/skills/rdog-control/SKILL.md`.
- [x] Bonsai 8B、Calculator 和小红书只作为外部评测条件与 holdout 输入,不得把任务答案写入另一份 skill 或额外 system prompt.
- [x] Pi 继续使用正式 `rdog-control-bash` profile、`read` + `bash` 工具、多轮 agent loop 和 `llama-server`.

### 当前执行边界

- [ ] 只验证本轮新增的通用 AX/parser/window ownership 能力及其直接回归;与该能力无调用关系的既有 TTY 测试失败单独记录,不扩张为本轮修复目标.
- [ ] 从 canonical rustdog 重建可执行文件,用通用 compact 命令完成 Calculator 程序层验证.
- [ ] 运行不含任何专用 prompt 的 canonical-only Pi Calculator holdout.
- [ ] 运行不含任何专用 prompt 的 canonical-only Pi browser/window holdout.
- [ ] 冻结 canonical skill hash,记录验证证据并完成阶段4收口.

**目前在阶段3:先完成 canonical 程序层验证.本轮停止条件仍是两个 canonical-only Pi holdout 均有真实工具调用和新鲜状态证据.**

## [2026-07-27 18:22:32] [Session ID: omx-1784789038072-clve0o] [状态]:canonical AX 相关回归完成

- [x] 本轮新增测试 9/9 通过,覆盖 compact parser、唯一窗口 fail-closed、app 只解析一次、同窗 sequence、首错停止和 targeted capture.
- [x] 扩大的 AX/window nextest 回归 124/124 通过.
- [x] `git diff --check` 与 `cargo fmt --check` 通过.
- [x] `cargo check` 成功.输出 6 条 warning,经 `HEAD` 与当前 diff 对照确认均由其他 Session 的 computer-act 脏改动触发,本轮不修改其代码.
- [ ] 整仓 clippy 未通过:4 个 deny-level `clippy::never_loop` 位于未修改的 `src/pty_control.rs` 和 `src/zenoh_control/client_pty.rs`;另有既有 warning.该结果如实保留,不冒充通过,也不扩张本轮范围.
- [ ] 构建 canonical `rdog`,验证 live Calculator compact sequence 与 fresh AX 结果.

**待办进度:程序相关静态/单测回归已完成;现在进入真实 GUI 程序层验证.**

## [2026-07-27 18:28:08] [Session ID: omx-1784789038072-clve0o] [状态]:canonical 程序层 Calculator 验证通过

- [x] 首次 live smoke 发现 canonical skill 的 `@open-app:Calculator` 与只接受对象的 parser 不一致;该错误导致后续窗口匹配为 0.
- [x] 先增加 compact open-app RED 测试,确认失败文案来自真实 parser.
- [x] 复用 `parse_compact_atom` 支持通用 shell-safe app 名;复杂 app 名和自定义等待仍走对象格式.
- [x] open-app 相关测试 14/14 通过,格式检查和构建通过.
- [x] 用新构建 client + daemon 重跑 live smoke:`@open-app` 返回 `ok:true`;6 步 sequence 全部 `performed:true`并固定到 `pid:82857/window:0`;fresh AX observation `obs-1785148078110-3` 返回表达式 `3+4÷2` 与结果 `5`.
- [ ] 审计并运行 canonical-only Pi Calculator holdout,输入保持为用户原始任务 `1+2*3`.
- [ ] 运行 canonical-only browser/window holdout.

**待办进度:canonical 程序合同已用动态证据验证;现在进入 Bonsai 8B 多轮 Pi agent loop 评测.**

## [2026-07-27 18:32:28] [Session ID: omx-1784789038072-clve0o] [实验]:增强 canonical skill 的通用触发描述

### 现象与结论边界

- 第一轮 canonical-only Pi 真实执行中,happy-path 使用 `xdg-open`;stale-state 把表达式当 shell 命令.两例均未调用 `read`,也没有 rdog command.
- provider/model 路由与多轮 loop 已验证正常.第三例在 runner prepare 阶段失败,尚未进入 Pi,不能算模型样本.
- Pi 源码确认 system prompt 只列出 skill 的 name、description、location,任务匹配 description 后才要求模型调用 `read`;canonical 正文不会自动内联.

### 最小可证伪实验

- [ ] 只把 canonical frontmatter description 从以 `rdog control` 为前提的描述,改为明确覆盖所有 computer-control 任务.
- [ ] 不改 skill 正文、profile、用户 prompt、模型参数或 runner命令.
- [ ] 用完全相同的 happy-path prompt 复测,第一成功门禁是首个 tool call 为 `read` canonical SKILL.md.
- [ ] 若仍不读取,立即推翻 description 主假设,再验证 profile 路由语句;不得继续向 skill 堆任务答案.

**目前在阶段3/4交界:先证明 8B 能从自然语言 computer-control 任务进入 canonical skill,再评测技能内容.**

## [2026-07-27 18:34:17] [Session ID: omx-1784789038072-clve0o] [状态]:description 假设不成立,转验 profile 前置路由

- [x] 仅修改 canonical description 后,完全相同的 happy-path prompt 仍以 `bash` 调用 `xdg-open /usr/bin/calculator` 开始.
- [x] 首轮 input 1170 tokens,无 `read` 调用;description 变化没有改善 skill 选择.
- [x] 已回滚该 description 实验,不保留未经验证的提示词改动.
- [ ] 单变量修改正式 `rdog-control-bash.appendSystemPrompt`:明确此 profile 的每个请求都必须先 `read` canonical,读取前禁止 `bash`.
- [ ] 文案保持跨任务通用,不得出现 Calculator、Chrome、小红书或 8B 专用步骤.
- [ ] 使用相同 prompt 再次 A/B,首个 tool call 必须为 canonical `read`;否则该假设同样撤回.

**目前仍在 skill 入口验证:canonical 正文尚未进入失败的 Pi 样本,因此当前不能据此评价 skill 内容质量.**

## [2026-07-27 18:38:20] [Session ID: omx-1784789038072-clve0o] [纠偏]:不再要求 8B 自主读取 canonical skill

### 已推翻的入口实验

- [x] 扩大 canonical description 后,同一 prompt 仍调用 `xdg-open`;该假设不成立并已回滚.
- [x] 强化 `rdog-control-bash.appendSystemPrompt` 的首次 `read` 文案后,同一 prompt 仍调用 `xdg-open`;该假设不成立并已回滚.
- [x] 两次动态失败与当前源码一致:Pi 只把 skill name、description、location 写入 system prompt,没有兑现 `ToolUseProfile.skills` 注释声明的正文注入合同.

### 当前最小可证伪修复

- [ ] 在 Pi `src/resources.rs` 先写 RED:profile 显式绑定的 skill 正文必须进入 system prompt,其中的独有标记可见.
- [ ] 同一测试证明普通发现到的 skills 仍只列 metadata,避免把所有 skill 正文都塞入 prompt.
- [ ] 用一个共享装配 helper 替换 `src/main.rs` 三处重复调用,保持初始、extension refresh 和 model-skill refresh 行为一致.
- [ ] 运行 Pi 精确测试、check、clippy、fmt;保留现有脏工作树和 profile extensions 能力.
- [ ] 使用新 Pi binary 与原始 Calculator prompt 做同条件 A/B;不得出现专用 prompt 或任务专用 skill.

### 结论边界

- 当前已有静态证据与两次动态失败证据,足以确认 Pi profile skill 装配合同发生回退.
- 修复目标是恢复通用 canonical skill 注入,不是给 Bonsai 8B、Calculator 或浏览器增加特殊路由.

**目前在阶段3/4交界:先恢复 Pi 对 profile 显式绑定 canonical skill 的通用装配合同,再评价 SKILL.md 内容本身.**

## [2026-07-27 18:40:33] [Session ID: omx-1784789038072-clve0o] [状态]:profile skill 装配 RED 已固定

- [x] 精确 nextest 已运行,退出码 101.
- [x] 关键错误为 E0599:`ResourceLoader` 没有 `format_skills_for_model_prompt`;当前生产代码不存在 profile-aware 正文装配能力.
- [x] RED 同时构造了绑定 skill 正文标记与未绑定 skill 正文标记,后续 GREEN 必须只出现前者.
- [ ] 让 `Skill` 保存加载时已校验的完整文件内容,避免每次 system prompt 重建时再次读取文件.
- [ ] 实现 profile-aware formatter,再用共享 helper 接入三条 system-prompt 路径.

**目前正在实现 GREEN:profile 显式绑定正文自动注入,普通 skill 保持 metadata-only.**

## [2026-07-27 18:48:12] [Session ID: omx-1784789038072-clve0o] [状态]:Pi profile skill loader GREEN

- [x] `Skill` 在首次加载时保存已校验的完整 `SKILL.md` 内容,system prompt 重建不再二次读盘.
- [x] 新 formatter 同时支持历史 model-id 绑定和 `ToolUseProfile.skills` 显式绑定,顺序稳定并去重.
- [x] profile 绑定正文直接内联;绑定项从 `<available_skills>` 中移除,避免 metadata 要求 `read` 与已加载正文互相矛盾.
- [x] 未绑定 skill 只保留 metadata,正文标记未进入 prompt;`read` 不可用时 profile 正文仍会内联.
- [x] `src/main.rs` 三条 prompt 重建路径已收敛到 `build_session_system_prompt` 单一 helper.
- [x] 精确测试 2/2 通过;`cargo check --package pi_agent_rust` 通过.
- [ ] 运行 clippy、fmt check、diff check并审查本轮变更.
- [ ] 构建新 Pi binary,执行原始 Calculator prompt 最小 A/B.

### 命令调整

- nextest 对本仓库先枚举大量集成测试目标,不适合单个资源测试;已终止本会话启动的枚举进程.
- 一次 `cargo test ... --exact` 把 `--exact` 放在 cargo 参数区,退出码 1;修正为 `-- --exact` 后目标测试稳定通过.

**目前在 Pi 静态验证末段:完成 clippy 与 diff 审计后进入动态模型验证.**

## [2026-07-27 18:51:45] [Session ID: omx-1784789038072-clve0o] [状态]:Pi 静态验证完成,进入同条件 A/B

- [x] 资源模块测试 100/100 通过.
- [x] `cargo clippy --package pi_agent_rust --lib` 无问题.
- [x] `cargo fmt --all -- --check` 与 `git diff --check` 通过.
- [x] `cargo build --package pi_agent_rust --bin pi` 成功,新 binary 位于 Pi repo `target/debug/pi`.
- [x] diff 审计确认 loader 没有 Calculator、Chrome、小红书或 Bonsai 8B 特判;仅按通用 profile/model skill 绑定装配.
- [ ] 检查 llama-server、rdog daemon、Calculator baseline 与 runner 命令合同.
- [ ] 用原始 prompt `打开计算器程序，输入1+2*3并计算结果` 执行最小 A/B.

**目前在阶段4动态验证:首个门禁是新 Pi 不再走 `xdg-open`,而是直接使用 canonical skill 中的 rdog control 合同.**

## [2026-07-28 00:03:47] [Session ID: omx-1784789038072-clve0o] [续行]:修复评测环境清零合同

### 已证伪与已验证

- [x] `07-canonical-loader` 首次执行在 prepare 阶段失败,尚未进入 Pi,不能计为模型样本.
- [x] "Calculator AX 尚未就绪"假设不成立:首次及连续 3 次 fresh AX 查询都返回 22 个按钮.
- [x] 当前零状态按钮 description 为`删除`,不是 runner 固定使用的`全部清除`;旧语义 selector 因此匹配 0 个元素.
- [x] 通用`@key:Esc`动态实验成立:先用 sequence 建立`42`,再发送 Esc,fresh AX 从`42`变为`0`.

### 当前行动

- [ ] 先改 runner 单测,要求清零 frame 为`@key:Esc`且禁止继续依赖`全部清除`文案,运行 RED.
- [ ] 修改`clear_calculator`,复用`run_rdog`既有错误门禁,运行 runner 16 个单测.
- [ ] 重跑三例 canonical-only Calculator,PATH 继续固定新 Pi 与 canonical rdog.

**目前继续阶段4:先修评测环境的易变 AX 文案依赖,再产生有效 Pi 样本.**

## [2026-07-28 00:07:12] [Session ID: omx-1784789038072-clve0o] [状态]:runner 清零逻辑 GREEN

- [x] 单测 RED 精确显示旧 frame 仍依赖`全部清除`.
- [x] `clear_calculator` 已改为`@key:Esc`;调用方继续用 fresh AX 值`0`判定真实清零,没有弱化门禁.
- [x] 目标测试 1/1、runner 全部测试 16/16 与`py_compile`通过.
- [ ] 运行`08-canonical-loader`三例真实评测,确认每例进入新 Pi agent loop.

**目前继续阶段4:环境 prepare 已修复,开始有效 canonical-only 模型评测.**

## [2026-07-28 00:11:06] [Session ID: omx-1784789038072-clve0o] [状态]:canonical daemon 已恢复

- [x] `08-canonical-loader` 未进入 Pi;prepare/reset stderr 同时证明 active managed local-default registry 不存在.
- [x] 未清理共享 FIFO;从 canonical repo 启动`target/debug/rdog daemon -c rdog_macos.toml`.
- [x] daemon 日志确认`mac.lab` local-default 注册成功;fresh`rdog control @ping`返回`pong`.
- [ ] daemon 受控会话为`60885`,最终收口前必须终止.
- [ ] 使用新输出目录重跑三例评测.

**目前继续阶段4:服务前置条件恢复,重新开始有效模型样本.**

## [2026-07-28 00:14:31] [Session ID: omx-1784789038072-clve0o] [分析]:首轮有效 canonical skill 评测 0/3

### 动态现象

- happy-path 有 4 次真实 rdog call,但先输入固定数字`1..7`,再逐字复制 skill 的`3,加,4,除,2,等于`示例;最终不是用户要求的`7`.
- stale-state 把示例 frames 作为裸 shell 命令执行,漏掉`rdog control`前缀,exit 127.
- error-result 打开 Calculator 后输入固定数字`1..0`,没有执行用户要求的除法或 fresh verification.

### 主假设、备选解释与验证

- 主假设:canonical skill 中的固定数字示例对 8B 形成锚定,且完整 command 前缀不变量不够显著.
- 备选解释:目标 Calculator token 序列或程序能力不可用.
- 直接程序验证已排除备选解释:`1+2*3 -> 7`,`(8+4)*5 -> 60`,`1÷0 -> 未定义`,全部 sequence performed 且 fresh AX 匹配.

### 当前单变量优化

- [ ] 删除固定数字 Calculator 示例,改为从用户表达式派生的无数字模板.
- [ ] 禁止 Calculator 数字/运算符走`@key`;只使用一个 app-scoped AX sequence.
- [ ] 将每个 bash command 必须以`rdog control`开头提升为 Core Loop 全局不变量.
- [ ] 保留通用 parenthesized subexpression 规则,不写入本次 3 个 holdout 的具体答案.
- [ ] 冻结新 hash 后用完全相同的 3 个 prompt 重跑.

**目前在 canonical skill 内容优化阶段:只消除示例锚定与命令边界歧义,不增加任务专用 skill.**

## [2026-07-28 00:17:26] [Session ID: omx-1784789038072-clve0o] [状态]:canonical skill v2.1 待动态验证

- [x] 固定数字示例已移除,替换为`BUTTONS_DERIVED_FROM_USER`通用模板.
- [x] Calculator 规则明确数字/运算符不得走`@key`,每次先 Esc,再执行一个 app-scoped AX sequence,最后 fresh find.
- [x] Core Loop 明确 bash 必须运行以`rdog control`开头的完整命令,禁止裸`@...` frame.
- [x] canonical 与 Pi symlink SHA-256 一致:`2ba96ac158d2272210e2af100e64b2c87fac43f8125c16b7d67e5f4ff93be73c`.
- [x] 目标 llama tokenizer 计数为 1,714 tokens;没有写入 3 个 holdout 的具体操作数或答案.
- [ ] 使用相同 3 个 prompt 运行`10-canonical-v2.1`,与`09`做行为对比.

**目前继续阶段4:只通过动态 A/B 决定 v2.1 是否保留.**

## [2026-07-28 00:21:38] [Session ID: omx-1784789038072-clve0o] [状态]:v2.1 占位符表示法被动态证伪

- [x] v2.1 三例仍为0/3;三例都把`BUTTONS_DERIVED_FROM_USER`作为字面 AX description 发送,sequence fail-closed.
- [x] v2.1 的完整 command 前缀规则有效:所有 bash tool call 都以`rdog control`开头,不再出现裸 frame.
- [x] 固定`3+4÷2`示例锚定已消失,但 code block 占位符产生了新的复制锚定.
- [ ] v2.2 删除 Calculator 可执行模板 code block,只用自然语言步骤描述提取、映射、单 sequence 与 fresh verify.
- [ ] 继续禁止数字/运算符使用`@key`;不增加任何 holdout 具体操作数.
- [ ] tokenizer 必须下降,再用同一三例复测.

**目前继续 canonical skill 优化:保留已验证有效的前缀不变量,撤回被证伪的占位符设计.**

## [2026-07-28 00:29:49] [Session ID: omx-1784789038072-clve0o] [状态]:v2.2 Keyboard lane 竞争已动态确认

### 动态现象

- [x] `11-canonical-v2.2` 完成 3 个正式 Pi 多轮样本,canonical SHA-256 为 `ae3f1ac17870f81f2512c977606bc39e35f39babacc8e77bb3d9f8164c9de766`,结果仍为 0/3.
- [x] happy-path 连续生成 `@key:Esc`、`@key:Cmd+Shift+R`、三个数字 key 和不支持的 `@key:Equals`;没有打开 Calculator、AX sequence 或 fresh AX find.
- [x] stale-state 连续生成 Esc、Cmd+R 和 Cmd+Shift+R,最后 fresh 外部验证仅为 `0`;没有 AX sequence.
- [x] error-result 只生成 `@key:Esc` 和 `@key:Escape`;没有打开 Calculator、执行除法或 fresh AX find.
- [x] v2.1 的字面占位符复制已消失,完整 `rdog control` 前缀规则继续有效;因此这两项不是 v2.2 的当前失败原因.

### 主假设与备选解释

- 主假设:入口后半段的通用 Keyboard 语法和可复制命令产生近因竞争,覆盖了前面的 Native controls / Calculator AX 路由.
- 最强备选解释:即使移除 Keyboard 竞争,8B 仍无法从用户表达式组合正确的 AX sequence.
- 推翻主假设的证据:移除入口内 Keyboard 详细语法后,相同 3 例仍主要生成 `@key`,或仍没有任何 `@ax-press-sequence`.

### 下一轮单变量实验

- [ ] v2.3 只消除入口内 Keyboard 详细语法对 Native controls 的竞争;不改 Pi loader、profile、runner、模型参数、holdout prompt 或 rdog 程序.
- [ ] 保留跨任务通用规则:只有用户明确要求快捷键,或语义 lane 无法表达动作时才使用 `@key`;native 可见控件不得用 key 模拟.
- [ ] 记录新 hash、目标 tokenizer 数和 canonical symlink identity,再用完全相同的 3 个 prompt 重跑.
- [ ] 若 3 例仍无 AX sequence,立即推翻当前主假设并回滚或进入下一个已记录的单变量实验.

**目前仍在阶段4:先验证 Keyboard lane 竞争是否真实决定 8B 的错误动作选择.**

## [2026-07-28 00:32:17] [Session ID: omx-1784789038072-clve0o] [门禁]:v2.3 准备真实评测

- [x] canonical 与 Pi symlink SHA-256 均为 `2e968d644074439dfa06cc16fb054751c9c9c3febad4bfa2da3a56d0699b1bca`.
- [x] 目标 llama tokenizer 为 1,593 tokens,比 v2.2 的 1,665 tokens 减少 72.
- [x] `git diff --check` 通过;profile 仍只提供 `bash,read` 并只绑定 canonical `rdog-control`.
- [x] 8080 仅返回 alias `bonsai-8b-ternary-q2`;rdog local-default 返回 `pong`.
- [x] runner 单测 16/16 通过;dry-run 仍为相同 3 个 case,每例一次 canonical profile.
- [ ] 从全新 `12-canonical-v2.3` 目录顺序运行 3 个 Pi 多轮样本.

**待办进度:静态和环境门禁全部通过,现在开始唯一 v2.3 真实 suite.**

## [2026-07-28 00:36:23] [Session ID: omx-1784789038072-clve0o] [假设回滚]:仅删除 Keyboard 示例不足以恢复 AX 路由

- [x] `12-canonical-v2.3` 完成 3/3 Pi 多轮样本,结果仍为 0/3;所有样本 reset 后 Calculator 均已退出.
- [x] happy-path 开始尝试 AX,但生成缺少 role 的 `@ax-find:app:Calculator` 与不受支持的 `@window-find:Calculator`;两个命令均返回 code 64.
- [x] stale-state 只执行一次 Esc,fresh 值为 `0`;error-result 仍用 `@key` 输入数字并自行报告数学常识.
- [x] 上一主假设不成立:删除后置 Keyboard 示例没有让相同 3 例生成任何 `@ax-press-sequence`.
- [ ] v2.4 改为通用 lane capsule:所有分支共享一个 tight loop,Native App 与 Browser 各自共置合法命令形状、动作步骤和 fresh 完成条件.
- [ ] 将 target 细节、bootstrap、WeChat、PTY、flow 和长安全说明下沉到既有 references;不新增 reference 或专用 skill.
- [ ] 保持 3 个 holdout 的数字和答案不进入 skill;继续使用相同 runner/profile/model 做动态判定.

**目前继续阶段4:下一实验变量是入口信息层级和共置结构,不是继续增加 Keyboard 禁令.**

## [2026-07-28 00:38:15] [Session ID: omx-1784789038072-clve0o] [门禁]:v2.4 lane capsule 待动态验证

- [x] canonical 与 Pi symlink SHA-256 均为 `a80af4f39959f2ed9e336986d7fb83fcb5a09fe5b51581b33a49871c0f57874d`.
- [x] 入口为 86 行、800 tokens;相对 v2.3 的 1,593 tokens 减少 793,降幅 49.8%.
- [x] holdout 操作数、答案、占位符和站点词搜索均无匹配;`git diff --check` 通过.
- [x] dry-run 仍为相同 3 例,rdog 返回 `pong`,新 artifact 目录不存在.
- [ ] 从全新 `13-canonical-v2.4` 运行 3 个正式 Pi 多轮样本.

**待办进度:v2.4 静态门禁全部通过,开始 lane capsule 动态 A/B.**

## [2026-07-28 00:43:14] [Session ID: omx-1784789038072-clve0o] [分析]:v2.4 进入正确 lane,表达式编译仍错误

- [x] `13-canonical-v2.4` 完成 3/3 Pi 多轮样本,结果 0/3,所有 reset 均验证 Calculator 已退出.
- [x] happy-path 首次真实完成 open-app、合法 AX find、5 步 performed sequence;但 sequence 为整个映射集合`加,减,乘,除,等于`,fresh 外部值为`0÷`.
- [x] stale-state 三次把裸`@key:Esc`交给 bash,均 exit 127;error-result 用完整前缀打开并清零后,把映射集合误写成一个 `@key`.
- [x] lane capsule 主方向获得局部动态支持:相比 v2.3 的 0 AX step,v2.4 至少让 happy-path 进入正确 app-scoped semantic lane.
- [ ] v2.5 只修 capsule 内的确定性转换:逐 token 保留数字,每个实际出现的运算符只映射一次,动作命令必须完整,动作后 fresh read 是完成条件.
- [ ] 继续不写入 holdout 数字或答案,整体 lane capsule 与其他 lane 保持不变.

**目前继续阶段4:v2.4 结构保留,下一变量是 Calculator 表达式到 AXButton description 的确定性编译合同.**

## [2026-07-28 00:48:06] [Session ID: omx-1784789038072-clve0o] [假设回滚]:逐 token prose 未形成合法 sequence

- [x] `14-canonical-v2.5` 完成 3/3 Pi 多轮样本,结果 0/3,performed step 全为0,reset均验证.
- [x] happy/error 已只保留实际运算符,但仍遗漏全部数字,并把合法`app:Calculator`简化为无效`Calculator` selector.
- [x] stale-state 直接复制`app:APP`,证明抽象 app 占位符仍会泄漏到可执行命令.
- [x] "逐 token emit"本身不足以让8B保留未转换的数字;该表述假设不成立.
- [ ] v2.6 改用 copy-edit 领先词:先复制完整表达式,只编辑运算符;最终 labels 必须保留所有原数字及顺序.
- [ ] Calculator 子路径使用真实稳定 selector `app:Calculator`,不再要求模型替换`APP`;其他 native app 仍保留通用 app-scoped合同.
- [ ] 用同一3例验证实际 sequence、fresh read与cleanup.

**目前继续阶段4:下一实验消除两个已动态暴露的抽象占位符,不增加任务答案.**

## [2026-07-28 00:56:33] [Session ID: omx-1784789038072-clve0o] [根因修正]:评测实际温度不是声明的0

### 现象与已验证结论

- [x] runner config 声明`temperature:0`,但`build_pi_command`不使用该字段;正式 Bonsai model entry 原先也没有 generation temperature.
- [x] Pi provider 静态路径只从 request options 或 model/provider generation compat 发送 temperature.
- [x] `/slots` 保留的最近 Calculator 请求显示`temperature:0.5`;此前 v2.0-v2.6 不是受控 temperature=0 A/B,不能作为最终可复现成绩.
- [x] runner RED 测试精确失败为`None != 0`.
- [x] 正式 model entry 已增加`generation.temperature:0`;runner load_config 现在强制与 eval config 相等.
- [x] runner 17/17、py_compile、models JSON、dry-run全部GREEN.
- [x] 新正式 Pi smoke 的`/slots`显示`temperature:0.0`,随后真实`rdog control @ping -> pong`并完成第二轮报告.

### 当前行动

- [ ] 不再改 skill,使用当前 v2.6 hash 与相同3例运行首个受控 temperature=0 suite.
- [ ] 结果输出到`16-canonical-v2.6-temp0`;只以performed timeline、fresh AX和cleanup评分.
- [ ] 根据受控结果决定保留 v2.6、回到更短 v2.4,或进入通用程序接口改良.

**目前阶段4重新建立有效基线:此前 artifact 保留为失败模式研究,不再冒充temperature=0最终样本.**

## [2026-07-28 01:00:39] [Session ID: omx-1784789038072-clve0o] [分析]:首个temperature=0基线为0/3

- [x] `16-canonical-v2.6-temp0` 完成 3/3,最近 server slot 再次确认`temperature:0.0`,reset全部验证.
- [x] happy-path 将完整表达式作为单个 sequence item,因`*`不安全返回 code 64.
- [x] stale-state 首次同样失败,随后自我修正为逐按钮`8,加,4,乘,5`,5步performed且fresh值为`8+4×5`;缺中间与最终`等于`.
- [x] error-result 绕到无效`@cmd:Calculator,...`,用数学常识代替真实 UI 结果.
- [ ] v2.7 将 sequence 定义为一按钮一逗号项;左括号不产生按钮,右括号产生中间`等于`,表达式末尾再产生最终`等于`.
- [ ] Calculator结果只允许来自动作后的fresh AXStaticText,包括错误/未定义结果.
- [ ] 其余所有实验条件保持不变,运行`17-canonical-v2.7-temp0`.

**目前继续阶段4:受控基线已建立,下一变量是sequence payload的可检查数据结构.**

## [2026-07-28 01:07:18] [Session ID: omx-1784789038072-clve0o] [分析]:v2.7 结构说明未改善真实动作序列

- [x] `17-canonical-v2.7-temp0` 完成 3/3 Pi 多轮样本,结果仍为 0/3;正式请求保持 `temperature:0.0`.
- [x] happy-path 两次生成逐项符号序列,但首条含空 item,第二条仍含 parser 当前拒绝的 `*`;没有 performed AX step.
- [x] stale-state 在错误反馈后改写为 `8,加,4,乘,5`,5 步全部 performed,但没有中间或最终 `等于`,fresh 值为 `8+4x5` 而不是计算结果.
- [x] error-result 仍把 `1/0` 作为单个 sequence item,没有真实 Calculator UI 结果.
- [x] "继续增加括号和最终等于 prose 就能让 8B 遵守结构"的假设不成立;v2.7 没有动态收益.

### 当前主假设与最强备选

- [ ] 主假设:通用 sequence parser 若接受安全、明确的单按钮符号别名,8B 已经自然生成的逐项输出可以进入程序动作路径.
- [ ] 最强备选:即使别名合法,8B 仍会漏中间/最终结果动作,或把整段表达式作为一个 item;因此程序改良仍不足以让三例通过.
- [ ] 推翻主假设的证据:parser GREEN 后,相同 Pi 样本仍没有正确 performed 数量和 fresh 结果.

### 本轮执行步骤

- [ ] 回读 v2.7 artifact,确认失败命令、performed timeline、fresh AX 和 cleanup 证据与记录一致.
- [ ] 先写 parser RED 测试:只接纳安全的一按钮别名,继续拒绝空 item、复合表达式和 shell 歧义字符.
- [ ] 实现通用 normalization,不新增 Calculator 专用命令,不改变 app/window ownership.
- [ ] 运行目标测试、相关 protocol/AX 测试、fmt、clippy 和 check.
- [ ] 同步精简 canonical skill,冻结 hash,用同一 3 例 `temperature:0` Pi 多轮 suite 复测.
- [ ] 只有 Calculator 达到严格门禁后,再跑 browser/window holdout;否则记录剩余模型边界并停止继续堆 prose.

**目前继续阶段4:先以程序 RED/GREEN 证伪 sequence 接口改良,再决定是否保留该方向.**

## [2026-07-28 01:07:18] [Session ID: omx-1784789038072-clve0o] [状态]:通用sequence alias测试RED

- [x] v2.7 artifact 已回读;三例命令、0/5 performed差异、fresh值和cleanup与计划记录一致.
- [x] 新测试直接调用`parse_ax_press_sequence_payload`,覆盖单item alias、复合表达式、空item与尾随逗号.
- [x] RED命令:`cargo nextest run --bin rdog -E 'test(=control_ax::tests::parse_ax_press_sequence_should_normalize_safe_button_aliases)'`.
- [x] RED关键输出:parser在第一个`*` item返回`短格式包含不安全或歧义字符`;1个目标测试失败,631个被过滤.
- [ ] 将现有generic sequence helper收窄为AXButton sequence helper;别名解析不得扩散到app名、window id或其他compact命令.
- [ ] GREEN后补跑原sequence顺序/ownership测试,确认没有破坏已有中文AX description路径.

**目前进入程序实现:测试已命中真实失败路径,开始最窄的专用parser改良.**

## [2026-07-28 01:07:18] [Session ID: omx-1784789038072-clve0o] [状态]:sequence alias parser目标测试GREEN

- [x] generic helper已收窄并重命名为`parse_compact_ax_button_sequence`;app名、window id与其他compact atom未放宽.
- [x] 单item alias归一化`+/-/*/x///÷/=`;已有中文description原样保留.
- [x] 数字+运算符的复合item、空item和尾随逗号继续在side effect前拒绝.
- [x] 目标nextest为1 passed,631 skipped.
- [ ] 运行已有sequence parser、app单次解析、partial failure和protocol compact命令测试.
- [ ] 运行真实Calculator alias命令,核对performed steps、同一window_id和fresh结果.

### 遇到错误

- 首个生产补丁因`control_ax.rs` import换行上下文不匹配而整体未应用;读取精确片段后按文件拆分重试成功,没有半应用代码.

**目前继续程序验证:目标parser已GREEN,开始验证ownership与真实UI行为.**

## [2026-07-28 01:07:18] [Session ID: omx-1784789038072-clve0o] [假设修正]:允许一个无歧义尾随分隔符

- [x] 原sequence parser测试2/2、ownership/partial failure测试3/3、canonical compact protocol测试1/1通过.
- [x] 上一记录中的`x`是记录笔误,实际实现和测试是Unicode乘号`x`对应的数学符号`×`;ASCII字母`x`没有别名.
- [x] 回读artifact确认happy-path两次均生成尾随逗号;继续拒绝会使alias GREEN无法进入真实动作路径.
- [ ] 修改测试:允许恰好一个尾随逗号,继续拒绝中间空item和两个连续尾随逗号.
- [ ] 先观察RED,再调整AXButton sequence专用helper;其他compact parser保持不变.

**目前仍在parser最小实验:补齐真实样本中的无歧义尾随分隔符,再做动态UI验证.**

## [2026-07-28 01:07:18] [Session ID: omx-1784789038072-clve0o] [动态诊断]:旧daemon仍承载control parser

- [x] 尾随分隔符RED为`短格式字段不能为空`;实现后目标1/1、parser 2/2、ownership 3/3、protocol 1/1全部GREEN.
- [x] `cargo fmt --all`与`cargo build --bin rdog`完成;构建0 error,但当前混合工作树的其他模块有6个既有warning.
- [x] 8080仍由PID 26636监听,alias为`bonsai-8b-ternary-q2`;rdog fast path返回`pong`.
- [x] 新client连接旧daemon后,`*`与`÷`仍返回旧parser code 64,fresh Calculator值均为`0`,没有side effect.
- [x] 已验证parser在daemon进程执行;磁盘binary更新不足以让运行时采用新合同.
- [ ] 终止本任务受控daemon session 60885,使用新`target/debug/rdog`和原`rdog_macos.toml`重启.
- [ ] 新daemon注册`mac.lab`并返回`pong`后,原样重跑三条alias序列.

**目前先修正运行时版本:重启受控daemon后再判定程序实验成败.**

## [2026-07-28 01:07:18] [Session ID: omx-1784789038072-clve0o] [门禁]:alias程序层三例全部GREEN

- [x] 旧daemon session 60885已终止;新daemon session 93312由当前`target/debug/rdog`启动并返回`pong`.
- [x] happy-path符号序列6/6 performed,fresh结果`7`.
- [x] parenthesized序列7/7 performed,包含中间与最终`=`,fresh结果`60`.
- [x] error-result符号序列4/4 performed,fresh结果`未定义`.
- [x] 三例每一步均解析到同一`pid:75609/window:0`;app唯一窗口ownership未改变.
- [ ] canonical skill删除已过时的中文operator copy-edit,只保留已验证的符号alias与结构不变量.
- [ ] 冻结新版本、hash、token数和Pi symlink identity,检查holdout泄漏与diff.
- [ ] 使用同一3例、正式profile、temperature 0运行新artifact suite.

**目前进入canonical同步:程序接口已验证,用更短的单一真相源替换旧映射说明.**

## [2026-07-28 01:07:18] [Session ID: omx-1784789038072-clve0o] [门禁]:canonical v2.8静态与单元验证通过

- [x] canonical skill已更新为v2.8,删除中文operator copy-edit,直接使用程序验证过的单按钮符号alias.
- [x] canonical与Pi symlink SHA-256一致:`bb3eea27842299093b60b53cf2038fcb0bf3c7c0cd31e8b12b52d40cca6166c2`.
- [x] 目标8B llama-server `/tokenize`计数为937,比v2.7的963再减少26 tokens.
- [x] 三组holdout表达式、答案和站点词在skill中无匹配;`git diff --check`通过.
- [x] reference protocol、control-line spec和README已同步alias、尾随分隔符与ownership合同.
- [x] 完整bin单元测试631 passed,1 skipped,没有失败.
- [ ] 运行cargo check与clippy,确认新代码无新增warning;既有6个跨模块warning单独记录.
- [ ] runner静态门禁后启动`18-canonical-v2.8-alias-temp0`正式Pi suite.

### 遇到错误

- 首次`/tokenize`经RTK过滤管道被错误统计为0;raw响应证明字段为`tokens`,重新用raw jq/curl得到937.无效的0不作为证据.

**目前完成Rust静态验证后进入Pi复测,不再修改skill prose.**

## [2026-07-28 01:07:18] [Session ID: omx-1784789038072-clve0o] [状态]:Rust门禁完成

- [x] `cargo check --bin rdog`成功;仍报告当前混合工作树既有的6个warning.
- [x] 首次clippy暴露4个历史`never_loop` deny error;两个文件工作树干净,静态控制流确认循环没有continue或第二次迭代.
- [x] 将4个伪循环等价改为单次read+match;PTY相关nextest 21/21通过.
- [x] `cargo clippy --bin rdog`现在0 error、38 warning;未继续修改其余跨模块历史warning.
- [ ] 回读runner execute合同、config与models temperature,确认artifact 18不存在.
- [ ] 执行相同3 prompt的正式Pi多轮suite,不得用direct rdog结果代替Pi评分.

**目前进入最终动态门禁:运行canonical v2.8的temperature 0 Pi suite.**

## [2026-07-28 01:07:18] [Session ID: omx-1784789038072-clve0o] [门禁]:Pi runner执行前检查

- [x] artifact 18不存在;8080 alias、rdog pong、models generation.temperature=0全部确认.
- [x] runner config、canonical symlink、profile tools/skills与双授权逻辑已回读.
- [x] `py_compile`与dry-run通过;计划仍是同一3例,每例一次`rdog-control-bash`.
- [ ] 使用unittest discovery重跑17项runner测试.
- [ ] 测试通过后创建artifact 18并执行,否则停止且不产生GUI样本.

### 遇到错误

- 直接把以`.`开头的文件路径传给`python3 -m unittest`触发`ValueError: Empty module name`;这是测试发现命令错误,不是用例失败.改用`unittest discover -s ... -p ...`.

**目前修正runner测试入口,通过后继续正式执行.**

## [2026-07-28 01:34:25] [Session ID: omx-1784789038072-clve0o] [假设回滚]:canonical v2.8 lane回退为0/3

- [x] runner discovery 17/17通过;artifact 18完成3个正式Pi多轮样本.
- [x] 三例provider/profile/temperature 0和cleanup均通过,但real rdog call与performed step全部为0.
- [x] happy-path转用`open`/`bc`;stale-state转用`clear`/裸表达式;error-result用shell报错文本替代fresh UI.
- [x] "删除本地化映射后直接暴露alias即可保留正确lane"的假设不成立,v2.8必须修正.
- [ ] v2.9只恢复Calculator局部lane guard:UI必须通过`rdog control @ax-press-sequence`执行,shell evaluation无效.
- [ ] 冻结v2.9 hash/token,保持其他条件不变并运行artifact 19.

**目前进行单变量v2.9实验:恢复UI执行边界,不撤回已验证的程序alias.**

## [2026-07-28 01:41:57] [Session ID: omx-1784789038072-clve0o] [假设回滚]:v2.9局部禁令无效

- [x] v2.9 hash`bd844e4390f9e6cc8a2da9a660306b6911b3fbe147f0312171aff784d88031ac`,955 tokens,无holdout泄漏.
- [x] artifact 19完成3例temperature 0 Pi multi-turn,结果仍为0/3且命令与v2.8一致.
- [x] 仅增加"shell evaluation invalid"没有恢复任何rdog call,该局部禁令假设不成立.
- [ ] 程序对复合表达式继续fail-closed,但错误返回输入派生的逐按钮重试hint;先写RED.
- [ ] canonical v2.10恢复v2.7已验证有路由收益的copy-edit结构,并检查sequence timeline最后一步必须是`等于`.
- [ ] 程序和skill分别验证后运行artifact 20.

### 遇到错误

- 首个hint测试补丁因fmt后的断言换行不同而未应用;读取精确片段后重放,没有半应用代码.

**目前进入v2.10前置程序改良:先验证可执行错误反馈,不自动执行表达式.**
