# 任务计划: Darwin Calculator 控制评测

## [2026-07-26 16:54:32] [Session ID: omx-1784789038072-clve0o] [计划]: 建立隔离实验与测试集

### 目标

用真实 macOS 计算器任务分别评估 rdog 程序能力、`rdog-control` skill 贡献、Bonsai 8B 模型行为和环境边界。只有 fresh AX/window/结果证据齐全时才判定成功。

### 阶段

- [x] Phase 0A: 读取 Darwin 规则、历史评测证据和当前工作树状态
- [x] Phase 0B: 创建独立 worktree、Darwin 分支和基线资产
- [x] Phase 0.5: 设计 3 个测试 prompt,保存并展示验收标准
- [ ] Phase 1: 用户确认后执行 with-skill / baseline 基线评估
- [ ] Phase 2: 每轮只优化一个维度,独立 judge 盲评,仅保留严格增分改动
- [ ] Phase 3: 汇总程序、skill、模型、环境四层结论并收口

### 已验证现场

- `rdog daemon` 正在运行,PID 39508。
- `llama-server` 未运行,端口 8080 空闲。
- 计算器当前未运行。
- rustdog 主工作树含大量其他 Session 修改,禁止 reset、checkout、stash 或清理。
- 当前 `rdog-control` 为 1.8,包含 active-browser capture 和 bare key 的上一轮未提交改动。

### 实验边界

- skill hill-climbing 的唯一可编辑资产是隔离 worktree 内的 `SKILL.md`。
- rdog 程序问题不得伪装成 skill 问题。需要静态调用链与动态失败证据同时成立后,才能进入程序 TDD 修复。
- GUI 测试串行执行。独立 judge 只评审已保存、去标签化的 artifacts。
- 不接受模型文字自述、截图单证据或成功响应作为完成证明。

### 当前状态

**正在执行 Darwin Phase 1**: 用户已确认 3 个测试 prompt。先核实 Pi/runner/rdog 合同,再串行执行 with-skill 与 baseline,最后由独立 judge 盲评已保存 artifacts。

## [2026-07-26 16:58:00] [Session ID: omx-1784789038072-clve0o] [阶段完成]: 隔离环境与测试集就绪

- [x] worktree: `/tmp/rdog-darwin-calculator`。
- [x] branch: `auto-optimize/20260726-165432-rdog-calculator`,起点为 `6973dfa`。
- [x] 当前 1.8 skill 已复制到隔离 worktree,源文件和副本 SHA-256 均为 `ad1ce02518d8cadc19d9f0dee612d562f5dc29686a7f63b2b5bf7d86998ca2cc`。
- [x] `results.tsv` 已初始化,尚无评分记录。
- [x] 测试集覆盖主任务、旧状态恢复、除零错误读取。
- [x] 隔离基线 commit 为 `8da8231e2865273ebee7f2c9ecb96f88759e893f`,worktree 提交后干净。
- [ ] 等待用户确认测试 prompt。确认后才启动 llama-server,并串行执行真实 GUI 基线。

## [2026-07-26 17:06:00] [Session ID: omx-1784789038072-clve0o] [确认]: 进入 Phase 1 基线评估

- [x] 用户确认 calculator happy path、旧状态恢复、除零错误读取 3 个测试 prompt。
- [ ] 核实 Pi 的 Bonsai provider、with-skill 与 baseline 注入边界。
- [ ] 复用并改良已有评测 runner,避免另写一套无法对比的执行逻辑。
- [ ] 启动 8B GGUF `llama-server`,执行 raw tool contract 和 rdog GUI preflight。
- [ ] 串行运行两组实验,每轮保存 Pi JSONL、真实 toolResults、窗口/AX后验和复位证据。
- [ ] 独立 judge 盲评后计算 Darwin 9 维基线分数。

## [2026-07-26 17:22:00] [Session ID: omx-1784789038072-clve0o] [错误]: Calculator runner 首轮 lint 未通过

- `ruff` 报告 F541 无占位符 f-string 和 F401 未使用 import。
- 由于验证命令使用 `set -e`,`unittest`、dry-run 和后续 diff 校验尚未执行,不能提前报告通过。
- 已删除多余 f-string 前缀和未使用 import。下一步从 `py_compile` 开始完整重跑验证链。

