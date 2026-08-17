---
title: "upstream Pi macOS ops CLI 合同"
date: 2026-08-13
last_updated: 2026-08-15
module: eval
component: upstream-pi-cli
problem_type: tooling_decision
severity: medium
status: active
tags:
  - upstream-pi
  - macos-ops
  - cli-contract
  - tool-allowlist
  - skill-preload
verified_by:
  - "upstream Pi v0.84.1 commit 53fa77ccd8a279eb87e92294ef3687b03ff80112"
  - "pi-rdog-calculator-eval upstream CLI contract tests"
  - "2026-08-12 tarball mock-provider contract and DeepSeek canary"
  - "2026-08-15 non-PTY successor 2-case rerun with isolated cwd"
  - "2026-08-15 fixed successor policy 3 x 2 matrix"
---

# upstream Pi macOS ops CLI 合同

## Context

macOS ops 评测从旧 Rust Pi 切换到 `@earendil-works/pi-coding-agent` v0.84.1 后,旧 `toolUseProfiles` 的意图必须映射到 upstream 已支持的 CLI 和配置面。本文只覆盖非交互、可复跑评测路径,不定义通用 extension 或 MCP 接入。

## Guidance

- 固定 `/Users/cuiluming/Library/pnpm/pi` 为评测入口,并通过 immutable tarball 或记录过 commit 的构建产物保证版本和来源可追溯。
- 用 `--tools bash,read` 固定工具 allowlist。它是 upstream CLI 的正式入口,不依赖旧 profile 字段或 PATH 中的另一个 `pi`。
- 用 `--append-system-prompt /absolute/path/to/rdog-control/SKILL.md` 预加载 canonical skill。参数值是文件路径,不是 skill 名称;upstream 会读取该文件并把内容追加到 system prompt。
- 用独立 `PI_CODING_AGENT_DIR` 保存评测专用 `models.json` 和状态,把全局 `~/.pi/agent/models.json` 当作用户配置而不是评测真相源。
- `--mode json --print` 的 headless 评测默认不分配 PTY,避免 Pi 继承 stdin 后停在 Node event loop。只有显式验证交互式 stdin 时才使用 PTY。
- canonical skill 的关键失败处理必须在 skill 本体中自包含。隔离 cwd 可能无法解析相对 reference 链接,不能让 agent 通过 `find /` 等无界搜索补齐协议知识。
- `models.json` 只写 upstream schema,顶层使用 `providers`。模型温度等 provider 参数写入 upstream 支持的 `samplingParams`;Qwen 等 provider 的 role/thinking 差异写入对应 `compat` 与 CLI `--thinking off` 合同。
- 不迁移 `toolUseProfiles`、`toolUseProfile`、`generation`、旧 `extensions` 等 Rust Pi 私有字段。未知字段不能实现旧行为,应删除并由显式 CLI 或 upstream schema 替代。

## Evidence

- upstream `/Users/cuiluming/local_doc/l_dev/my/ts/pi/packages/coding-agent/src/config.ts` 将 `PI_CODING_AGENT_DIR` 或默认 `~/.pi/agent` 解析为 agent 目录;`/Users/cuiluming/local_doc/l_dev/my/ts/pi/packages/coding-agent/src/core/model-config.ts` 的 `ModelsConfigSchema` 顶层只接受 `providers`。
- `/Users/cuiluming/local_doc/l_dev/my/rust/pi-rdog-calculator-eval/runner/run_macos_ops_eval.py` 强制 `tools == ["bash", "read"]`,只允许一个存在的绝对 `appendSystemPromptFiles`,并要求 agent 目录隔离和 `models.json`。
- `/Users/cuiluming/local_doc/l_dev/my/rust/pi-rdog-calculator-eval/runner/test_upstream_pi_contract.py` 用 mock provider 检查真实 request 的工具列表和 system prompt payload;全量提交前 runner 测试为 89 passed。
- `PI_CODING_AGENT_DIR=/path/to/agent /Users/cuiluming/Library/pnpm/pi --tools bash,read --append-system-prompt /absolute/path/to/rdog-control/SKILL.md ...` 是已记录的最小 invocation 形态。
- `/tmp/pi-rdog-macos-ops-deepseek-20260812-010240` 的 8-case canary 证明该 CLI、provider、rdog 和 fresh evidence 链路可运行;它只是链路 canary,不是所有模型的完整认证。
- 2026-08-15 的 successor 2-case live rerun 记录了两个失败启动载体: PTY 继承 stdin 无法产出 JSONL,非 PTY 但相对 skill 链接不可读时触发 `find /`。改为非 PTY并将 failure contract 自包含后,两个 case 均生成完整 artifact,最终 AXValue 和 postcondition 均通过。
- `macos-ops-successor-policy-deepseek-2case-20260815` 进一步以相同 Pi launcher、runner、debug binary、canonical skill 和两个 TextEdit case 运行 3 x 2 live matrix。6 个 suite 的 12 个 case 全部通过,且没有再次出现无界文件搜索。这个 artifact 证明修复后的 headless contract 可重复运行,但不替代完整 6 x 8 产品矩阵。

## Why This Matters

旧字段看起来像配置兼容层,但 upstream 不会按这些字段重建工具选择或 skill 注入。继续保留它们会让配置表面上完整,实际 request 却缺工具、缺 prompt 或读取了错误的全局状态,导致评测结果不可解释。

## When to Apply

- 运行或修改 upstream Pi 的 macOS ops、GUI 控制或其它需要固定工具集合和 canonical skill 的 headless 评测。
- 需要把旧 Pi profile 的静态工具和 skill 意图迁移到 upstream CLI 时。
- 需要运行隔离 cwd 的 headless Pi GUI 评测,或排查评测尚未发出首条工具调用时。

## When Not to Apply

- 需要动态修改 prompt、注册运行时工具或接入 MCP 的场景;这些应单独验证 upstream extension/MCP 路径。
- 需要证明模型能力或完整矩阵认证时;CLI contract test 和单次 canary 只能证明接入链路。

## Examples

```bash
PI_CODING_AGENT_DIR=/tmp/pi-rdog-agent \
  /Users/cuiluming/Library/pnpm/pi \
  --tools bash,read \
  --append-system-prompt /absolute/path/to/rdog-control/SKILL.md \
  --thinking off \
  --mode json --print "执行 macOS ops case"
```

评测器还必须固定 `piBinary`、`rdogBinary`、skill SHA-256、provider/model、case 集和 artifact 路径,否则不能把不同载体的结果放入同一认证 baseline。

## Related

- [多窗口 GUI 评测必须使用运行时窗口基线](../logic-errors/macos-ops-multi-window-runtime-baseline.md)
- [rdog macOS ops 交互效率](../../../workflows/macos-ops-interaction-efficiency.md)
- `.codex/skills/rdog-control/SKILL.md`
