# WORKLOG: LFM2.5-2.6B-OptiQ-4bit macOS ops 测试接入

## [2026-08-10 00:58:00] [Session ID: omx-1786268168901-f711dm] 任务: LFM2.5 macOS ops 接入

### 任务内容
- fast-infer 新增 `lfm25_tool_parser.py` (LFM2.5 Gemini 风格 `<|tool_call_start|>[name(args)]<|tool_call_end|>` 解析器, 含 demo 自检)
- `mlx_lm_server.py` 注册 `mlx_lm.tool_parsers.lfm25` 模块 (sys.modules 注入)
- `model_profiles.json` 加 `"*LFM2.5*": "lfm25"` tool_parser_type 映射
- `~/.pi/agent/models.json` 修正 `local-lfm25-2-6b` provider:
  supportsTools=false→true, toolUseProfile=null→rdog-control-bash,
  新增 generation.temperature=0, reasoning=true→false, supportsUsageInStreaming=true→false
- `~/.pi/agent/models.json` rdog-control-bash profile filePathDescription 修正
  (rdog-control.md → rdog-control/SKILL.md)
- `pi-rdog-calculator-eval/runner/eval-macos-ops.sh` 增加 lfm25 条目
- `rustdog/workflows/macos-ops-interaction-efficiency.md` 5→6 模型矩阵表述

### 完成过程
1. 根因分析: mlx_lm._infer_tool_parser 对 LFM2.5 template 返回 None,
   动态验证 (curl 18095 带 tools) 确认 tool_calls=null 原文输出。
2. 写 parser + 注册 + 重启 server (tmux 守护), 动态验证 tool_calls 结构化返回。
3. 修 Pi models.json (先备份 models.json.bak.20260810_lfm25_ops_fix)。
4. 发现 rdog daemon 未运行 (评测前置依赖), tmux 启动 daemon, @ping pong。
5. 手动 probe 1 case (calendar-window-check): 端到端链路打通但模型语法挣扎。
6. runner 真实跑 calendar-window-check: attempt-1 失败 (模型路径幻觉),
   attempt-2 通过 — 完整 rdog 调用链 + fresh 证据 (window title=日历)。

### 总结感悟
- 新模型接入 tool call 前必须先做动态验证: 模型输出格式 vs 服务端 parser 能力。
  mlx-lm 内置 parser 启发式不覆盖所有新模型格式。
- LFM2.5 2.6B 能完成 macOS ops case, 但 attempt 成功率低 (路径幻觉、语法探索),
  完整 8-case 矩阵预计每 case 10-15 分钟, 认证耗时显著高于远程模型。
- 评测前置依赖: LFM2.5 server (tmux lfm25) + rdog daemon (tmux rdog-daemon) 都需运行。

## [2026-08-10 07:35:00] [Session ID: omx-1786268168901-f711dm] 任务: 完整 8-case 矩阵

### 任务内容
- 跑完 LFM2.5 的 macOS ops 完整 8-case 矩阵 (eval-macos-ops.sh lfm25)
- 修复 runner: environment_blocked 不终止 suite (run_one 捕获 EvalError → environment_blocked_result)

### 完成过程
1. 第一次矩阵运行在 textedit-type-text 环境阻塞时 runner 退出 (fail-fast 行为), 8 case 全跑不了。
2. 定位: prepare_case 的 EvalError (environment_blocked_window) 直接冒泡到 main 终止 suite。
3. 修复: run_one 捕获 EvalError, 记 environment_blocked 失败 attempt, 继续下一个 case;
   新增 environment_blocked_result() 构造器 + 回归测试 (28 tests OK)。
4. 重新跑完整矩阵 (~3.5 小时), 8/8 case 完成, suite-result.json 生成。

### 结果 (LFM2.5-2.6B-OptiQ-4bit)
- successCount: 1/8 (preview-open-image, 1/3 attempts, 干净通过)
- 7 case 3/3 失败; 22 attempts 中: 1 passed / 2 env_blocked / 1 任务完成但进程超时
  (calendar title=日历) / 3 无 rdog 调用 / 15 有调用但结果未达预期

### 总结感悟
- 2.6B 弱模型在 macOS ops 上真实能力: 单步任务 (open app + 确认窗口) 偶发成功,
  多步语义任务 (输入文字/导航/新建窗口) 基本失败。
