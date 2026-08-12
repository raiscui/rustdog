## [2026-08-12 02:08:42] [Session ID: omx-1786429420551-ysl4w1] 修复: upstream Pi v3 JSONL 被误判为路由与多轮失败

### 问题
- upstream Pi v3 的 session 没有 `provider`、`modelId` 和 `turnIndex`。
- 旧共享解析器只读取这些字段,导致已完成 TextEdit 输入并有 fresh AXValue 的 DeepSeek attempt 评分失败,可能触发重复 GUI 操作。

### 原因
- route 实际位于完成的 assistant `message_end` 中。
- v3 的多个 `turn_end` 事件按 JSONL 顺序表达多轮,但不再序列化数值索引。

### 修复
- 有任一旧 session route 字段时,仍要求 session route 完整匹配并要求所有 assistant route 匹配。
- session route 两字段都缺失时,要求至少一条且所有完成 assistant message route 都精确匹配。
- turnIndex 仅允许全部连续,或全部缺失时按至少两个有序 turn_end 验证;混合字段拒绝。

### 验证
- 新增 4 个 parser regression tests。
- 真实 v3 DeepSeek 与旧 LFM2.5 artifact 均回放为 route/multi-turn `true`。
- 47 个相关 Python tests 和 Ruff 检查通过。

## [2026-08-12 14:07:26] [Session ID: omx-1786429420551-ysl4w1] 错误修复: macOS ops daemon Accessibility 身份漂移

### 现象
- MiniMax M3 的完整 8-case artifact `/tmp/pi-rdog-macos-ops-minimax-20260812-185629` 全部是 `environment_blocked`。
- 每个 case 均在模型启动前的 `@window-find` 准备查询返回 `code 77: macOS Accessibility API 当前不可用或未授权`。

### 原因
- 动态证据: 运行中的 daemon PID 55149 映射到历史 `target/debug/rdog` 路径,而评测器控制客户端使用已安装的 `/Users/cuiluming/.cargo/bin/rdog`。
- 同一精确 `@window-find` frame 在旧 daemon 上稳定返回 `code 77`;重启后返回正常 `rdog.window.v1` 结构化结果。
- `@capabilities` 的粗粒度 `available` 状态没有证明实际 AX 查询可用。

### 修复
- 终止 `rdog-daemon` tmux session 中的旧 debug daemon。
- 用 `/Users/cuiluming/.cargo/bin/rdog daemon --config /Users/cuiluming/local_doc/l_dev/my/rust/rustdog/rdog_macos.toml` 重启。

### 验证
- 复跑精确 frame `@window-find#2101:{app:"TextEdit",limit:10,include_state:true,include_recipes:false}`。
- 修复前返回 `code 77`;修复后返回 `rdog.window.v1` 且状态 `complete`。

## [2026-08-12 19:53:48] [Session ID: omx-1786429420551-ysl4w1] 修复: DashScope Qwen 与 upstream Pi 请求不兼容

### 问题
- Qwen 3.7 完整矩阵的 24 个 attempt 全在首请求失败,错误为 `developer` role 不受支持。
- 修复 role 后,真实单 case 又被 DashScope 拒绝,错误为 `max_completion_tokens [8192] must be greater than thinking_budget [32768]`。

### 原因
- upstream Pi 的 OpenAI-compatible provider 默认可使用 `developer` role;DashScope 当前端点只接受 `system`。
- runner 未显式关闭 Pi thinking,Qwen provider又没有声明 `thinkingFormat:"qwen"`,所以 provider 保留默认 32768 思考预算。

### 修复
- 两个 Qwen provider 使用 `compat.supportsDeveloperRole:false` 和 `compat.thinkingFormat:"qwen"`。
- upstream macOS ops CLI 命令固定 `--thinking off`。
- HTTP 合同测试读取生产 `qwen37-flash` entry,断言 `system`、`enable_thinking:false` 与 `bash/read`。

### 验证
- `jq empty agents/upstream/models.json`、`python3 -m unittest -v test_upstream_pi_contract`、`ruff check run_macos_ops_eval.py test_upstream_pi_contract.py` 均通过。
- Qwen 3.7 单 case 修复后有 `reasoning:0`、真实 rdog 多轮与 fresh AXValue 证据。
- 完整 artifact `/tmp/pi-rdog-macos-ops-qwen37-20260812-194613` 成功 7/8,没有 provider 参数错误。

## [2026-08-12 21:08:00] [Session ID: omx-1786429420551-ysl4w1] 修复: TextEdit 多窗口 setup 与 cleanup 契约

### 问题
- setup 预先执行 `Cmd+N`,prompt 却要求初始 1 个窗口;reset 后 macOS 还会恢复旧窗口,导致 M3/Qwen 3.7 都在旧 case 失败。
- 首次引入新 setup 时遗漏 `quit_after_run` 映射,真实执行在 finally 阶段触发 `KeyError: 'textedit-window-baseline'`。

### 修复
- 新增不预建窗口的 `textedit-window-baseline` setup。
- prompt 改成运行时基线 N 到 N+1,verifier 改为精确增量。
- 将新 setup 加入 TextEdit cleanup 映射,补充 setup/评分/cleanup 回归测试。

### 验证
- `python3 -m unittest -q test_run_macos_ops_eval`: 37 tests, `OK`。
- `ruff check run_macos_ops_eval.py test_run_macos_ops_eval.py`: 无问题。
- M3 artifact `/tmp/pi-rdog-macos-ops-minimax-multiwindow-fixed-20260812-210439`: before 2 -> after 3,通过。
- Qwen 3.7 artifact `/tmp/pi-rdog-macos-ops-qwen37-multiwindow-fixed-20260812-210555`: before 2 -> after 3,通过。
