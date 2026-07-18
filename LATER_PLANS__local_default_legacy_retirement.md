# local-default legacy退役后续计划

## [2026-07-18 13:26:23] [Session ID: omx-1784304547353-h5409r] 后续:Windows ownership契约迁移

### 当前边界

- 本轮动态证据和managed-only收紧限定在Unix local-default/unixpipe路径.
- 非Unix daemon-name guard仍使用create-new、PID检查和stale unlink,没有迁移到OS生命周期绑定lease.

### 后续措施

- 单独设计Windows named-pipe与进程ownership机制,不要直接复制Unix advisory file lock假设.
- 建立active owner、crash recovery、PID复用和旧版升级矩阵后再替换非Unix分支.
- 同步Windows配置、错误文案和平台测试,明确跨平台能力差异.

### 不在本次交付中的原因

- 当前机器无法提供Windows动态证据;直接改代码只能得到静态候选结论,不满足根因和迁移验证纪律.