- runner 的 fail-fast 设计适合认证阶段, 不适合矩阵运行期; 环境阻塞应记失败继续。
- LFM2.5 完整矩阵耗时 ~3.5h, 成本显著高于远程模型; 是否纳入正式 6×8 baseline
  需用户决策。

## [2026-08-11 14:50:00] [Session ID: omx-1786429420551-ysl4w1] 任务名称: 为 upstream Pi macOS ops 迁移建立 Wayfinder 地图

### 任务内容
- 读取 LFM2.5 macOS ops handoff、现有任务计划、旧 Pi 配置与 pnpm 全局入口。
- 将迁移范围固定为 upstream `earendil-works/pi` 的 `packages/coding-agent` 开发,目标是恢复 macOS ops 评测链路。

### 完成过程
- 动态确认 `/Users/cuiluming/Library/pnpm/pi` 是 `@earendil-works/pi-coding-agent@0.84.1` 的启动脚本,而非源码工作树。
- 确认上游仓库、包目录和 Node 版本条件,并得到用户对最小迁移范围和 LFM2.5 基线口径的确认。
- 创建 GitHub Wayfinder map #41,建立 research ticket #42,再创建并原生阻塞 #43 与 #44。

### 总结感悟
- 旧 fork 的单测通过不足以证明 CLI 工具注入成立;迁移验收必须查看 `--mode json --print` 的实际 provider request。
- 将上游能力盘点放在 source link 与配置改造之前,可以避免把 `toolUseProfiles` 等旧 fork 私有字段再次带入新实现。

## [2026-08-11 15:50:00] [Session ID: omx-1786429420551-ysl4w1] 任务名称: 完成 upstream Pi macOS ops 迁移的 Wayfinder 决策

### 任务内容
- 将 macOS ops 执行器从旧 `pi_agent_rust` 转向上游 `earendil-works/pi` 的规划路径。
- 保留旧 profile 的预选工具和完整 skill preload 行为,但不保留旧 fork 私有 schema。

### 完成过程
- research ticket #42 用 mock provider 验证 upstream `v0.84.1` headless CLI 的显式 tools、skills 与 extension 注入。
- 决策固定上游 `v0.84.1` / `53fa77c`,工作树位于 `/Users/cuiluming/local_doc/l_dev/my/ts/pi`;开发使用 `pnpm link --global`,认证使用 `pnpm pack` tarball 的全局安装。
- 迁移 runner 的调用契约改为 `--tools bash,read` 和 `--append-system-prompt <canonical SKILL.md>`,使完整 skill 进入 system prompt。
- 创建并收口 Wayfinder map #41 的三个 child tickets #42、#43、#44。

### 总结感悟
- upstream 的 CLI resource surface 已经覆盖静态 profile 的核心意图;单文件 prompt preload 不应额外引入 extension。
- 旧模型 profile schema 与新 Pi 配置 schema 不能混用;应通过 runner config 将 upstream config 与 CLI 参数集中为单一真相源。

## [2026-08-12 00:28:03] [Session ID: omx-1786429420551-ysl4w1] 任务名称: 建立 upstream Pi v0.84.1 可执行开发基线

### 任务内容
- clone `earendil-works/pi` 的固定提交 `53fa77ccd8a279eb87e92294ef3687b03ff80112` 到独立 TypeScript 工作树。
- 用 pnpm 建立 coding-agent 及其最小运行时依赖闭包,生成供后续 `pnpm link --global` 使用的本地 CLI。

### 完成过程
- 首次 clone 的 Git pack 下载中断,通过确认远端 tag 可达并完成 detached fetch 后恢复;没有覆盖全局 pnpm launcher。
- 识别 upstream npm workspaces 与 pnpm 的边界,没有新增 `pnpm-workspace.yaml`;改为在相关 package 内隔离安装依赖。
- `ai` 需要执行上游 model catalog 生成脚本,并在 pnpm 下启用临时 hoisting 兼容 npm 的间接 `@smithy/types` 解析。
- `tsgo` 在 `agent` 的 Node fetch 全局类型上失败,同一 tsconfig 的标准 `tsc` 通过;本轮以 `tsc` 生成 dist 并运行 `node dist/cli.js --version` 验证 `0.84.1`。

