## [2026-07-27 17:44:06] [Session ID: omx-1784789038072-clve0o] 笔记: canonical native-profile 首轮取证

### 现象

- 正式 Pi profile + Bonsai 8B 的首轮 input 为 18,317 tokens,第二轮 input 为 18,426 tokens,其中 cache read 为 18,313 tokens.
- 首轮真实生成 `bash(command:"rdog control @ping")`,工具返回 `@response "pong"`;第二轮输出连接成功,证明多轮 agent loop 与 tool round-trip 可工作.
- 首轮等待约 340 秒,第二轮 provider streaming 为 7,033 ms.主要性能问题发生在冷 prefill.

### 主假设与备选解释

- 主假设:Pi 全局 `~/.pi/agent/AGENTS.md` 被加入 system prompt,造成首轮 prompt 膨胀.
- 备选解释:canonical skill 正文、profile 规则或工具 schema 本身过大.
- 推翻主假设所需证据:llama-server tokenizer 显示 global AGENTS token 数远小于首轮 input,或者 `build_system_prompt` 不加载 global context.

### 静态证据

- `build_system_prompt()` 在非 `PI_TEST_MODE` 下调用 `load_project_context_files(cwd, global_dir)`.
- `load_project_context_files()` 总是先加载 `global_dir/AGENTS.md`,之后才找 cwd 祖先文件.
- `format_skills_for_prompt()` 只写 skill 名称、描述和路径,不会内联 `SKILL.md` 正文.
- profile 中“it is already loaded into the system prompt at startup”与当前实现不一致;模型必须调用 `read` 才会获得 canonical 正文.

### 动态证据

- 运行中 PID 26636 的 server 命令使用目标 `Ternary-Bonsai-8B-Q2_0.gguf` 与 alias `bonsai-8b-ternary-q2`.
- `/slots` 显示首轮 `n_prompt_tokens=18345`,Pi JSONL 最终 usage 为 input 18,317/output 29.
- llama-server `/tokenize` 结果:
  - `~/.pi/agent/AGENTS.md`:16,841 tokens.
  - canonical `SKILL.md`:3,284 tokens.
  - `rdog-control-bash.appendSystemPrompt`:359 tokens.

### 已验证结论

- 16K 级首轮 prefill 的主因是 Pi 全局 AGENTS,不是 canonical skill正文.全局 AGENTS 约占首轮 input 的 92%.
- 本次 native smoke 证明 profile/model/tool loop可运行,但没有证明模型实际读取了 canonical skill正文,因为该 prompt 已直接给出 `@ping` 命令且本轮没有 `read` tool call.
- `context-preview` 只生成 `provider_calls:0` 的语义 bundle,不能证明 system prompt 或 skill 注入.

### 修复顺序

1. 评测 runner 继续使用正式 Pi agentDir/profile,设置 Pi 已有的 `PI_TEST_MODE=1` 隔离 global project context.
2. 修正 profile 文案,要求 rdog 任务首次使用前通过 `read` 加载 canonical `SKILL.md`;禁止再声称正文已自动内联.
3. 将 canonical `SKILL.md` 改为跨任务短入口,长协议继续保存在已有 references,不创建 8B 或 Calculator 专用 skill.
4. 用 RED 测试禁止 runner 出现 `--append-system-prompt`、`--no-skills` 和 `--tools bash`,再运行 canonical-only 基线.

## [2026-07-27 18:00:30] [Session ID: omx-1784789038072-clve0o] 笔记: canonical 程序能力 RED

### 现象

- canonical skill 已声明 `@ax-find:app:APP,...`、`@ax-press:app:APP,...` 和 `@ax-press-sequence:app:APP,...`.
- canonical 源码尚无 compact selector parser、sequence command variant和唯一可交互 app window resolver.
- 临时验证树还包含 compact `@window-find` response,但 canonical skill 当前并不依赖该能力.

### 主假设与备选解释

- 主假设:skill 与 canonical 程序源码发生能力漂移,导致从 canonical repo 重建后 compact 命令失败.
- 备选解释:现有 object parser 或其他 AX/window path 已能间接接受这些命令.
- 推翻主假设所需证据:canonical parser 对三条命令返回成功,或无需新增 selector 即可通过唯一窗口归属测试.

### 动态证据

