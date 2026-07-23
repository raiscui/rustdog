# Recording redaction 错误修复记录

## [2026-07-23 16:45:00] [Session ID: omx-1784512435044-92wxat] 问题: 只读检索命令被 shell 语法影响

### 现象

- 一次只读检索把含反引号的 pattern 放入双引号,zsh 先执行了 command substitution,导致检索内容被改写。
- 一次只读检索使用未匹配的 `src/macos*` glob,zsh 在命令执行前以 `no matches found` 拒绝命令。

### 原因

- 双引号不会抑制反引号的 command substitution。
- zsh 默认对未匹配 glob fail closed,不会把原 pattern 交给下游命令。

### 修复

- 含反引号的 literal pattern 使用单引号,或通过 quoted heredoc 传入。
- 文件枚举先使用 `rg --files` / `rtk find`,再把已存在路径交给检索命令;不直接依赖可能为空的 shell glob。

### 验证

- 后续相关只读检索使用单引号 literal pattern和明确文件路径完成。
- 两次错误都发生在只读命令阶段,没有修改工作区文件或产生生产代码副作用。

## [2026-07-23 16:52:00] [Session ID: omx-1784512435044-92wxat] 问题: GitHub dependency API transient EOF

### 现象

- 批量读取 Wayfinder child dependencies 时,`issues/11/dependencies/blocked_by` 返回 `unexpected EOF`。

### 原因

- 前序 issue 查询均成功,同一 endpoint 单独重试也成功,证据符合一次远端响应中断,不是本地参数或权限错误。

### 修复与验证

- 没有根据不完整输出推断 frontier。
- 单独重试相同 API,成功确认 `定义 Participating Window 与 geometry precondition 编译` 仍被 `验证语义提升与坐标 fallback 的可行性` 阻塞。
