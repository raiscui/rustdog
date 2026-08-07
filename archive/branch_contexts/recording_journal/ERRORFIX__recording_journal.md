## [2026-07-22 12:12:53] [Session ID: omx-1784512435044-92wxat] 错误修复: 文档 diff 与 Mermaid 长任务验证

### 现象

- `rtk git diff -- AGENTS.md specs/rdog-recording-journal-model.md` 返回 `fatal: bad revision 'AGENTS.md'`。
- 首个 Mermaid 验证在 10 秒内没有可见输出,一度被怀疑为空渲染。

### 假设与验证

- diff 主假设是 RTK git 子命令错误解析 `--` 后的 pathspec。改用 `rtk proxy git diff -- ...` 后立即得到正确 `AGENTS.md` diff,假设成立。
- Mermaid 初始假设是图结构触发空渲染。分段实验显示 2、5、10、15 行前缀均 `exit=0` 且输出非空;19 行进程只是超过 tool yield window,因此上一假设不成立。

### 修复

- path-scoped diff 改用 `rtk proxy git diff -- <paths>`。
- 长 Mermaid 渲染使用 PTY session 轮询,直到进程真正结束,不把空的首轮 yield 当作命令输出。
- 等待此前启动的渲染进程自然结束并确认无残留,没有并发修改规格文件。

### 验证

- 完整首图最终返回 `exit=0`,Unicode 输出为 `13704` bytes。
- 第二个 sequenceDiagram 返回完整 Unicode 时序图。
- `rtk proxy git diff -- AGENTS.md ...` 正常输出 path-scoped diff。
- 两次工具问题都没有修改或损坏仓库内容。
