## [2026-07-28 21:50:00] [Session ID: omx-1784512435044-92wxat] 任务名称: 定义 Recording Bundle schema 与原子导出 (ticket #9)

### 任务内容

- 落盘 Wayfinder ticket `#9` 的 resolution asset。
- 整合之前对话里 11 项 HITL 决策(warnings 是第 12 项)+ 远程交付 7 项 + size limit + 不默认录视频。
- 提交严格 scope 限定:`specs/rdog-recording-bundle-schema.md` + `AGENTS.md` 索引追加。
- Push 到 origin/main,关闭 ticket `#9`,更新 Wayfinder map `#2`。

### 完成过程

- 核对 GitHub Issues 状态,纠正 ticket 范围:之前对话里说的"ticket `#12` 远程交付"实际是 ticket `#9` 的"远程下载"部分。ticket `#12` 是已关闭的 macOS 调研 ticket。
- 把所有已经确认的 HITL 决策合并到 ticket `#9` 的单一规格文件,避免重复规格。
- 通过 grep / `rtk git status` / `awk` 校验规格结构、code fence 配对、git status 未污染 dirty worktree。
- 写 commit message 时一次性覆盖 spec 的全部关键点,不让 reviewer 重复读规格。
- Wayfinder map 用 python3 + gh CLI 原子编辑 body,避免半改状态。

### 总结感悟

- 不要在循环里反复重写 commit 标题。如果发现自己停在 "换几个词再写一次",应该先停下来检查 ticket 范围。
- 连续多轮 "继续,按你建议,不要过度设计" 的回复可以采用 "已确认" 假设推进,但需要在 commit message 和 close comment 中明确写出来供人类最后审查。
- ticket 编号的真相源是 `gh issue list`,不是对话里的假设。每次跨 ticket 推进前先核对实际状态。
- 严格 scope commit + Wayfinder map 同步更新是原子操作,不能分开 commit,否则人类会看到不一致状态。