- `cargo nextest run -E 'test(parse_should_support_compact_app_scoped_ax_commands)'` 实际运行 1 个测试,退出码 100;第一条 `@ax-find:app:Calculator,AXStaticText` 断言失败.
- `cargo nextest run -E 'test(unique_interactable_window_selector_should_fail_closed)'` 编译退出码 101;四处 E0425 证明唯一窗口选择器尚不存在.
- 两次更早命令没有运行测试:`--exact` 用法错误退出 2;`--lib` 因无 library target 退出 101.

### 已验证结论

- 失败路径确实经过 canonical parser,不是 Pi、skill loader、llama-server 或已安装临时 binary 的问题.
- `app:APP` 必须通过 fresh exact `WindowQuery.app` 解析为唯一且可交互的 canonical `window_id`,再进入 AX 命名域.
- 正式实现不需要同步 compact `@window-find` response,可减少 `control_window/macos.rs` 与既有调用方的改动面.

## [2026-07-27 18:22:32] [Session ID: omx-1784789038072-clve0o] 笔记:canonical AX 回归与整仓 lint 边界

### 动态验证

- `cargo nextest run` 的本轮新增测试集合:9/9 通过.
- 扩大到名称匹配 AX/window 的测试集合:124/124 通过.
- `git diff --check`、`cargo fmt --check` 通过.
- `cargo check` 退出码 0,共有 6 条 warning.
- `cargo clippy --all-targets --all-features` 退出失败,4 个 deny-level `clippy::never_loop` 分别位于 `src/pty_control.rs:395`、`:442` 和 `src/zenoh_control/client_pty.rs:97`、`:137`.

### 归属判断

- 4 个 clippy error 所在文件没有工作树修改,不在本轮 AX/parser/window ownership 调用链内.
- `control_actions.rs` 的 unused import 在 `HEAD` 已存在;当前其他 Session 的 computer-act 改动移除了其 live caller.其余 warning 也位于该脏改动集合.
- 因此不能声称整仓 clippy 通过,但也不能为完成 canonical skill 评测而擅自修改不属于本轮的 PTY/computer-act 代码.

### 当前结论

- canonical AX 新能力已有直接测试与较宽模块回归证据,尚未发现本轮新增编译 warning.
- 下一步必须用 canonical repo 新构建的 `rdog` 做真实 GUI 动态验证;只有 live performed timeline 与 fresh AX 状态才能证明程序合同可用.

## [2026-07-27 18:28:08] [Session ID: omx-1784789038072-clve0o] 笔记:compact open-app 合同漂移与 live GREEN

### 现象

- canonical skill 使用 `rdog control @open-app:Calculator`.
- 新构建 daemon 首次返回 `code:64`,错误为 `@open-app payload 必须是对象,实际收到: Calculator`.
- 因应用没有打开,同轮后续 sequence 与 AX find 都返回 app 匹配 0 个.

### 已验证原因

- 静态证据:`parse_open_app_payload()` 对所有非对象 payload 直接返回 InvalidData.
- 动态证据:新增 `parse_should_accept_open_app_with_compact_bare_app_name` 后精确 nextest RED,失败文案与 live 响应一致.
- 旧 daemon 假设不成立:失败请求由当前 canonical `target/debug/rdog` daemon 处理,其 local-default ready 日志已经取证.

### 修复与回归

- 非对象输入复用共享 `parse_compact_atom`,成功时构造默认 `wait_ms=1500` 的 `OpenAppRequest`.
- quoted、带空白和复杂字段没有被隐式放宽;它们继续使用原有对象格式.
- open-app 测试 14/14 通过.

### live 证据

- compact open-app:`ok:true`,app 为 Calculator.
- press sequence:`performed:true`,6/6 步 target 均属于 `pid:82857/window:0`.
- fresh AX observation:`obs-1785148078110-3`,表达式值为 `3+4÷2`,结果值为 `5`.

## [2026-07-27 18:40:33] [Session ID: omx-1784789038072-clve0o] 笔记:Pi profile skill 装配合同回退

### 现象

- canonical-only Calculator 的两个真实 Pi 样本都没有调用 `read`,分别走向 `xdg-open` 和 shell 表达式.
- 单独扩大 skill description、单独强化 profile 首次 `read` 文案后,相同 prompt 的首个行为都没有改变.

### 静态证据

- `ToolUseProfile.skills` 注释声明 profile 绑定的 `SKILL.md` 会在模型启动时注入 system prompt.
- `extend_with_model_skills` 目前只把对应目录加入 `ResourceLoader.skills`.
- `src/main.rs` 三条 system-prompt 装配路径都调用 metadata-only 的 `format_skills_for_prompt()`.
- formatter 明确要求模型后续使用 `read` 自主加载正文,与 profile 字段合同不一致.