### 总结感悟
- 对 npm-only monorepo 使用 pnpm 时,先区分"包本身可运行"和"上游 workspace 工具链可复现"。前者已验证,后者不能因替代编译器通过而被误报为成功。
- 保持开发工作树、全局可执行入口和认证 tarball 三层分离,可以让失败的构建前置不影响正在使用的 Pi。

## [2026-08-12 00:48:05] [Session ID: omx-1786429420551-ysl4w1] 任务名称: 切换 macOS ops runner 到 upstream Pi 显式 CLI 合同

### 任务内容
- 删除旧 fork 的 `toolUseProfiles`、模型 `toolUseProfile`、`generation.temperature` 和不受 upstream 支持的 CLI 参数。
- 用 upstream 原生 `models.json`、`--tools` 与 `--append-system-prompt` 复现评测所需的工具预选和完整 skill preload。

### 完成过程
- 新建 runner v2 config 和无 secret 的 `runner/agents/upstream/models.json`;provider key 只通过已有环境变量解析,模型 temperature 通过 `samplingParams` 固定为 0。
- 发现 mock 启动时 upstream 拒绝三个旧 CLI flags,按 parser 源码与动态错误删除它们;外层 `processTimeoutSeconds` 继续承担 run 上限。
- 用 `pnpm link --global` 把现有 `/Users/cuiluming/Library/pnpm/pi` 链接到固定上游源码的本地 dist。
- 标准库 mock provider 捕获真实请求,证明只有 `bash/read` tools 且 system prompt 含完整 canonical skill 片段。Python lint 和 41 个相关测试均通过。

### 总结感悟
- upstream 配置迁移不能只替换 JSON 字段。完整 CLI argv 必须经过真实进程解析,否则旧 fork 的无效 flags 会在任何 provider 请求之前阻断评测。
- 对静态 prompt 行为,provider request 是最短的动态证据:它同时验证 tool allowlist、prompt preload 和 headless agent-loop 三个关键路径。

## [2026-08-12 02:08:42] [Session ID: omx-1786429420551-ysl4w1] 任务名称: 收口 upstream Pi 的 macOS ops 可执行验证

### 任务内容
- 修复共享 `pi_events.py` 对 upstream Pi v3 JSONL 的评分误判,并新增定向 case 运行能力。
- 完成 DeepSeek 8-case canary、LFM2.5 Preview 观测和全局 tarball 安装认证。

### 完成过程
- parser 保持旧 Pi session route/连续索引的严格分支;v3 仅在全部完成 assistant message route 精确匹配且所有 turn_end 无索引时按自然顺序验证多轮。
- 新增 `runner/test_pi_events.py` 的四个回归样本,以及 `--case` 的正反 CLI 回归测试。
- DeepSeek 新 artifact 为 8/8 功能通过;LFM2.5 Preview 在第三次尝试通过,两者的 recoverable error 都保留在 artifact 中。
- 用 `pnpm pack` 生成固定 tarball,记录 SHA-256,随后 `pnpm add -g` 并用 global Pi 重新通过真实 mock provider 请求测试。

### 验证
- `ruff check` 通过。
- `python3 -m unittest -v test_macos_ops_interaction test_run_macos_ops_eval test_upstream_pi_contract test_pi_events` 通过 47 项。
- global tarball 的 `dist/cli.js --version` 输出 `0.84.1`;mock request 仍只含 `bash/read` 且含 canonical skill 片段。

### 总结感悟
- Pi JSONL 版本升级应按语义证据而非固定 envelope 字段评分,但旧 envelope 的缺失字段必须继续 fail-closed。
- 模型最终完成状态和过程质量是不同指标。recoverable tool/protocol error 需要保留,不能被成功结果抹掉。

## [2026-08-12 14:35:00] [Session ID: omx-1786429420551-ysl4w1] 任务名称: 确认 upstream Pi 的全局 models.json 行为

### 任务内容
- 验证 `/Users/cuiluming/Library/pnpm/pi` 是否读取 `/Users/cuiluming/.pi/agent/models.json`。
- 记录旧 Rust Pi 私有字段在 upstream v0.84.1 中的处理方式和替代命令。

