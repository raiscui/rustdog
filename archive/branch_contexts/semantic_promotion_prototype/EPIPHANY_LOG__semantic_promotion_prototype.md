## [2026-07-23 16:47:52] [Session ID: omx-1784512435044-92wxat] 主题: Fresh geometry不能证明stale semantic point仍安全

### 发现来源

- 语义提升prototype的stale target最小反例。

### 核心问题

- Window rect、display topology和point validity全部fresh时,旧坐标仍可能指向dynamic layout中的另一个控件。
- 因此coordinate freshness不能替代semantic target freshness。

### 为什么重要

- 如果compiler把semantic re-find失败解释为允许坐标降级,它会把"找不到原目标"伪装成可执行动作,形成静默误操作。

### 当前结论

- 动态prototype已经证明该错误路径可以发生。
- 推荐的fail-closed规则仍等待human verdict,尚未成为main规格。

### 后续讨论入口

- 先看GitHub ticket"验证语义提升与坐标 fallback 的可行性"及commit `c0d2e01`。

## [2026-07-23 17:07:55] [Session ID: omx-1784512435044-92wxat] 解决记录: 风险已迁入正式规格

- Human已确认fail-closed规则。
- `specs/rdog-recording-semantic-promotion-policy.md`现在是唯一正式真相源。
- Main commit为`3de8cd631c9a307910829f42f914f09923596f4d`。
- 当前ticket已关闭,map已建立decision pointer。
- 该风险不再是未决EPIPHANY;后续实现按正式policy验收。
