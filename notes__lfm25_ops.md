## [2026-08-12 00:02:49] [Session ID: omx-1786429420551-ysl4w1] 笔记: upstream Pi v0.84.1 本地构建边界

### 来源
- 上游工作树: `/Users/cuiluming/local_doc/l_dev/my/ts/pi`。
- 固定提交: `53fa77ccd8a279eb87e92294ef3687b03ff80112`。
- 运行命令: `pnpm install --lockfile=false --ignore-scripts` 与 `pnpm exec tsgo --version`。

### 已验证事实
- upstream 根目录只用 `package.json` 的 npm `workspaces` 字段,没有 `pnpm-workspace.yaml`。
- pnpm 10.26.1 输出警告,不会将该字段当成 workspace 定义。
- 根目录 pnpm 安装后没有生成 `node_modules/@earendil-works`,不能拿它构建相互依赖的源码包。

### 采用的最小路径
- 不改 upstream workspace 元数据。
- 在 `packages/coding-agent` 独立安装其已发布依赖,只验证此 package 的构建与后续 `pnpm link --global` 行为。
- 如果未来需要同时改动 `pi-ai`、`pi-agent-core` 等 sibling package,再单独评估 pnpm workspace 适配,不提前引入该配置。

## [2026-08-12 00:50:19] [Session ID: omx-1786429420551-ysl4w1] 笔记: rdog unixpipe client teardown 日志观察

### 动态证据
- 命令: `/Users/cuiluming/local_doc/l_dev/my/rust/rustdog/target/debug/rdog control @ping`。
- 命令: `/Users/cuiluming/local_doc/l_dev/my/rust/rustdog/target/debug/rdog control @capabilities`。
- 两次均走 unixpipe fast path,均在 Zenoh session close 后输出 `Unable to publish transport event: session closed`,但 exit code 为 0 且 `@response` 完整。

### 当前结论
- 已确认的现象: request/reply 功能与 capabilities 可用,错误发生在临时 client session close 日志阶段。
- 候选假设: Zenoh admin transport event 在 session 已关闭后仍尝试 publish。
- 最强备选: daemon side session 生命周期存在实际资源清理问题,只是在当前单请求 smoke 中未影响 response。
- 反证条件: 后续 canary 若出现 rdog response 缺失、session 提前关闭或 control timeout,则不能再把它当作 non-blocking 日志。

## [2026-08-12 00:59:52] [Session ID: omx-1786429420551-ysl4w1] 笔记: upstream Pi v3 JSONL 的最小评分复现

### 动态证据
- 复现文件: `/tmp/pi-rdog-macos-ops-deepseek-20260812-005051/textedit-type-text--canonical-profile/attempt-1/pi-events.jsonl`。
- 当前 `summarize_events()` 输出: `providerRouteVerified=false`, `multiTurnVerified=false`,但已解析出 6 条 rdog command 与 6 个有序 `turn_end`。
- v3 session 只有 `id/cwd`;每个完成的 assistant `message_end` 都携带 `provider=deepseek` 与 `model=deepseek-v4-flash`;每个 `turn_end` 均没有 `turnIndex`。
- 旧 artifact `/tmp/pi-rdog-macos-ops-lfm25-20260810-043637/preview-open-image--canonical-profile/attempt-1/pi-events.jsonl` 仍有 session route 和连续 `turnIndex=0..14`,当前评分为两个 `true`。

### 修复边界
- session 同时缺少 `provider` 与 `modelId` 时,只接受每条完成 assistant message 都精确匹配预期 route 的 v3 数据。
- turn_end 的 turnIndex 要么全部存在且连续,要么全部缺失且至少两个事件;混合或缺失 assistant route 一律失败。
- 修复应留在 `vendor/pi_events.py`,因为 macOS、Calculator、XHS 三个 runner 都调用它。

## [2026-08-12 02:08:42] [Session ID: omx-1786429420551-ysl4w1] 笔记: upstream Pi canary 与 tarball 认证

### DeepSeek 动态结果
- artifact `/tmp/pi-rdog-macos-ops-deepseek-20260812-010240` 的 8 个 case 都满足实际 route、多轮、rdog 调用和 fresh AX/window 验证。
- `safari-navigate-example` 有一次 stale AX path (`code=64`) 后恢复;`safari-new-tab-navigate` 有一次带 pipe 的 rdog bash 工具失败后恢复;`textedit-multi-window` 首次环境阻塞,第二次通过。
- workflow 明确要求将 recoverable error 单独报告,不允许称为零错误认证。

### LFM2.5 动态结果
- 单 case artifact `/tmp/pi-rdog-macos-ops-lfm25-preview-20260812-012323` 的 Preview 最终在 attempt 3 通过。
- 三次尝试出现 `xdg-open`、错误 rdog 语法与 pipe 探索。结果只证明 upstream Pi 对该 provider 的运行链路可用,不改变历史完整矩阵 `1/8` 的能力判断。

### 安装证据
- tarball: `/tmp/pi-coding-agent-upstream-0.84.1-20260812-0123.tgz`。
- SHA-256: `b86ed5626d2cc890e4c42b1f44408904fa84131bbfeeb09e27913195db1e26e5`。
- global package 的 realpath 位于 pnpm `file:/tmp` store,不是 `/Users/cuiluming/local_doc/l_dev/my/ts/pi` 源码目录;从该入口运行 mock provider contract 通过。

