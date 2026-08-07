## [2026-07-26 17:22:00] [Session ID: omx-1784789038072-clve0o] 问题: Calculator runner 首轮 Ruff 失败

### 现象

- F541: `run_calculator_eval.py` 中固定 `@window-find` 字符串误写成 f-string。
- F401:单测文件导入 `tempfile` 后没有使用。

### 原因

- 初版 runner 从需要插值的相邻 frame builder 复制了 f-string 形式。
- 单测设计收口后删除了临时目录 case,但遗漏 import。

### 修复

- 固定 frame 改为普通字符串。
- 删除未使用的 `tempfile` import。

### 验证计划

- 重新运行 `py_compile`、`ruff check`、完整 `unittest`、dry-run、JSON 和 diff 校验。

## [2026-07-26 17:24:00] [Session ID: omx-1784789038072-clve0o] 问题: unittest CLI 路径解析失败

### 现象

- `python3 -m unittest` 收到 `.scratch/.../test_run_calculator_eval.py` 后报 `ValueError: Empty module name`。

### 原因

- `unittest -m` 的 positional argument 按模块名解析,以 `.scratch` 开头会产生空模块前缀。

### 修复

- 改为直接运行 `python3 test_run_calculator_eval.py -v`。

### 验证计划

- 确认 5 个无副作用测试实际执行,再继续 dry-run 与 diff 校验。

## [2026-07-26 18:03:00] [Session ID: omx-1784789038072-clve0o] 问题: RTK find 复合谓词不受支持

### 现象

- `rtk find` 对多个 `-name` 通过 `-o` 连接的命令报“不支持 compound predicates”。

### 原因

- RTK 的 find 过滤器仅支持简单查询,复合原生 find 表达式需要 passthrough。

### 修复

- 使用 `rtk proxy find` 保留原生 find 语义并继续记录 token 使用。

### 验证

- 重跑后必须列出真实 contract 文件或明确返回空结果。

## [2026-07-26 18:12:00] [Session ID: omx-1784789038072-clve0o] 问题: Calculator reset artifact cwd 未创建

### 现象

- 首次 `--execute` 在任何 Pi 调用前抛 `No such file or directory: .../reset`。
- artifact 只有 run plan,没有 Pi JSONL;Calculator 保持关闭。

### 原因

- `quit_calculator` 假设 artifact dir 已由先前 rdog 调用创建。
- `prepare_case` 的第一步和 `finally` early-failure 路径都不满足这个假设。

### 修复

- `quit_calculator` 自己创建 artifact dir,让函数拥有其 cwd 前置条件。
- 添加 mock 单测,从不存在的目录调用 reset 并断言目录创建。

### 验证计划

- `py_compile`、Ruff、6 个单测和 dry-run 全部通过后,用新 output root 重跑真实 suite。

## [2026-07-26 18:23:00] [Session ID: omx-1784789038072-clve0o] 问题: Darwin with-skill 条件未产生 skill 输入差异

### 现象

- baseline 与 with-skill 首轮 input 都是 496 tokens。
- 两组完整命令序列和 token totals 完全相同,都重复错误的旧式 CLI 参数。

### 主假设与备选解释

- 主假设:当前安装 Pi binary 在 `--system-prompt` override 路径没有有效拼入显式 skill。
- 备选解释:显式 skill 加载器忽略了该文件,与 system prompt 无关。
- 反证标准:改用 append 后首轮 input 仍相同,则主假设被推翻,继续调查 skill loader。

### 修复

- system prompt 改用 `--append-system-prompt`,让默认 assembly 保持主导。
- 在真实 GUI 重跑前增加 baseline/with-skill ping smoke,直接比较首轮 input tokens 和工具行为。
- Calculator 清除键接受“全部清除”或“清除”。

### 验证计划

- 静态测试通过后运行两个 ping smoke。只有 with-skill 输入明显大于 baseline 且无加载诊断错误时,才恢复 6 样本 GUI suite。

## [2026-07-26 18:35:00] [Session ID: omx-1784789038072-clve0o] 问题: append-system-prompt 接线仍无 skill 增量

### 现象

- 改用 `--append-system-prompt` 后,baseline/with-skill 的首轮 input 仍为 915 tokens,工具行为完全相同。

### 原因候选

- 当前 Bonsai profile `rdog-control-xhs-bash` 没有 `skills` 绑定,显式 `--skill` 在安装 binary 上可能不生效。

### 修复

- 创建 baseline 与 with-skill 两个独立 agentDir。
- 保持模型、provider、tool profile append 文案相同,仅为 with-skill profile 增加 `skills:["rdog-control"]` 和隔离 skill 文件。

### 验证计划

- 运行 agentDir 绑定后的 ping smoke,比较 input tokens、首个命令和 skill 现代命令出现情况。

## [2026-07-26 18:47:00] [Session ID: omx-1784789038072-clve0o] 问题: profile skill 绑定未加载完整内容

### 现象

- with-skill profile 只有 6-token 首轮增量,没有 `SKILL.md` 的内容级增量。

### 原因候选

- 当前 Pi binary 对 profile `skills` 只注入名称/短元数据,或模型 profile 的 skills 字段未走完整 loader。

### 修复

- 去除两个 agentDir 的 profile skill 差异。
- runner 生成 `system-prompt-with-skill.md`,显式拼接完整隔离 `SKILL.md`。

### 验证计划

- 通过 token 增量和后续现代 rdog 命令证明 deterministic with-skill 接线有效。

## [2026-07-26 19:10:00] [Session ID: omx-1784789038072-clve0o] 问题: Calculator 清除键 description 存在第三状态

### 现象

- stale prepare 看到按钮 description 为“删除”,标签白名单仍无法定位清除动作。

### 原因

- Calculator 根据当前输入/结果状态动态切换同一位置的语义,AX description 不是稳定 selector。

### 修复

- 改用已动态验证的 `@key:Esc` 清除。
- 清除后必须 fresh AX 精确观察到 `0`,否则 fail closed。

### 验证计划

- runner 静态验证通过后,完整 suite 从头运行,不拼接不同 runner 版本的样本。
