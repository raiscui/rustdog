# rdog macOS ops eval runner

5 model × 8 case live matrix runner，验证 `@computer-act` outcome / status / epoch 字段
是否被 Pi-driven 真客户端实际消费。

## 状态 (2026-08-09)

- **dry-run 骨架就绪**：5 model + 8 case 配置完整，manifest schema v1，rdogBinary + skill SHA-256 锁定
- **live 跑不动**：5 provider (deepseek / minimax-cn / qwen37-flash / qwen36-flash / minimax-m27-highspeed) 全 HTTP 401，API key 过期，等 user 更新 key 后跑 live

## 用法

```bash
# dry-run (0 风险, 不调 Pi)
bash runner/eval-macos-ops.sh dry all /tmp/rdog-eval-dry

# live 单 model 1 case (mini-test)
bash runner/eval-macos-ops.sh live deepseek /tmp/rdog-eval-deepseek

# live 完整 5x8 (40 run, API key 必须 live)
bash runner/eval-macos-ops.sh live all /tmp/rdog-eval-live
```

`dry` 模式 emit manifest + 验证骨架。`live` 模式起 managed local-default daemon + 调 Pi + 解析 session JSONL → ledger。

## 文件结构

```
runner/
├── config.json                 # 5 model + 8 case + baseline + schema
├── cases/
│   ├── calculator-happy-path.json
│   ├── calculator-old-state-recovery.json
│   ├── calculator-divide-by-zero.json
│   ├── terminal-run-command.json
│   ├── safari-new-tab-navigate.json
│   ├── textedit-multi-window.json
│   ├── clipboard-copy-paste.json
│   └── multi-window-textedit.json
├── lib/
│   ├── daemon_manager.py        # managed local-default daemon lifecycle
│   ├── interaction_ledger.py    # bash command 6 档分类
│   └── runner.py                # main runner
└── eval-macos-ops.sh            # shell 入口
```

## Ledger schema (`rdog.macos-ops.interaction-ledger.v1`)

每条 agent decision (bash tool call) 分类到 6 档之一：

| 分类 | 含义 |
|---|---|
| `query` | 只读协议请求，不是动作后验证或错误恢复 |
| `action` | 含通用状态改变 verb 的请求（`@open-app` / `@key` / `@ax-press` / `@computer-act` 等） |
| `post_action_evidence` | 动作后只读请求（`@observe` / `@ax-find` / `@screenshot`） |
| `recovery` | 紧接同一 attempt rdog/tool 错误后的请求 |
| `supporting_shell` | 非 rdog control bash 调用（`sleep` / `mkdir` 等） |
| `unknown` | 无法可靠归类（不读 app/case/prompt 文本） |

分类只用通用协议 verb + 错误响应 + 相邻请求顺序。

## 关键 invariant

1. **强制 Pi 用 current binary**：archive 5×8 lesson — `PATH="$repo_root/target/debug:$PATH"` prepend
   到 Pi 的 bash tool，SHA-256 写入 ledger manifest
2. **不读 app/case/prompt 文本做分类**：只读 verb + 错误响应 + 相邻顺序，分类器无歧义
3. **同一 bash 多个 control invocation 算 1 request**：跟 archive 一致
4. **maxCaseAttempts=3**：失败 case 自动重试到 3 次，结果计入 `agentDecisions` + `rdogRequests` + `attempts`

## Baseline 对比

| 指标 | archive 2026-08-07 baseline | 目标 |
|---|---|---|
| `agentDecisions` | 260 | < 260 (改善) |
| `rdogRequests` | 252 | < 252 (改善) |
| `attempts` | 41 | < 41 |

current binary 跑 5×8 应该 < baseline 才算改善。

## 加新 model / case

按 user 偏好"用户要求同一 benchmark 的既有与新增模型"一起"比较时, 默认先确认正式命令包含完整目标列表"——加新 model/case 必须：

1. 更新 `config.json` 的 `models` 或 `cases` 数组
2. 加新 case JSON 到 `cases/`
3. 跑 dry-run 验证 manifest 完整
4. 不能假装跑过子集：必须跑完整 5×(N+1) 或 (N+1)×8，不能跳
