# control_ax 拆分 - Tickets 索引

**总览**: 11 个垂直切片 tickets，按依赖顺序排列

---

## 🎯 可立即开始的 Tickets（无依赖）

- **#01: ax_input 模块基础结构** - 创建最小的 ax_input 模块，验证拆分流程
- **#03: ax_action 路由表基础** - 创建数据驱动的 ROUTES 表和统一入口
- **#07: ObservationCapture adapter 实现** - 创建 observation ↔ query 的接口边界

这三个可以并行工作。

---

## 📋 依赖关系图

```
#01 (ax_input 基础)
  └─→ #02 (迁移 control_actions 到 ax_input)

#03 (routing 表基础)
  └─→ #04 (protocol 层)
      └─→ #05 (execution 层)
          └─→ #06 (迁移 control_actions 到 ax_action)

#07 (ObservationCapture)
  ├─→ #08 (Cache 迁移)
  └─→ #09 (ax_query 创建)
      └─→ #10 (打破循环依赖) ←─── #08

#02, #06, #10
  └─→ #11 (清理与文档)
```

---

## 📊 Tickets 列表（按依赖顺序）

### Phase 1: ax_input（最小，流程验证）
1. **#01: ax_input 模块基础结构** ⚡ 无依赖
2. **#02: 迁移 control_actions.rs 到 ax_input** ← 依赖 #01

### Phase 2: ax_action（最大，核心逻辑）
3. **#03: ax_action 路由表基础** ⚡ 无依赖
4. **#04: ax_action protocol 层 (parse 函数)** ← 依赖 #03
5. **#05: ax_action execution 层 (perform 函数)** ← 依赖 #04
6. **#06: 迁移 control_actions.rs 到 ax_action** ← 依赖 #05

### Phase 3: ax_query + Cache（最复杂）
7. **#07: ObservationCapture adapter 实现** ⚡ 无依赖
8. **#08: AxSnapshotCache 迁移到 ObservationStore** ← 依赖 #07
9. **#09: ax_query 模块创建** ← 依赖 #07
10. **#10: 打破循环依赖** ← 依赖 #08, #09

### Phase 4: 收尾
11. **#11: 清理与文档** ← 依赖 #02, #06, #10

---

## 🚀 建议执行顺序

### 第一批（并行）
- 启动 #01, #03, #07（三个无依赖 ticket）

### 第二批（串行）
- 完成 #01 → 执行 #02
- 完成 #03 → 执行 #04 → 执行 #05 → 执行 #06
- 完成 #07 → 执行 #08 和 #09（可并行）

### 第三批（汇合）
- 完成 #08 和 #09 → 执行 #10

### 第四批（收尾）
- 完成 #02, #06, #10 → 执行 #11

---

## ⏱️ 预计时间

| Phase | Tickets | 预计时间 |
|-------|---------|---------|
| Phase 1 | #01, #02 | 1-2 天 |
| Phase 2 | #03-#06 | 3-5 天 |
| Phase 3 | #07-#10 | 4-6 天 |
| Phase 4 | #11 | 1 天 |
| **总计** | 11 tickets | **2-3 周** |

---

## 📝 关键决策参考

- **ADR-0008**: `docs/adr/0008-control-ax-module-split.md`
- **Spec**: `specs/control-ax-module-split.md`
- **实施计划**: `specs/control-ax-split-implementation-plan.md`
- **CONTEXT.md**: 领域术语定义

---

## ✅ 验收标准（全部完成后）

- [ ] 所有 11 个 tickets 的 acceptance criteria 满足
- [ ] `cargo test` 全量测试通过（至少 796 tests）
- [ ] `cargo clippy -- -D warnings` 无警告
- [ ] 循环依赖已消除（cargo 依赖图验证）
- [ ] 接口深度提升（53 函数 → ~15 函数）
- [ ] 模块文档完整
- [ ] CONTEXT.md 与代码同步
- [ ] 无性能回归

---

## 🎯 下一步行动

```bash
# 查看第一个 ticket
cat .scratch/control-ax-split/issues/01-ax-input-module-foundation.md

# 或者并行启动三个无依赖 ticket
cat .scratch/control-ax-split/issues/01-ax-input-module-foundation.md
cat .scratch/control-ax-split/issues/03-ax-action-routing-table-foundation.md
cat .scratch/control-ax-split/issues/07-observation-capture-adapter.md
```
