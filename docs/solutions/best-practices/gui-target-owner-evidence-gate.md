---
title: "GUI 坐标 hit-test 必须独立证明目标归属 (owner evidence gate)"
date: 2026-08-22
last_updated: 2026-08-22
module: control
component: gui-targeting
problem_type: best_practice
severity: high
status: active
tags:
  - owner-evidence
  - ax-hit-test
  - foreign-tree
  - wechat-no-ax
  - fail-closed
verified_by:
  - "rg -ni 'copyelementatposition|hit_test|hit-test' src/ -> 0 命中 (坐标 hit-test 发现路径不在 main)"
  - "git log -S 'WeChat' --oneline -- .codex/skills/rdog-control/SKILL.md (92a3d06 移除政策段落)"
  - "git show 92a3d06 -- .codex/skills/rdog-control/SKILL.md (被删政策全文)"
  - "2026-07-14 归属探测记录: EXPERIENCE.md [2026-07-14 23:57:10] 条目 (application-scoped hit-test 18 次 kAXErrorNotImplemented -25208)"
related_solutions:
  - docs/solutions/logic-errors/gui-resource-epoch-read-write-race.md
---

# GUI 坐标 hit-test 必须独立证明目标归属 (owner evidence gate)

## Context

macOS 应用的标准 `AXChildren` 树可能缺少内容节点。一种自然的补树思路是用
`AXUIElementCopyElementAtPosition` 等坐标 hit-test 去发现补充 AX root。2026-07-14
的 WeChat 兼容性实验证明这条路有一个隐蔽的所有权陷阱: hit-test 返回的树可能属于
重叠窗口下的**其它应用** (foreign tree), 而发现链本身完全自洽 - snapshot 写入了
目标 PID、后续 `find` / `get` / stale 拒绝都能通过。当时一次实验把小红书 WebArea
和 Chrome 内容包装成了 WeChat backend ID, "WeChat AX 兼容已动态 GREEN" 的结论
被撤回; 同日复核中两个 WeChat 窗口各 9 次 application-scoped hit-test 均返回
`kAXErrorNotImplemented(-25208)`, system-wide hit 落到前景 VS Code。

## Guidance

1. **请求目标和实际 hit owner 是两份不同状态**, 必须分别采集证据后才允许建立
   关联。"发现成功 + 引用可重放" 只证明候选树可再次定位, 不证明候选树的 owner
   正确。
2. 坐标发现路径至少要保存并交叉验证五类证据, 缺一即 fail closed:
   - hit 元素的实际 PID 和角色;
   - parent 链到达的精确 application/window identity, 以及途中遇到的 foreign boundary;
   - 同一点的 CGWindow owner 与前后 z-order;
   - hit 元素、目标窗口和 display 之间的几何关系;
   - AX 树业务语义与截图中可见内容是否一致。
3. "find 成功 + get 成功 + stale 拒绝正常" 不能当作 owner 验证的替代品 - 它们
   验证的是引用机制, 不是归属。
4. 对坐标发现机制的测试必须包含 foreign 重叠窗口反例, 不能只用单窗口正向
   fixture。
5. **WeChat 临时 no-AX 政策** (基于上述探测的 fail-closed 约束): 对
   `com.tencent.xinWeChat` 的内容定位不使用 AX; 允许路径是 `@window-find` +
   fresh screenshot + `include_ax:false` 观察 + 窗口 rect 复核 + guarded
   coordinate 动作 + 动作后新截图。不重用 `发现` / `直播` / `发布` 等 AX 派生
   ref。重新启用前必须通过受控重叠窗口 owner 回归、真实 `文件传输助手` 命中、
   focused/occluded 多状态重复验证和全链路 fail-closed。

## Evidence

- 坐标 hit-test 发现路径当前不在 main: `rg -ni 'copyelementatposition|hit_test|hit-test' src/`
  返回 0 命中 - 上述实验路径未合入生产代码, 本文档是预防性门禁。
- 政策原文可从 git 历史复原: `git show 92a3d06^:.codex/skills/rdog-control/SKILL.md`
  的 "WeChat Temporary No-AX Policy" 章节 (2026-07-28 被 92a3d06 skill 瘦身
  移除, 提交正文未记录移除意图; 2026-08-22 经用户决策恢复进 SKILL.md v2.28,
  本文档继续保留完整原理、门禁 checklist 与重新启用条件)。
- 2026-07-14 探测的动态证据保留在 `EXPERIENCE.md` [2026-07-14 23:57:10] 条目:
  foreign tree 被包装为目标 backend ID 的失败模式、18 次
  `kAXErrorNotImplemented(-25208)`、撤回 "动态 GREEN" 结论的记录。

## Why This Matters

- GUI 自动化中, 稳定操作错误目标比明确返回 "不可定位" 更危险 - 前者会静默
  点击/输入到其它应用 (可能是浏览器、密码框或另一个会话)。
- 本仓库的 canonical skill 历史上承载过该 fail-closed 政策, 但 skill 是 token
  优化的高频瘦身对象: 2026-07-28 的 dim3 优化在 -209 行时把整个安全章节连同
  普通内容一起删掉, 且无任何意图记录。安全政策放在会被批量裁剪的载体里, 等
  于没有放。政策本体必须沉淀在 docs/solutions/ 这类有索引、有校验的 durable
  载体, skill 只应引用。

## When to Apply

- 未来任何人重新引入坐标 hit-test、AX root discovery 或 "补树" 类机制时。
- 任何 "发现的树/元素要绑定到用户指定目标" 的场景: WeChat 之外的重叠窗口、
  浏览器内嵌页面、多 window 同 app 都适用。
- 评审 GUI 定位 PR 时: 反例测试 (foreign owner) 是必查项。

## When Not to Apply

- 普通的 app-scoped / window-scoped AX 遍历 (走 `AXChildren`) 没有 hit-test
  环节, 不需要本门禁; 它们的归属由 scope 本身保证。
- 用户显式以坐标为目标 (coordinate fallback, 带 `target_resolution.source`
  标记) 的动作不经过 AX 归属, 风险由 display guard 与 rect 复核承担。

## Related

- `docs/solutions/logic-errors/gui-resource-epoch-read-write-race.md` (同属
  "操作与观察交错" 家族: 该文档管 epoch 一致性, 本文档管归属一致性)
- `git show 92a3d06^:.codex/skills/rdog-control/SKILL.md` (被删政策全文)