### 完成过程
- 阅读 upstream `model-config.ts` 与 agent-dir 路径解析代码。
- 直接运行全局 Pi 的 `--list-models`,观察到非法 `audio` 导致 schema 错误,但进程退出码仍为 0。
- 在临时副本删除非法 `audio`,保留 `toolUseProfiles`、`toolUseProfile`、`generation`、`repetitionPenalty`,确认自定义 provider 正常列出,证明未知字段被忽略。
- 保持原始 `~/.pi/agent/models.json` 不变,将评测工具预选和 skill preload 固定为 `--tools bash,read` 与 `--append-system-prompt`。

### 总结感悟
- 迁移时应迁移行为,不迁移旧字段名。upstream 的配置真相源是 `providers` + `samplingParams`;工具和 skill 的真相源是 CLI 参数。
- 配置校验错误不能用退出码判断,必须检查 stderr 和自定义 provider 是否实际出现在 `--list-models` 输出中。

## [2026-08-12 14:48:00] [Session ID: omx-1786429420551-ysl4w1] 任务名称: 确认全部 macOS ops 模型切换到 pnpm Pi

### 任务内容
- 检查 6 个模型的 runner 映射和 upstream Pi 配置。
- 修复当前 `rdogBinary` 路径漂移,避免 dry-run 在 Pi 启动前阻断。

### 完成过程
- 确认 wrapper 的 `all` 覆盖 DeepSeek、MiniMax M3、Qwen 3.7、Qwen 3.6、MiniMax M2.7 highspeed、LFM2.5。
- 确认所有模型共用 `runner/config-macos-ops.json`,其 `piBinary` 为 `/Users/cuiluming/Library/pnpm/pi`。
- 将不存在的 debug `rdog` 路径改为实际已安装的 `/Users/cuiluming/.cargo/bin/rdog`。
- 6 个模型的 dry-run 全部通过,并通过 33 项 runner/upstream contract 测试。

### 当前结论
- 现在可以用 pnpm Pi 对全部 6 个模型执行 macOS ops 评测命令。
- 这表示评测链路已可执行,不表示所有模型已经真实 8-case 通过或完成 6 x 8 认证。
- Qwen 两个模型当前缺少 `DASHSCOPE_API_KEY`,真实运行会被凭据前置条件阻断。

## [2026-08-12 13:58:21] [Session ID: omx-1786429420551-ysl4w1] 任务名称: 复核 pnpm Pi 的全模型执行状态

### 任务内容
- 对当前全局 Pi、评测配置、6 模型 dry-run、rdog 与本地 LFM2.5 服务做即时复核。

### 完成过程
- 执行 `/Users/cuiluming/Library/pnpm/pi --version`,确认全局入口为 `0.84.1`。
- 执行 `./eval-macos-ops.sh dry all`,6 个模型均生成完整的 8-case `upstream-cli` 计划。
- 确认共用配置固定使用 `/Users/cuiluming/Library/pnpm/pi` 和隔离的 upstream agent 目录。
- 确认 `rdog 3.0.0`、LFM2.5 health 均正常;随后通过 `direnv exec` 确认 Qwen 凭据存在于 `.envrc.private`。

### 总结感悟
- 全模型 dry-run 证明的是统一 CLI 合同和本地前置条件,不能代替 provider 实际调用或模型能力认证。
- 全部远程模型的凭据已可用;真实认证必须经由已加载 `.envrc` 的 shell 启动。

## [2026-08-12 14:04:53] [Session ID: omx-1786429420551-ysl4w1] 任务名称: 更正 Qwen 凭据状态

### 任务内容
- 复核当前 shell 中的缺失变量是否等同于私有环境文件中没有密钥。

### 完成过程
- `.envrc` 用 `source_env ".envrc.private"` 加载私有配置。
- `direnv exec` 下四个 provider 的变量均已设置,没有泄露值。
- 以该环境执行 Qwen 3.6 与 Qwen 3.7 dry-run,均生成完整 8-case `upstream-cli` 计划。

### 总结感悟
- 凭据检查必须在和实际 runner 相同的 direnv 环境进行。裸 shell 的变量缺失不能作为配置缺失证据。