## [2026-07-26 17:24:00] [Session ID: omx-1784789038072-clve0o] [错误]: unittest 路径被误解析为模块名

- `py_compile` 与 Ruff 已通过。
- `python3 -m unittest -v .scratch/.../test_run_calculator_eval.py` 把以点开头的路径解析为空模块名,报 `ValueError: Empty module name`。
- 该错误发生在测试加载前,不代表测试用例通过或失败。下一步直接执行测试文件并继续剩余验证。

## [2026-07-26 17:27:00] [Session ID: omx-1784789038072-clve0o] [验证]: Calculator runner 静态门禁通过

- [x] `py_compile` 通过。
- [x] Ruff 通过,无 warning/error。
- [x] 5 个无副作用单测全部通过。
- [x] dry-run 生成 6 个交替条件样本:baseline/with-skill 各 3 个。
- [x] 用户 prompt 只从 `test-prompts.json` 读取;新增 `setup` 和 `expectedResult` 作为结构化验收字段。
- [x] JSON 与 scoped `git diff --check` 通过。
- [ ] 下一步验证 Pi context 中 skill 的唯一变量边界,然后启动 llama-server。

## [2026-07-26 17:32:00] [Session ID: omx-1784789038072-clve0o] [动态验证]: rdog Calculator 程序通路成立

- [x] `@open-app`、唯一窗口发现、AX按钮枚举、语义按键和 fresh 结果读取全部动态通过。
- [x] 主表达式语义按钮路径得到 `1+2×3` 与 `7`。
- [x] 除零路径得到 `1÷0` 与 `未定义`。
- [x] `@paste:"1+2*3"` 仅显示 `1`;该 lane 不得作为成功输入证据。
- [x] Pi `context-preview` 不包含显式 skill 内容,因此不能承担 skill 注入证明;上一验证方向已撤回。
- [ ] 复位 Calculator,启动 llama-server 并执行 server/tool contract preflight。

## [2026-07-26 18:03:00] [Session ID: omx-1784789038072-clve0o] [错误]: RTK find 不支持复合谓词

- `rtk find ... -name A -o -name B` 返回“不支持 compound predicates”。
- server 本身已成功启动,该错误只影响查找既有 tool contract artifact。
- 下一步改用 `rtk proxy find`,不把失败输出当成资产不存在。

## [2026-07-26 18:08:00] [Session ID: omx-1784789038072-clve0o] [验证]: llama-server 与 raw tools 合同通过

- [x] Calculator 复位后窗口数为 0。
- [x] 8B GGUF server 由 tmux session `bonsai-calculator` 启动,PID 65221 监听 127.0.0.1:8080。
- [x] `/v1/models` 只返回 alias `bonsai-8b-ternary-q2`,context 65,536。
- [x] 两个 live raw tools request 均返回 `finish_reason:tool_calls`,bash arguments 可执行。
- [x] 进程参数包含 `--reasoning-budget 0 --reasoning-format none`;响应 `reasoning_content` 为空。
- [x] 识别 runner 安全缺口:异常路径可能跳过 Calculator reset。已在 `run_one` 加入 `finally` 强制复位。
- [ ] 重跑 runner 静态测试后开始 6 个串行真实样本。

## [2026-07-26 18:12:00] [Session ID: omx-1784789038072-clve0o] [错误]: 首次真实 suite 在 prepare 前失败

- 现象:`quit_calculator` 使用尚不存在的 artifact dir 作为 cwd,抛 `Errno 2`;`finally` reset 再次遇到同一问题。
- 动态审计:输出目录只有 `run-plan.json`,没有 `pi-events.jsonl`;无 Pi 进程,Calculator 窗口数为 0。
- 结论:本次是 `environment_not_executed` 的 runner bug,不能计入 baseline 样本或分数。
- 修复:在 `quit_calculator` 入口显式 `mkdir(parents=True, exist_ok=True)`,并新增 mock 回归测试覆盖 early-finally 路径。
- [ ] 完整验证通过后使用全新 output root 从第一个样本重跑。

## [2026-07-26 18:23:00] [Session ID: omx-1784789038072-clve0o] [验证失败]: 首套 with-skill/baseline 接线无效