### 动态证据

- RED 测试为 profile 绑定正文写入 `PROFILE_BOUND_BODY_MARKER`,为普通发现 skill 写入 `UNBOUND_BODY_MARKER`.
- 精确 nextest 退出码 101,E0599 说明 profile-aware formatter 尚不存在.

### 已验证结论

- 当前失败不是 canonical skill 正文执行后的质量问题,因为失败轮次没有获得正文.
- 正确修复是恢复 Pi 通用 profile skill 正文装配,同时保持普通发现 skills 为 metadata-only.
- Calculator、Chrome、Bonsai 8B 都不应出现在 loader 实现或 profile prompt 中.

## [2026-07-28 00:29:49] [Session ID: omx-1784789038072-clve0o] 笔记:v2.2 的动作 lane 竞争

### 来源

- suite:`Bonsai-demo/.scratch/pi-bonsai-rdog-calculator/artifacts/11-canonical-v2.2/suite-result.json`.
- 每例 Pi 事件和聚合:`*/pi-events.jsonl`、`*/pi-summary.json`.
- skill identity:`ae3f1ac17870f81f2512c977606bc39e35f39babacc8e77bb3d9f8164c9de766`.

### 现象

- 3 个样本均验证为 Pi 多轮 loop,但成功率为 0/3,performed AX step 均为 0.
- happy-path 的 6 个真实命令全部选择 Keyboard lane,最终 `@key:Equals` 返回 code 64.
- stale-state 的 5 个真实命令全部是 Esc 或刷新快捷键;外部 fresh AX 值为 `0`,证明它清除了旧值但没有计算.
- error-result 的 2 个真实命令都是 Escape 变体;Calculator 最终不存在.
- 所有 bash 命令都保留完整 `rdog control` 前缀,没有 v2.1 的字面占位符.

### 静态对应

- Calculator 段已经明确要求数字和运算符走单个 app-scoped AX sequence.
- 该段之后仍存在独立 `## Keyboard` 章节,含 3 条短小、可直接复制的 `@key` 命令.
- 对 8B 而言,后置短命令比前置自然语言算法更容易成为动作模板.这只是当前主假设,需要 v2.3 A/B 动态证伪.

### 实验控制

- v2.3 只调整 canonical 入口的 lane 竞争,不引入测试表达式、答案、Calculator 专用 skill 或额外 system prompt.
- loader、profile、runner、3 个 prompt、temperature、tool iteration 和 llama-server 保持不变.
- 成功门禁不降低:必须有正确 AX performed timeline、fresh AX 结果和 cleanup;仅停止错误 key 不能算任务成功.

## [2026-07-28 00:36:23] [Session ID: omx-1784789038072-clve0o] 笔记:v2.3 证伪与 v2.4 信息层级

### v2.3 动态结果

- suite:`Bonsai-demo/.scratch/pi-bonsai-rdog-calculator/artifacts/12-canonical-v2.3`.
- canonical hash:`2e968d644074439dfa06cc16fb054751c9c9c3febad4bfa2da3a56d0699b1bca`;tokenizer 1,593.
- 3 例均为 multi-turn,success 0/3,performed AX step 0.
- 生成命令分别暴露三种失败:不完整 compact selector、只完成清零就提前结束、继续把值当 key.

### 假设回滚

- "靠后的 Keyboard 示例是 AX lane 未被选择的充分原因"不成立.
- 推翻证据是 v2.3 已删除整个 Keyboard 语法块,模型仍未生成一个 AX sequence.
- 仍可保留的较弱结论是 Keyboard 示例会增加复制概率,但删除它本身不是完整修复.

### v2.4 设计依据

- 当前入口要求模型跨 Core Loop、Choose One Lane、GUI Ownership、Compact AX、Calculator、Verify 六处拼出一条 native-app 动作链.
- `writing-great-skills` 将这种状态归为 sprawl / sediment,建议把步骤留在入口、参考下沉,并把同一概念的定义、规则和 caveat 共置.
- v2.4 的单变量是信息层级:入口变成 tight loop + 自包含 lane capsule.合法命令和完成条件不再分散.
- 最强备选解释仍是 8B 缺少表达式到按钮描述的组合能力;若短入口仍无 sequence,该解释将获得支持.

