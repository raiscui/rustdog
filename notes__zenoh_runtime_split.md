# zenoh_runtime职责拆分研究笔记

## [2026-07-18 16:16:59] [Session ID: omx-1784304547353-h5409r] 笔记: 模块依赖与interface清单

### 当前结构

- `src/zenoh_runtime.rs`共1928行,生产代码到1061行,其后是34项inline单元测试.
- 已有`src/zenoh_runtime/process_lease.rs`,说明父文件 + 同名目录的子模块布局已经是仓库现有模式.
- 外部调用只通过`crate::zenoh_runtime::<symbol>`或`process_lease`路径进入,没有调用者依赖新的子模块名.

### 外部interface

- session:`open_router_session`、`open_client_session`、`resolve_client_connect_endpoints`、`UnixpipeClientProbe`.
- unixpipe:`compose_listen_endpoints`、`ComposedListenEndpoints`、`prepare_unixpipe_listener`及Unix path/probe helpers.
- local-default:`register_local_default_daemon`、`LocalDefaultDaemonGuard`、`find_local_daemon_name`.
- process lease路径已经被`zenoh_control/target_resolve.rs`直接使用,本轮保持不动.

### 单向依赖设计

```text
zenoh_runtime facade
├── session ──────────────> unixpipe
├── local_default ────────> unixpipe + process_lease
├── unixpipe ─────────────> process_lease
└── process_lease
```

- `session`只读取unixpipe路径、存在性和locator.
- `local_default`只复用组件校验与base存活判断,不被unixpipe反向调用.
- 父模块继续重导出现有公开符号,调用者路径保持不变.

### 测试布局决定

- 不创建单一巨型`tests.rs`.
- session、unixpipe、local-default测试分别跟随实现放在各文件的`#[cfg(test)] mod tests`.
- 父模块保留一个共享环境变量mutex,避免拆分后不同测试模块并发修改`TMPDIR`.
- 该布局减少`pub(super)`测试泄漏,预计三个新文件均低于1000行.

### 工具边界

- CodeGraph成功识别公开entry points与调用文件,但对大文件私有symbol召回不完整;后续读取已定位文件的完整生产段与测试名清单补齐.
- 当前环境没有Context7工具.本轮不新增依赖或调用新library API,只搬迁现有Rust实现.

## [2026-07-18 16:20:21] [Session ID: omx-1784304547353-h5409r] 笔记: 拆分前动态基线

- HEAD:`d9e0013`.
- runtime相关单元共38项:父模块34项 + process lease 4项.
- integration:unixpipe 12项,router-client 26 passed / 2 ignored.
- `cargo check --all-targets`为0 error;10条control-act warning是既有基线.
- live fast path由PID 19047持有,裸ping成功.

## [2026-07-18 16:24:40] [Session ID: omx-1784304547353-h5409r] 笔记: session切片证据

- `session.rs`包含session open、autodiscovery locator排序、client endpoint解析和3项原测试.
- `UnixpipeClientProbe`仍由父模块重导出,`zenoh_control.rs`调用点无需改动.
- session只依赖父层提供的三个unixpipe helper;下个切片会改为直接依赖`super::unixpipe`,不形成反向依赖.
- 完整runtime过滤从旧`zenoh_runtime::tests`改用`zenoh_runtime::`,因此同时覆盖session、父模块和process lease共38项.

## [2026-07-18 16:29:54] [Session ID: omx-1784304547353-h5409r] 笔记: unixpipe切片与并发证据

- `unixpipe.rs`持有path推导、FIFO cleanup、path lease、probe和listener composition,只依赖`process_lease`.
- 父层只重导出真实调用者使用的`compose_listen_endpoints`与`prepare_unixpipe_listener`;没有为未使用的历史`pub`项添加allow warning.
- 分模块改变了测试调度顺序,暴露旧`prepare_unixpipe_listener`测试未参与TMPDIR互斥的问题.
- 串行通过 + 默认并发稳定失败 + 加共享mutex后默认并发5/5通过,三类动态证据共同确认测试隔离原因.

## [2026-07-18 16:33:49] [Session ID: omx-1784304547353-h5409r] 笔记: local-default切片与门面收口

- local-default测试13项保持原断言和错误文案;生产实现只改文件位置与import路径.
- `zenoh_runtime.rs`现在只声明5个内部模块并重导出当前真实调用者使用的interface.
- 旧`pub`项若没有父层调用者,留在private子模块或降为`pub(super)`;没有用`#[allow(unused_imports)]`掩盖门面泄漏.
- AGENTS索引与workflow source anchor需要从单一父文件更新为`src/zenoh_runtime/`职责目录.

## [2026-07-18 16:37:36] [Session ID: omx-1784304547353-h5409r] 笔记: 最终文件布局

- `zenoh_runtime.rs`:22行interface门面.
- `session.rs` 254行 + `session/tests.rs` 42行.
- `unixpipe.rs` 383行 + `unixpipe/tests.rs` 335行.
- `local_default.rs` 457行 + `local_default/tests.rs` 464行.
- `process_lease.rs` 445行;`test_support.rs` 28行.
- 总行数从原生产+lease 2368增至2430,增加部分主要是模块imports、模块文档和共享测试隔离helper;没有复制生产逻辑.

## [2026-07-18 16:49:31] [Session ID: omx-1784304547353-h5409r] 笔记: symbol与interface等价审查

- production symbol集合完全一致:46个function、5个type、4个constant,无缺失、无新增、无重复.
- tests symbol集合完全一致:42个function、1个constant;34个测试名称逐项相同.
- 原文件内的共享测试锁与临时目录helper已移动到`test_support.rs`,没有丢失.
- 仓库外部`zenoh_runtime::`调用行在HEAD与当前工作树均为26行,集合及顺序完全一致.
- 除父门面和process-lease测试互斥外,没有修改任何生产调用文件;外部调用路径保持原样.

## [2026-07-18 16:54:18] [Session ID: omx-1784304547353-h5409r] 笔记: clean-code与Clippy审查

- 门面22行,只声明5个内部模块并重导出8个现有入口;26个外部调用行全部命中这些入口.
- 生产依赖为`session -> unixpipe`、`local_default -> unixpipe + process_lease`、`unixpipe -> process_lease`,无反向import和环.
- 所有Rust文件低于1000行;顶层5个文件,三个测试子目录各1个文件.
- 新模块没有TODO、FIXME、dead-code allow、unused allow或占位实现.
- production中的唯一`.expect("socket_path checked above")`在HEAD旧文件已有,本轮没有新增panic路径.
- 全仓Clippy退出101:4个deny级`never_loop`均在未修改的`pty_control.rs`和`zenoh_control/client_pty.rs`;当前diff没有触碰这些文件.
- zenoh_runtime仅报告3条普通建议:`local_default.rs`原有布尔表达式、`unixpipe`测试原有`repeat().take()`两条同源建议. 为保持纯重构,本轮不混入语义清理.
