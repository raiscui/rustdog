# local-default legacy退役错误修复记录

## [2026-07-18 13:26:23] [Session ID: omx-1784304547353-h5409r] 问题:旧daemon仍能成为空target/self的正常owner

### 现象

- 纯`rdog.local-default.v1` registry只要PID存活且uplink存在,新版client仍返回pong.
- 没有有效registry时,唯一`*.pipe_uplink`候选仍会被自动选为daemon.
- 旧二进制在新版停止后会unlink三个stable guard path并覆盖回纯v1 registry.

### 假设与验证

- 主假设:正常client仍存在PID fallback和唯一FIFO fallback两条legacy owner路径.
- 最强备选:问题只来自本机残留旧二进制或陈旧状态,代码路径本身已经managed-only.
- 静态证据:`owner_is_active`对无lease metadata记录调用`process_exists`;`find_local_daemon_name`在registry失败后对单FIFO返回`Ok(name)`.
- 动态证据:真实旧版/新版矩阵复现纯v1 ping成功;两条focused RED都在"期望错误,实际成功"处失败.

### 已验证原因

- legacy兼容被放在正常发现路径内,使PID和FIFO存在性继续充当owner身份.
- 这与process lease的单一真相源冲突,也让旧二进制继续处于无感自动发现支持范围.

### 修复

- 无完整lease metadata的local-default记录统一判定为非active owner.
- FIFO扫描保留namespace过滤、排序和候选列表,但任何候选数量都不再自动选择.
- active legacy PID检查保留在`ProcessLease::acquire`,只作为升级安全门.
- 配置、规格和skill统一提示显式target或`local_default = true`恢复路径.

### 验证

- runtime、unixpipe e2e、router-client、all-targets check和release build通过.
- 正式daemon切换保持三个stable lease inode,managed identity一致,三种target ping成功.
- duplicate daemon被service-name lease拒绝,失败前后FIFO inode不变,bare ping继续成功.

### 执行中纠错

- `cargo fmt -- <file>`意外格式化24个此前clean的文件.已按执行前status显式恢复;后续改用直接`rustfmt <file>`.
- `rtk find`忽略原生`-print`.已改用`rtk proxy find`精确过滤,没有清理状态目录.
- duplicate验证首次过度匹配"已存在"文案.实际语义正确但脚本失败;改为验证exit code和重复identity字段后通过.
