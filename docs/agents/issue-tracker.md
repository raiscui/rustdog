# Issue tracker: GitHub

本仓库的 issues、PRDs 和 Wayfinder maps 均存放在 GitHub Issues 中。所有操作使用 `gh` CLI,仓库由当前工作目录的 Git remote 推导。

## 常规操作

- 创建 issue: `gh issue create --title "..." --body "..."`。
- 读取 issue: `gh issue view <number> --comments`。
- 列出 issues: `gh issue list --state open --json number,title,body,labels,comments`。
- 评论 issue: `gh issue comment <number> --body "..."`。
- 修改标签: `gh issue edit <number> --add-label "..."` 或 `--remove-label "..."`。
- 关闭 issue: `gh issue close <number> --comment "..."`。

GitHub 的 issue 和 pull request 共用编号空间。遇到裸编号时,先用 `gh pr view <number>` 判断,失败后再用 `gh issue view <number>`。

## Pull requests 作为 triage 入口

**PRs as a request surface: no.**

外部 pull request 不进入本仓库的 triage issue 队列。以后如需改变这项约定,直接修改本节。

## Skill 发布和读取约定

- Skill 要求“publish to the issue tracker”时,创建 GitHub issue。
- Skill 要求“fetch the relevant ticket”时,使用 `gh issue view <number> --comments`。

## Wayfinding operations

Wayfinder map 使用一个带 `wayfinder:map` 标签的 GitHub issue。Map body 只保存 destination、notes、已完成决策索引、fog 和 scope 边界。

### Child tickets

- 每个 ticket 是 map 的 GitHub sub-issue。
- Ticket 使用 `wayfinder:research`、`wayfinder:prototype`、`wayfinder:grilling` 或 `wayfinder:task` 标签。
- 优先通过 GitHub sub-issues API 建立关系。
- 如果仓库未启用 sub-issues,则在 map 中使用 task list,并在 ticket body 首行写 `Part of #<map>`。

### Blocking

依赖关系优先使用 GitHub native issue dependencies。创建边时,`issue_id` 必须是 blocker 的数据库数字 ID,不能使用 issue number 或 `node_id`。

```bash
gh api repos/<owner>/<repo>/issues/<child>/dependencies/blocked_by \
  --method POST \
  -F issue_id=<blocker-database-id>
```

如果 native dependencies 不可用,在被阻塞 ticket body 顶部使用 `Blocked by: #<number>` 作为 fallback。

### Frontier 和 claim

- Frontier 是 map 中所有 open、unblocked、unassigned 的 child tickets。
- 没有指定 ticket 时,选择 map 顺序中的第一个 frontier ticket。
- 开始工作前先用 `gh issue edit <number> --add-assignee @me` claim。
- Resolution 必须先写 comment,再 close ticket,最后把一行上下文指针追加到 map 的 `Decisions so far`。
