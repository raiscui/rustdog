## [2026-07-25 01:30:24] [Session ID: omx-1784789038072-clve0o] 问题: active browser使用截断global快照执行页面定位

### 现象

- active target选中正确focused Chrome,仍返回`AX_WEB_AREA_NOT_FOUND`。
- 同页显式window target能找到唯一入口并完成verified press。

### 原因

- `capture_web_snapshot_with`在没有显式window id时只返回global snapshot。
- global元素预算跨窗口共享,目标Chrome的WebArea未展开。

### 修复

- global snapshot只做active browser消歧。
- 唯一窗口选定后以fresh backend id做targeted capture。
- 选择失败保留global blocker,targeted backend失败回退global。

### 验证

- RED/GREEN、100项相关测试、fmt、cargo check通过。
- live active find 1个匹配,web-act performed/verified。
- Pi v6 scored 5/5成功。

## [2026-07-25 01:30:24] [Session ID: omx-1784789038072-clve0o] 问题: quoted key增加小模型shell截断概率

### 现象

- Pi两轮把命令截断为`rdog control '@key:`,rdog没有收到动作。

### 原因

- 简单key也强制quoted/object payload,跨模型JSON、bash和rdog parser形成多层引号。

### 修复

- 非空且无空白key接受bare payload,例如`@key:Cmd+T`。
- 含空白key仍要求quoted/object,避免语法歧义。

### 验证

- parser RED/GREEN和旧语法回归通过。
- live bare key返回legacy `value:0`。
- Pi v6六轮均生成完整7步命令。