- happy-path 两组首轮 input 都是 496 tokens,20轮 input/output totals 完全相同,19个 bash 调用逐字相同。
- 两组都生成不存在的 `rdog control -t Calculator -i ...`,没有出现 skill 中的 `@open-app`、`@window-find` 或 `@ax-find`。
- 因此 `--system-prompt` 条件下的显式 `--skill` 没有进入当前 binary 的有效模型输入。已推翻“CLI argv 不同即可证明 skill 注入”的上一假设。
- suite 在 stale-state 后续 prepare 暴露清除键 description 会在“全部清除”与“清除”之间变化;finally reset 已证明 Calculator 窗口数归零。
- 已停止该 suite,所有已运行样本标记为 `invalid_experiment`,不进入 Darwin 分数。
- 修正方案:改用 `--append-system-prompt`,保留 Pi 默认 prompt/skill assembly;清除键接受两种状态;max tool iterations 由24降为12。
- [ ] 先跑 baseline/with-skill `@ping` smoke,只有首轮 input tokens 和行为出现预期差异才重跑 GUI suite。

## [2026-07-26 18:35:00] [Session ID: omx-1784789038072-clve0o] [验证失败]: append-system-prompt 仍未加载 skill

- baseline 与 with-skill `@ping` smoke 首轮 input 都为 915 tokens,total usage 都为 1,974。
- 两组都只调用 `rdog control @ping`,行为和 2-turn 结构完全相同。
- 该证据推翻“仅换 append-system-prompt 即可恢复 skill 注入”的候选修复。
- 新实验接线:复制同一 Bonsai model profile 为 baseline/with-skill 两个 agentDir,只在 with-skill profile 加 `skills:["rdog-control"]` 并放入隔离 SKILL.md;不再使用 `--skill` 或 `--no-skills` flag。
- [ ] 创建两个 agentDir 后,重跑 `@ping` smoke。input 仍无差异时,停止优化并把问题归入 Pi skill binding。

## [2026-07-26 18:47:00] [Session ID: omx-1784789038072-clve0o] [验证失败]: profile skill 绑定只有 6-token 增量

- with-skill agentDir profile 增加 `skills:["rdog-control"]` 后,首轮 input 仅从 888 增到 894 tokens。
- 两组 `@ping` 行为仍相同,没有完整 skill 内容或现代 GUI 命令出现。
- 结论:当前 Pi binary 的隐式 profile skill loader 不是本轮可依赖的有效加载路径。
- 新方案:两个 agentDir models/profile 完全相同,with-skill 直接将隔离 `SKILL.md` 与共同 system prompt 合并到 `system-prompt-with-skill.md`;baseline 只加载共同 prompt。
- [ ] 用 deterministic prompt injection smoke 验证 with-skill 首轮 token 明显增长,再恢复 GUI suite。

## [2026-07-26 18:58:00] [Session ID: omx-1784789038072-clve0o] [验证]: deterministic skill 接线通过

- [x] baseline 首轮 input 为 888 tokens。
- [x] with-skill 首轮 input 为 3,825 tokens,增量 2,937 tokens。
- [x] 两组 `@ping` 都产生 2 个连续 turn、真实 `rdog control @ping` 和 pong,无 tool/rdog error。
- [x] 两个 agentDir 的 models/profile 内容相同;唯一有效输入差异是 with-skill prompt 追加完整隔离 `SKILL.md`。
- [x] Calculator runner 7 个单测、Ruff、py_compile 和 diff check 通过。
- [ ] 使用全新 output root 执行 6 个有效 Calculator GUI 样本。

## [2026-07-26 19:10:00] [Session ID: omx-1784789038072-clve0o] [错误]: stale-state prepare 依赖易变清除键 description

- 有效 suite 前两个 happy-path 已完成,第三个 prepare 发现清除键 description 为“删除”,不是此前观察的“全部清除/清除”。
- finally reset 通过,无 Pi/Calculator 遗留进程。该 suite 因 runner 版本不完整不纳入评分。
- 最小动态实验:打开持久化状态后发送一次 `@key:Esc`,fresh AX 精确返回 `0`,按钮恢复“全部清除”。
- 正式修复:prepare 用 `Esc` 清除并验证 `0`,不再按本地化且随状态变化的清除按钮 description。
- [ ] 重跑静态测试后,用第三个全新 output root 从头执行 6 样本。
