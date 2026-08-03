# 任务计划: 原型验证 Recording Journal → rdog.flow.v1 编译器

## 目标

提供一个 human 可审阅的 minimal viable compiler prototype,证明相同 Recording Journal 在固定 compiler profile 下生成稳定的 rdog.flow.v1,覆盖 ticket #8 列出的 10 项优化。

## 阶段

- [x] 阶段 1: 任务计划与设置,创建独立分支
- [ ] 阶段 2: 静态证据收集(rdog.flow.v1 schema + Journal 模型 + semantic promotion 规则)
- [ ] 阶段 3: HITL 设计(minimal viable prototype 范围与文件布局)
- [ ] 阶段 4: 写 prototype code + fixture + commit + push
- [ ] 阶段 5: close ticket #8 + update Wayfinder map

## 关键问题

1. prototype 放在哪里(src/bin/ vs examples/ vs 独立 crate)?
2. fixture journal 长度(1 个完整 happy-path 案例 vs 多场景)?
3. 10 项优化各覆盖几个 case?
4. 如何验证 determinism(byte-equal 比较)?
5. prototype 是否依赖已有 daemon 代码(yes - 复用 rdog.flow.v1 step 类型)?

## 做出的决定

- prototype 独立分支:`prototype/recording-replay-compiler` 基于 main b6bc5f8
- prototype 代码:`src/bin/replay-compiler.rs` + `tests/fixtures/recording-compiler/*`
- 不引入新 crate,复用现有依赖

## 遇到错误

- 无

## 状态

**当前在阶段 3**:写 minimal viable prototype 前先确认文件布局与 fixture 范围。

## [2026-07-29 09:30:00] [Session ID: omx-1784512435044-92wxat] [任务完成]: ticket #8 prototype delivered

- [x] 创建独立 git worktree `/tmp/rdog-replay-compiler-prototype` 基于 main b6bc5f8。
- [x] 写 binary `src/bin/replay_compiler.rs` (719 行),实现 11 项 pass。
- [x] 写 fixture `tests/fixtures/replay-compiler/journal_optimizations.jsonl` (28 events)。
- [x] 写 integration tests `tests/recording_replay_compiler.rs` (119 行,6 个 test,全部通过)。
- [x] 写 spec `specs/rdog-recording-replay-compiler-prototype.md` (224 行)。
- [x] git commit 2b59688 on branch prototype/recording-replay-compiler。
- [x] git push origin prototype/recording-replay-compiler。
- [x] gh issue close 8 with full resolution comment。
- [x] Wayfinder map ticket #2 body 追加 ticket #8 entry,放在第一个位置(最新优先)。
- [x] 主 repo AGENTS.md 追加 prototype 索引(保持 dirty,等用户 merge)。
- [x] 范围内dirty worktree未污染(用户的 47 modified + 24 untracked 不动)。