## [2026-07-28 00:43:14] [Session ID: omx-1784789038072-clve0o] 笔记:v2.4 lane capsule 动态证据

### 改善证据

- v2.4 入口为 800 tokens;happy-path 生成合法 `@open-app:Calculator`、`@ax-find:app:Calculator,AXStaticText` 和 app-scoped press sequence.
- sequence 返回 `performed:true`,5 个步骤全部固定在同一 `pid:66820/window:0`.
- 这是 canonical-only 评测首次出现真实 sequence 和 fresh AX 读取,因此短入口与概念共置不是纯静态优化.

### 剩余错误

- happy-path 把映射说明中的四个运算符全部当作按钮序列,遗漏所有用户数字,最终 fresh 值为`0÷`.
- stale-state 把 capsule 内的裸 frame `@key:Esc`复制为完整 bash command,三次 exit 127.
- error-result 生成`rdog control @key:加,减,乘,除,等于`,说明模型仍把映射集合当作一个可执行值.
- happy-path 的 AX read 在动作之前,动作后直接虚报结果为7;外部 runner fresh read才揭示真实值.

### v2.5 可证伪合同

- 将表达式定义为左到右一对一 transducer:输入数字原样输出,输入一个运算符只输出对应一个 AX description.
- 删除可被误读为 sequence 的逗号映射集合;每个映射使用独立句子.
- frame 片段不再单独充当 bash 形状;动作调用明确要求完整 `rdog control` 前缀.
- 动作后的 fresh `AXStaticText` 是唯一完成条件.若仍复制整张映射或漏数字,则符号转写假设不成立.

## [2026-07-28 00:48:06] [Session ID: omx-1784789038072-clve0o] 笔记:v2.5 转写假设被推翻

- suite:`Bonsai-demo/.scratch/pi-bonsai-rdog-calculator/artifacts/14-canonical-v2.5`.
- happy-path:`@ax-press-sequence:Calculator,加,乘,等于`;error-result:`@ax-press-sequence:Calculator,除`.
- stale-state 复制`@ax-find:app:APP`与`@ax-press-sequence:app:APP,加,乘,等于`.
- 三例均证明模型能选择实际运算符子集,但不能把"数字原样 emit"落实为输出,也不能稳定替换抽象 APP.
- v2.6 使用更强的预训练操作"copy-edit":先完整复制,再只替换运算符.完成条件显式检查全部原数字仍按原顺序存在.
- Calculator 的 canonical selector 是稳定事实,直接写为`app:Calculator`;这不是 holdout 答案或8B专用 prompt.

## [2026-07-28 00:56:33] [Session ID: omx-1784789038072-clve0o] 笔记:temperature 评测合同缺口

### 静态证据

- runner `load_config`只检查自身 JSON 的`temperature == 0`,但旧`build_pi_command`没有 temperature 参数.
- 正式`models.json`的 Bonsai 8B model 原先没有`generation.temperature`.
- Pi `OpenAIProvider`将 request options temperature 与 compat temperature 合并;两者均空时不发送该字段.

### 动态证据

- v2.6 suite 后`/slots`最近任务参数为`temperature:0.5`.
- 配置修复后,正式 Pi ping smoke 的任务参数为`temperature:0.0`.
- smoke 首轮真实调用`rdog control @ping`,返回`pong`;第二轮只报告`Daemon 在线`.

### 修复与口径

- 正式 Bonsai model entry 的`generation.temperature`是单一运行时真相源.
- runner 新增 test 与 load_config fail-closed校验,避免只在评测JSON里声明却未送入请求.
- v2.0-v2.6 artifact 仍可用于分析复制、占位符和 lane 竞争,但不再作为 deterministic temperature=0 最终比较.

## [2026-07-28 01:00:39] [Session ID: omx-1784789038072-clve0o] 笔记:temperature=0 v2.6基线

- suite:`Bonsai-demo/.scratch/pi-bonsai-rdog-calculator/artifacts/16-canonical-v2.6-temp0`.
- happy-path:`@ax-press-sequence:app:Calculator,1+2*3`,parser因单个item含`*`拒绝.
- stale-state 先生成`8,+,4,*,5`,收到错误后生成`8,加,4,乘,5`;第二次5步全部performed,fresh值`8+4×5`.
- error-result 生成`@cmd:Calculator,1÷0`,没有真实Calculator动作或fresh read.
- 结论:模型在真实错误反馈后能完成数字保留和运算符映射;当前缺口是comma-item结构、括号求值点、最终等于和UI结果来源.
- v2.7 用结构不变量替代更多解释:一个AXButton对应一个逗号item,右括号对应中间等于,末尾对应最终等于.

