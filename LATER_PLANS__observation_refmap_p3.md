## [2026-05-20 23:18:46] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] 后续计划: scoring conformance 细化

### 背景

- Architect 已 APPROVE P3 主契约,但建议后续补一轮更细的 scoring conformance。

### 后续项

- 为 `rect` / `ax_path` proximity 补 reason code 和 fixture。
- 为 hidden / disabled / unsupported action / kind mismatch / missing hard field 各自补更细 golden 或 focused test。
- 如后续进入 P4 `@observe` 或 P5 mouse ref 化,优先复用本轮 `@selector-refind` 的 scoring contract,不要另造一套恢复评分。
