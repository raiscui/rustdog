# Domain docs

本仓库使用 single-context domain docs 布局。

## 探索前读取

- 根目录存在 `CONTEXT.md` 时先读取它。
- 读取 `docs/adr/` 中与当前任务有关的 ADR。
- 文件不存在时静默继续,不要为了形式完整预建空文件。

`CONTEXT.md` 由 domain-modeling 工作流按需创建。它只保存 rustdog 领域术语和推荐用词,不保存实现细节、规格或任务计划。

## 文件布局

```text
/
├── CONTEXT.md
├── docs/adr/
└── src/
```

## 用词规则

Issue title、规格、重构提案和测试名称应使用 `CONTEXT.md` 中定义的 canonical terms。不要改用 glossary 明确列入 `_Avoid_` 的同义词。

遇到 glossary 没有的术语时,先判断它是不是 rustdog 特有的领域概念。只有领域概念才进入 `CONTEXT.md`;通用编程概念不进入。

## ADR 冲突

输出内容与现有 ADR 冲突时必须明确指出,不能静默覆盖。只有难以逆转、缺少上下文会显得意外、且确实经过取舍的决定,才值得新增 ADR。