## [2026-07-28 01:07:18] [Session ID: omx-1784789038072-clve0o] 笔记:v2.7证伪与通用sequence接口实验

### v2.7动态证据

- suite:`Bonsai-demo/.scratch/pi-bonsai-rdog-calculator/artifacts/17-canonical-v2.7-temp0`.
- canonical hash:`4ec013a0eafeb45744133e8a7ccc0a3d55b7ffaf1035d46bdb5d3b81b5bcb35b`;目标 tokenizer 963 tokens.
- happy-path 生成`1,+,2,*,,3,=,`和`1,+,2,*,3,=,`;空 item与`*`均在side effect前被parser拒绝.
- stale-state 先生成`8,+,4,*,5`,随后自修正为`8,加,4,乘,5`;5步performed,fresh值为`8+4x5`,证明窗口归属与顺序正确,但没有结果动作.
- error-result 把`1/0`作为单个item,没有performed timeline或fresh结果.
- 因此"增加一按钮一item、括号和最终等于 prose"没有改变模型行为,该假设不成立.

### 最小可证伪程序实验

- compact sequence 对单字符算术按钮提供固定别名,但不成为表达式parser.
- 精确单item别名可归一化为本机AX描述;已有中文描述保持不变.
- 数字与运算符组成的复合item、空item、尾随逗号继续fail-closed.
- 不自动插入`等于`,不修改app/window唯一归属,不新增Calculator命令.
- 若GREEN后相同Pi样本仍漏最终结果动作,则证明剩余缺口属于模型序列规划,不能再靠这个parser改良解释.

## [2026-07-28 01:07:18] [Session ID: omx-1784789038072-clve0o] 笔记:sequence alias程序层GREEN

### 运行时边界

- 新client连接旧daemon时仍得到旧`*`/`÷`拒绝,证明control parser在daemon进程执行.
- 终止受控session 60885后,使用新binary启动daemon session 93312;同一`mac.lab` fast path恢复`pong`.
- 旧daemon实验没有side effect,fresh值始终为`0`,因此没有污染后续GREEN样本.

### 三例动态结果

- `1,+,2,*,3,=,`:6/6 step performed,全部目标属于`pid:75609/window:0`,fresh结果`7`.
- `8,+,4,=,*,5,=,`:7/7 step performed,同一window ownership,fresh结果`60`.
- `1,÷,0,=,`:4/4 step performed,同一window ownership,fresh结果`未定义`.
- 一个尾随逗号被无歧义忽略;中间空item与双尾随逗号的测试继续拒绝.

### 已验证结论

- 主假设的程序层部分成立:8B自然生成的逐按钮符号可以经过通用alias进入AX动作路径.
- 最强备选尚未排除:模型仍可能漏中间或最终`=`,程序不会替它推断或补动作.
- canonical skill现在应删除本地化运算符映射,直接描述已验证的符号alias和结构不变量,再由相同Pi suite判断agent loop能力.

## [2026-07-28 01:34:25] [Session ID: omx-1784789038072-clve0o] 笔记:v2.8发生Native App lane回退

### 动态证据

- suite:`Bonsai-demo/.scratch/pi-bonsai-rdog-calculator/artifacts/18-canonical-v2.8-alias-temp0`.
- canonical hash:`bb3eea27842299093b60b53cf2038fcb0bf3c7c0cd31e8b12b52d40cca6166c2`;目标tokenizer 937.
- 三例均为正式Pi multi-turn且temperature 0,但真实rdog call为0,performed step为0,success为0/3.
- happy-path使用macOS`open`和`bc`;stale-state使用`clear`并反复执行裸表达式;error-result用shell错误文本代替Calculator fresh值.
- 三例reset后的`@window-find`均`match_count:0`,cleanup完成.

### 结论与下一实验

- v2.8相对v2.7发生明确行为回退,不能作为最终canonical版本.
- 当前主假设是"aliases accepted directly"弱化了UI执行边界,使8B把表达式路由到shell;具体词级因果尚未验证.
- 最强备选是8B对任意短化措辞都不稳定,需要恢复本地lane内的强动作边界.
- v2.9只增加一条局部不变量:Calculator结果必须由`rdog control @ax-press-sequence`驱动UI得到,shell evaluation无效.其余alias、程序、profile和runner不变.
