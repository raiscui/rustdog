## [2026-07-18 17:00:00] [Session ID: omx-1784304547353-h5409r] 任务名称: zenoh_runtime职责拆分

### 任务内容

- 将1928行的`src/zenoh_runtime.rs`拆为稳定门面与session、unixpipe、local-default、process-lease职责模块.
- 将三个职责模块的单元测试下沉到各自`tests.rs`,避免生产文件再次快速超过1000行.
- 保持现有外部调用路径、公开入口、错误文案、配置优先级、lease语义和运行时行为不变.
- 同步AGENTS索引、rdog-control workflow reference与unixpipe fast-path规格.

### 完成过程

- 先建立外部interface和单向依赖清单,再按session、unixpipe、local-default三个垂直切片逐步搬迁.
- 父门面最终为22行;最大生产模块457行,最大测试模块464行.
- 模块依赖固定为`session -> unixpipe`、`local_default -> unixpipe + process_lease`、`unixpipe -> process_lease`,没有循环import.
- symbol等价脚本确认旧/新production均为46个function、5个type、4个constant;34个测试名与26个外部调用行完全一致.
- 拆分改变测试调度后暴露TMPDIR进程级竞态,通过祖先模块唯一共享的`env_test_guard`完成测试隔离.
- 文档中的3个Mermaid block均通过`beautiful-mermaid-rs --ascii`验证.

### 验证结果

- scoped rustfmt check通过.
- 全bin测试612 passed / 1 ignored;runtime定向测试38 passed.
- unixpipe e2e 12 passed;router-client 26 passed / 2 ignored.
- `cargo check --all-targets`和release build均为0 error;10条warning仍全部来自未修改的control-act基线.
- release与安装版SHA-256一致:`1cb163bd810c710bce67fc36c28a1132605f386df9104d9a33b0c5ab88be25e0`.
- 正式daemon PID 82774继续运行;bare、self、显式`mac.lab`三种ping均返回pong,重复daemon正确拒绝且失败后bare ping仍成功.

### 总结感悟

- 大文件拆分不能只看行数,要先固定调用路径和依赖方向;父门面只应保留稳定interface.
- 测试从单一module拆成siblings后,调度关系会改变.凡是修改`TMPDIR`、`HOME`等进程级环境的测试,必须共享同一把祖先mutex.
- 行为保持重构中发现的既有lint建议应单独记录,不要顺带混入语义清理.
