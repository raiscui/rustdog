## [2026-07-23 16:47:52] [Session ID: omx-1784512435044-92wxat] 错误修复: stale semantic target错误回退旧坐标

### 现象

- 原12个scenario全部通过,但缺少"执行期语义target丢失、坐标guard仍fresh"的组合。
- 最小负向fixture期望reject,实际得到guarded-coordinate,进程以exit code 1报告decision mismatch。

### 原因

- Click和scroll在semantic gate失败后直接进入coordinate gate。
- Coordinate gate只检查window/display/point/verifier,没有检查录制期是否已经存在semantic identity,也没有检查coordinate evidence TTL。

### 修复

- Coordinate gate要求candidate count为0、durable selector为空、captured ref为空。
- Coordinate evidence超过TTL时拒绝。
- Text补齐ownership、capability和parameter id门禁;shortcut补齐owned selector与KeyDelivery门禁。
- 新增`web-click-refind-not-found`常驻scenario。

### 验证

- 13个常驻scenario全部通过。
- 4个额外负向fixture全部fail closed。
- `git diff --cached --check`通过。
- 修复只存在throwaway prototype branch,没有修改生产代码。

## [2026-07-23 17:00:57] [Session ID: omx-1784512435044-92wxat] 错误修复: staged review命令被JavaScript反引号截断

### 现象

- `functions.exec`返回`SyntaxError: Unexpected identifier 'reject'`,nested shell没有启动。

### 原因

- JavaScript template literal中直接放入Markdown反引号,导致字符串提前闭合。

### 修复

- 校验短语不再包含反引号。
- Mermaid fence计数改用`chr(96)`构造,避免wrapper与被测Markdown语法相互干扰。

### 验证

- 失败发生在shell调用之前,不会产生partial staging或文件修改。
- 下一次命令必须重新执行git add、staged diff check和全部policy assertions。

## [2026-07-23 17:04:41] [Session ID: omx-1784512435044-92wxat] 错误修复: Push后的SSH remote核对断线

### 现象

- `git push origin main`成功并报告`main -> main`。
- 随后的独立`git ls-remote`出现SSH remote host断开和Broken pipe,导致整个脚本最终exit non-zero。

### 当前结论

- Commit和push有直接成功输出,但最终remote SHA仍需独立确认。
- 该错误不能被忽略,也不能把本地`origin/main` tracking ref当作新的网络证据。

### 修复计划

- 使用GitHub HTTPS API读取`refs/heads/main`。
- 只有API SHA等于本地HEAD时才发布resolution和关闭ticket。

### 验证结果

- GitHub refs API、commit API和本地HEAD均为`3de8cd631c9a307910829f42f914f09923596f4d`。
- Remote规格提交已确认,可以继续tracker结案。
