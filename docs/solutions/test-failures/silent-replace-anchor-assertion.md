---
title: 脚本化 replace 修改源码必须 assert anchor — 静默未命中的两次事故
date: 2026-08-28
last_updated: 2026-08-30
module: tooling
component: python-source-editing-scripts / multi-session-worktree
problem_type: test_failure
severity: high
status: active
tags:
  - replace
  - anchor
  - assertion
  - source-editing
  - silent-failure
  - multi-session
verified_by:
  - "事故一 (2026-08-27): 跨行正则把 deprecated 属性插到错误结构体 + rindex 找错 mod 结尾, 编译器抓回 (ERRORFIX 2026-08-21 条目)"
  - "事故二 (2026-08-28): test_zenoh_credentials 路径替换静默未命中, 测试 session 用真实 HOME 凭证连隔离 daemon 被 usrpwd 全拒, 排障链五步证伪后由对拍+连接探测锁定 (PR #92)"
  - "对拍诊断测试 diag_pub_reaches 保留于 tests/zenoh_router_client.rs, 可复现判别"
root_cause: "python str.replace 不校验 anchor 是否存在; fmt 重排/并行会话修改会让'当时正确'的 anchor 失配, 替换静默跳过, 脚本却打印成功消息"
resolution_type: "replace 前后强制 assert old in src / new in src; replace 无 assert 版本禁止用于源码编辑"
---

# 脚本化 replace 修改源码必须 assert anchor — 静默未命中的两次事故

## Problem

用 python 脚本对 Rust 源码做文本替换是本仓库高频操作 (fmt 后的微调、批量补丁)。
`str.replace(old, new)` 在 old 不存在时静默返回原串, 脚本照常打印成功,
造成的"以为改了其实没改"在后续以更隐蔽的方式爆炸:

- 事故一 (ax-split): 跨行正则 + DOTALL 把 `#[deprecated]` 插到错误结构体;
  `rindex("}\n")` 把测试插进函数体。
- 事故二 (HOME 隔离): `test_zenoh_credentials` 的 HOME 路径替换静默未命中,
  测试 session 用真实凭证连隔离凭证的 daemon, usrpwd 全拒。
  排障走了五步证伪链 (声明顺序/纯 pub/agent 干扰/sleep 位置/连接探测) 才锁定,
  因为"打印了成功消息"让所有人默认改动存在。

## Guidance

```python
assert old in src, f"anchor missing: {old[:60]}"
src = src.replace(old, new, 1)
```

- 只要有 `assert`, 未命中当场报错, 成本是一次脚本重跑;
- 没有 assert, 成本是被污染的验证结论 + 一条完整排障链。
- 追加/插入类操作同理: `assert anchor in src` 后再 `replace(anchor, new_block)`。

## Evidence

- ERRORFIX.md [2026-08-21] 与 [2026-08-29] 两个条目完整记录现象/证伪链/真因;
- PR #92 排障中, 加 assert 后的脚本在第一次运行即报 anchor missing,
  避免了第三次静默失败;
- 对拍诊断测试 `diag_pub_reaches_mailbox_with_and_without_agent`
  (tests/zenoh_router_client.rs) 保留, 用于快速判别"投递未达"类问题。

## Why This Matters

静默替换失败的三重成本: 验证结论被污染 (测试通过/失败的归因全错)、
排障时间指数级放大 (第二次事故耗掉一个完整会话)、以及多会话并行时
"其他会话改了文件"的天然干扰让 anchor 失配成为常态而非例外。

## When to Apply

- 任何用脚本 (python/sed) 对源码/配置做文本替换的场景;
- 多会话并行工作树 (anchor 随时可能被 fmt 或他人改动);
- 长文件中的跨行模式匹配 (改用行级唯一标识)。

## When Not to Apply

- 生成全新文件 (无 anchor 概念);
- 一次性 throwaway 数据处理 (不碰源码)。

## Related

- `ERRORFIX.md` [2026-08-21] / [2026-08-29]
- docs/solutions/best-practices/parallel-test-global-state-single-lock.md
  (同一多会话并行背景下的隔离纪律)