## [2026-08-12 14:19:18] [Session ID: omx-1786429420551-ysl4w1] 任务名称: MiniMax M3 完整 8-case 认证

### 任务内容
- 使用 pnpm Pi `0.84.1` 对 MiniMax M3 执行完整 8-case macOS ops 矩阵。

### 完成过程
- 首轮 artifact `/tmp/pi-rdog-macos-ops-minimax-20260812-185629` 因旧 debug daemon 的 Accessibility 身份漂移全部 environment_blocked,未计入模型结果。
- 停止旧 daemon,使用 `/Users/cuiluming/.cargo/bin/rdog` 重启后,相同 `@window-find` probe 恢复为正常 `rdog.window.v1`。
- 重跑 artifact `/tmp/pi-rdog-macos-ops-minimax-20260812-190844`,8 个 case 完成,7 个通过。

### 结果
- 通过: `textedit-type-text`, `calendar-window-check`, `safari-navigate-example`, `preview-open-image`, `terminal-window-check`, `terminal-run-command`, `safari-new-tab-navigate`。
- 失败: `textedit-multi-window`,3 次均有真实 route/fresh evidence,但模型发现实际起始窗口数不是 prompt 声称的 1,因此没有执行不满足前置条件的动作。

### 总结感悟
- 权限 daemon 必须与评测使用的稳定安装二进制保持一致;只看 `@capabilities` 摘要不够,必须用实际 AX 查询验证。
- MiniMax M3 已证明 7 个通用 case 可用;多窗口 case 需要单独审查 setup/prompt 契约。

## [2026-08-12 21:08:00] [Session ID: omx-1786429420551-ysl4w1] 任务名称: 修订并重新认证 TextEdit 多窗口 case

### 任务内容
- 修订 `textedit-multi-window` 的 setup、prompt 和窗口数量验收契约。
- 对 MiniMax M3 与 Qwen 3.7 只重新运行该 case,不重复其它 7 个 case。

### 完成过程
- 历史 artifact 显示 setup 在模型开始前已经发送 `Cmd+N`,但 prompt 仍要求 `1 -> 2`;重试期间 TextEdit 窗口还会因 macOS 恢复行为累积。
- 新增 `textedit-window-baseline` setup,让 `before` 保存现场实际窗口数 N;模型只负责一次 `Cmd+N`。
- verifier 改为严格要求 `after == before + 1`,并将新 setup 加入 finally cleanup。
- 真实重跑中 M3 和 Qwen 3.7 都首轮从 2 个窗口增加到 3 个窗口并通过 fresh window evidence。

### 结果
- MiniMax M3 artifact: `/tmp/pi-rdog-macos-ops-minimax-multiwindow-fixed-20260812-210439`,通过,无 tool/rdog 错误。
- Qwen 3.7 artifact: `/tmp/pi-rdog-macos-ops-qwen37-multiwindow-fixed-20260812-210555`,通过,含一次可恢复 rdog 短格式错误。

### 总结感悟
- 原来的 7/8 不能直接解释为模型能力失败;统一 setup 基线后两个模型都完成了相对窗口增量。
- 多窗口验收必须验证精确增量,否则“动作多建了窗口”会被宽松的 `after > before` 掩盖。

### 长期经验沉淀
- Compound Gate 结论为 capture;新增 `docs/solutions/logic-errors/macos-ops-multi-window-runtime-baseline.md`。
- 文档已通过 frontmatter 与 claims 校验,`AGENTS.md` 已增加在修改多窗口 case 前的读取入口。

## [2026-08-12 20:12:48] [Session ID: omx-1786429420551-ysl4w1] 任务名称: 完成未认证模型的 upstream Pi macOS ops 8-case 矩阵

### 任务内容
- 使用 `/Users/cuiluming/Library/pnpm/pi` 与稳定安装的 `/Users/cuiluming/.cargo/bin/rdog` 依次认证 MiniMax M3、Qwen 3.7、Qwen 3.6、MiniMax M2.7 Highspeed。
- 修复 DashScope Qwen 的 upstream Pi 请求 role 和 thinking 参数兼容,并保留每个模型的独立 artifact。