## [2026-08-12 14:35:00] [Session ID: omx-1786429420551-ysl4w1] 笔记: upstream Pi 的 models.json 读取和未知字段边界

### 静态证据
- `/Users/cuiluming/local_doc/l_dev/my/ts/pi/packages/coding-agent/src/config.ts` 将 agent 目录解析为 `PI_CODING_AGENT_DIR` 或默认 `~/.pi/agent`,models 路径为该目录下的 `models.json`。
- `/Users/cuiluming/local_doc/l_dev/my/ts/pi/packages/coding-agent/src/core/model-config.ts` 的 `ModelsConfigSchema` 顶层只声明 `providers`。
- `ModelDefinitionSchema` 声明 `samplingParams`,没有 `toolUseProfiles`、`toolUseProfile`、`generation` 或 `repetitionPenalty`。
- `input` 的允许值是 `text` 和 `image`;不存在 `audio`。
- 加载失败时 `ModelConfig.load()` 返回空 provider map 和错误文本,CLI `--list-models` 仍可能以退出码 0 打印内建模型,所以不能只看 exit code 判断配置生效。

### 动态证据
- 原始配置命令:
  `PI_CODING_AGENT_DIR=/Users/cuiluming/.pi/agent /Users/cuiluming/Library/pnpm/pi --list-models`
  输出 `Invalid models.json schema`，指出 `providers.local-gemma4-vlm.models.0.input.2` 的 `audio` 非法;自定义 provider 没有进入列表。
- 临时副本只把该模型的 `input` 从 `["text","image","audio"]` 改成 `["text","image"]`,保留旧顶层和模型字段后执行同一命令,成功列出 `local-gemma4-vlm`、`local-holo31-*`、`local-lfm25-2-6b`、`local-nemotron*` 等自定义模型,stderr 为空。

### 结论和操作方法
- upstream 会读取全局 `~/.pi/agent/models.json`,但评测 runner 用隔离目录,两者不要共用旧 Rust Pi 配置。
- 不支持字段不是迁移机制:它们会被忽略,不会实现工具预选、skill 加载或生成参数。
- 工具和 skill 应通过 CLI 明确传入:
  `PI_CODING_AGENT_DIR=/path/to/agent /Users/cuiluming/Library/pnpm/pi --tools bash,read --append-system-prompt /absolute/path/to/rdog-control/SKILL.md ...`
- 若要迁移全局配置,先备份原文件,生成只含 upstream schema 的独立 `models.json`;本轮不修改原始全局文件。

## [2026-08-12 19:53:48] [Session ID: omx-1786429420551-ysl4w1] 笔记: DashScope Qwen 的 upstream Pi 请求合同

### 现象和证据
- `qwen37-flash` 首轮所有请求被 DashScope 以 `developer is not one of ...` 拒绝;设置 `compat.supportsDeveloperRole:false` 后该错误消失。
- 下一轮返回 `max_completion_tokens [8192] must be greater than thinking_budget [32768]`,说明默认 reasoning 仍在 request 层阻断,还没有模型 token 或 rdog 调用。
- upstream `openai-completions.ts` 的 `thinkingFormat:"qwen"` 会发送顶层 `enable_thinking`;runner 的 `--thinking off` 令其固定为 `false`。

### 已验证修复
- 生产 Qwen 3.6/3.7 provider 都设置 `supportsDeveloperRole:false` 与 `thinkingFormat:"qwen"`。
- runner 的 upstream CLI command 固定 `--thinking off`。
- mock HTTP 回归测试直接加载生产 Qwen 3.7 配置,确认 payload 的 Qwen request 同时含 `system` role 与 `enable_thinking:false`。
- Qwen 3.7 实际单 case 产生 `reasoning:0`,正常工具循环与 fresh AXValue 读回;完整 artifact 为 7/8。

### 复用边界
- 这个契约只适用于当前 DashScope OpenAI-compatible Qwen provider。其他 reasoning 模型不能仅因为 runner 关闭 Pi thinking 就假定其 API 接受或需要相同 `thinkingFormat`。

## [2026-08-12 21:08:00] [Session ID: omx-1786429420551-ysl4w1] 笔记: TextEdit 多窗口契约动态验证

### 现象与静态证据
- 历史 `textedit-empty-doc` setup 在模型前执行 `@key:Cmd+N`,但 prompt 写死“从 1 增加到 2”。历史 artifact 的 `before.windowCount` 实际为 2 至 4。
- retry 的 `osascript quit` 可能超时或被用户取消,`killall TextEdit` 虽返回成功,macOS 仍可能恢复旧未命名窗口。
- 原 verifier 只判断 `after.windowCount > before.windowCount`,与 prompt 的绝对数量要求不是同一合同。

### 修订
- `textedit-window-baseline` setup 不再预建第二个窗口,保存现场实际 N。
- prompt 明确只允许一次 `Cmd+N`,verifier 使用 `after == before + 1`。
- setup cleanup 映射和纯逻辑回归测试同步更新。

### 动态结果
- M3: before 2, after 3,一次 `Cmd+N`,所有必需检查通过,无 tool/rdog 错误。
- Qwen 3.7: before 2, after 3,一次 `Cmd+N`,必需检查通过;有一次可恢复的短格式 `role:AXWindow` 错误。

### 结论
- 两个先前 7/8 的失败 case 在统一基线后都通过,不能再将失败归因于模型能力。
- Qwen 的 recoverable error 仍需保留在认证质量报告中,但不改变本 case 的最终通过状态。
