被跳过的 `test (<os>)` 说明这次运行对该 OS **没有测试证据** —— 这不是"测试失败", 也
不是"通过"。按失败步骤的注解归因:

| 注解 | 结论 |
| --- | --- |
| `onnxruntime dist unavailable` / `onnxruntime dist digest mismatch` | 外部依赖缺口 (基础设施), 不是本改动坏了东西 |
| `onnxruntime runtime DLL missing` | ort-sys 的 dylib 铺放前提变了 (见 `ci-merge-gate.md` §4) |
| `build prep failed` | 准备阶段异常 (工具链 / registry / lockfile), 同样没有测试证据 |
| 其它 | 编译 / 代码问题, 按真实失败处理 |

判读口径: `docs/agents/ci-merge-gate.md` §3

> 这段文字为什么是文件而不是 `echo` 行: 2026-11-26 的覆盖缺口演练里, 直接写在
> `echo "..."` 双引号里的反引号被 bash 当命令替换执行了 (`line 9: onnxruntime:
> command not found`), 摘要表因此变成坏行。文件内容不经过 shell 解析, 同一份文字也不再
> 在三个 job 里各抄一遍。