### 完成过程
- 修正 `runner/agents/upstream/models.json`:移除误加在 M2.7 的 `supportsDeveloperRole:false`,为 Qwen 3.6/3.7 增加 `supportsDeveloperRole:false` 与 `thinkingFormat:"qwen"`。
- 修改 `run_macos_ops_eval.py` 的 upstream CLI 命令,固定 `--thinking off`;更新 HTTP 合同测试断言 `system` role、`enable_thinking:false` 和生产 Qwen 配置。
- Qwen 3.7: 单 case 修复后通过,完整 7/8;唯一失败 `textedit-multi-window` 三次未满足 expected window count。
- Qwen 3.6: 单 case 和完整矩阵通过,最终 8/8;多窗口 case attempt 2 通过。
- MiniMax M2.7 Highspeed:完整 8/8,全部 attempt 1 通过。

### 证据路径
- M3: `/tmp/pi-rdog-macos-ops-minimax-20260812-190844`
- Qwen 3.7: `/tmp/pi-rdog-macos-ops-qwen37-20260812-194613`
- Qwen 3.6: `/tmp/pi-rdog-macos-ops-qwen36-20260812-195617`
- M2.7 Highspeed: `/tmp/pi-rdog-macos-ops-m27hs-20260812-200338`

### 总结感悟
- DashScope 的 Qwen OpenAI-compatible endpoint 不接受 `developer` role,且默认思考预算会大于模型输出上限;必须用 provider compat + `--thinking off` 的真实 request 合同修复,不能只依赖模型名称或退出码。
- `textedit-multi-window` 的结果受现场窗口累积影响。认证报告要保留 attempt 级 fresh evidence,不能把一次通过或一次失败扩展成稳定能力结论。

## [2026-08-12 21:58:00] [Session ID: omx-1786429420551-ysl4w1] 任务名称: upstream Pi 与 Rust Pi 对话轮数口径核验

### 任务内容
- 对比 `/Users/cuiluming/Library/pnpm/pi` 的 upstream Pi v0.84.1 与旧 `pi_agent_rust` 的 macOS ops JSONL 事件和 agent-loop 语义。
- 区分 provider/assistant 回合、工具执行数、`turnIndex` 记录字段和 `maxToolIterations` 上限。

### 完成过程
- Rust Pi MiniMax M3 的历史 artifact 显示 `turn_end.turnIndex` 连续从 0 开始。日历 case 是 4 回合/3 次工具执行,Safari 新标签 case 是 14 回合/13 次工具执行。
- upstream Pi v0.84.1 保留 `turn_end`、assistant `message_end` 和 `tool_execution_end`,但 JSONL v3 CLI 输出没有 `turnIndex`,按有序 `turn_end` 计数即可。
- 同一多窗口 case 的历史 Rust Pi 样本为 7 回合/6 次工具执行,修订 runtime-baseline 契约后的 upstream Pi 样本为 4 回合/3 次工具执行。

### 总结感悟
- 两套实现都遵循 assistant -> tool -> tool result -> assistant 的循环。事件字段不同,循环语义没有本质差别。
- 由于 case 契约已经修订,两次样本不能用来归因“upstream Pi 必然少 3 回合”。后续若需要性能结论,应锁定同一模型、prompt、skill、case 版本和运行环境后 A/B 复跑。

## [2026-08-13 00:00:00] [Session ID: omx-1786429420551-ysl4w1] 任务名称: 提交 upstream Pi macOS ops 迁移

### 任务内容
- 分别提交评测器的 upstream Pi 迁移实现,以及 Rustdog 的多窗口契约、经验沉淀和任务账本。
- 排除本机状态、临时目录与未完成交接文件。

### 完成过程
- `pi-rdog-calculator-eval` 提交 `228e57f feat(macos-ops): migrate evaluator to upstream pi`。
- `rustdog` 提交 `docs(macos-ops): record upstream pi migration`。
- 提交前完成 89 个 Python 测试、Ruff、Python 编译、JSON syntax 和 `git diff --check`。

### 总结感悟
- 评测器配置与 Rustdog 的迁移说明必须分仓库提交,避免 consumer implementation 与 producer 文档混在同一个历史节点。
- `runner/agents/upstream/auth.json` 与 `models-store.json` 是空的本机状态文件,不应进入版本库。
