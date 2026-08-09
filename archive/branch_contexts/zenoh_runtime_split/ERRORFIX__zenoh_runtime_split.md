## [2026-07-18 17:00:00] [Session ID: omx-1784304547353-h5409r] 问题: 模块拆分暴露TMPDIR测试竞态

### 现象

- unixpipe focused测试单独通过,但38项runtime测试默认并发时稳定在`prepare_unixpipe_listener_should_recover_stale_owner_guard_and_files`报`NotFound`.
- 同一组测试改为串行后全部通过.

### 假设与备选解释

- 主假设:local-default测试临时修改并删除进程级`TMPDIR`,unixpipe测试未持同一把mutex,其临时目录在测试执行中被另一线程删除.
- 最强备选:毫秒时间戳目录名碰撞,或production process-lease cleanup误删当前owner目录.
- 推翻主假设所需证据:失败时目录仍存在,或接入同一把环境mutex后并发失败继续出现.

### 已验证原因

- 静态证据:`unique_test_dir()`读取`std::env::temp_dir()`;local-default测试会切换并清理`TMPDIR`;失败测试此前没有参与共享互斥.
- 动态证据:串行38项通过,默认并发可复现;失败测试接入共享`env_test_guard`后,默认并发38项连续5轮全部通过.
- 结论:这是测试隔离竞态,不是生产lease逻辑错误.模块拆分改变调度后才把既有隐患暴露出来.

### 修复

- 把`env_test_guard`和`unique_test_dir`放入唯一的`src/zenoh_runtime/test_support.rs`.
- unixpipe相关测试和process-lease的4项临时目录测试复用同一把`OnceLock<Mutex<()>>`.
- 没有修改生产lease、FIFO cleanup或registry语义.

### 验证

- runtime 38项默认并发连续5轮通过.
- 最终全bin 612 passed / 1 ignored,unixpipe e2e 12 passed,router-client 26 passed / 2 ignored.
- release live smoke的bare/self/显式target ping全部返回pong.

## [2026-07-18 17:00:00] [Session ID: omx-1784304547353-h5409r] 工具错误: 切片与审查脚本范围错误

### Python切片脚本

- 首次session切片脚本把整数offset传给`str.index`,抛出`TypeError`.
- 失败发生时新文件尚未接入模块树,父文件没有删除实现,运行路径未改变.
- 修正为从原父文件按明确marker重新生成,并让shell在脚本失败后立即停止,不再继续rustfmt.

### symbol审查脚本

- 首次集合比较漏纳入`test_support.rs`,误报`env_test_guard`、`unique_test_dir`和`LOCK`缺失.
- 修正后分别比较production、tests与外部调用行,最终三组集合均完全一致.

### grep审查命令

- 首次`unwrap/expect`正则括号未闭合,rg返回parse error.
- 改用fixed-string扫描重新执行,确认production唯一`.expect`为HEAD旧实现已有路径.
