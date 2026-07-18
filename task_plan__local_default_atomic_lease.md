# 任务计划: local-default 原子 lease状态源

## [2026-07-18 10:54:47] [Session ID: omx-1784304547353-h5409r] [计划]: 原子lease可行性与实现

### 目标

让service-name、canonical unixpipe path和local-default选择都使用可原子声明、由OS进程生命周期释放、可验证资源identity的lease语义,避免仅靠PID存活造成复用误判和双文件状态分裂.

### 两个方向

1. 最佳方案:使用进程持有的非阻塞独占文件锁作为ownership事实,guard文件写入统一JSON lease记录;local-default registry原子替换并引用同一lease identity,进程退出后锁由OS自动释放.
2. 过渡方案:继续使用PID文件,但增加schema、随机lease_id、进程启动时间和原子rename;减少写入中断,仍保留PID探测与平台特化风险.

### 当前决策

- 优先验证方向1.文件锁比PID探测更直接地证明"当前是否仍有进程持有资源",不会因PID复用把无关进程当owner.
- 在确认项目Rust版本、标准库API、macOS/Linux语义和v1迁移路径前,不修改生产代码.

### 阶段

- [ ] 阶段1:读取当前三类guard/registry实现、规格与项目Rust版本约束.
- [ ] 阶段2:最小跨进程实验验证文件锁的互斥、崩溃释放、记录可读性和文件删除语义.
- [ ] 阶段3:完成lease schema、资源identity、迁移与启动/退出时序规格.
- [ ] 阶段4:TDD实现共享lease抽象并迁移service/path/local-default调用路径.
- [ ] 阶段5:运行单测、e2e、router-client、编译与真实重复启动/崩溃恢复smoke.
- [ ] 阶段6:更新长期记录并做独立scoped commit.

### 关键问题

1. 当前工具链的 `std::fs::File` 是否已有稳定的 `try_lock` / `unlock` API,项目MSRV是否允许使用?
2. macOS/Linux上锁是否随进程异常退出自动释放,guard文件保留时新进程能否安全接管?
3. local-default client如何在不持锁的情况下验证registry引用的lease仍活跃?
4. 是否需要保留旧 `*.pid` / `lab.json` 读取兼容,以及何时清理旧文件?
5. path owner、service-name与local-default是三个资源key,如何共享lease schema而不错误合并冲突域?

### 验证纪律

- 现象:当前实现以PID存活和多个文件组合推断owner,已存在状态分裂历史.
- 主假设:OS advisory lock可替代PID存活成为ownership真相源,同时保留JSON元数据用于诊断.
- 最强备选:标准库锁API不满足MSRV或跨平台语义,需要引入经过验证的锁crate或采用原子rename过渡方案.
- 推翻主假设的证据:第二进程能同时拿锁、异常退出后锁不释放、只读client无法无副作用验证锁状态,或锁文件删除导致双owner.

### 状态

**阶段1进行中**:读取工具链、Cargo约束、当前guard代码和现有规格.

## [2026-07-18 10:57:07] [Session ID: omx-1784304547353-h5409r] [状态更新]: 标准库锁与冲突域确认

### 已验证事实

- 本机工具链为Rust 1.96.0,项目固定stable但没有声明 `rust-version`.
- `std::fs::File::{lock,try_lock,try_lock_shared,unlock}` 从Rust 1.89.0稳定.
- 官方标准库源码说明Unix当前映射到 `flock`,Windows映射到 `LockFileEx`;所有关联handle关闭时锁自动释放.
- 当前三类guard使用不同resource key:service `(namespace,daemon_name)`,path canonical base,local-default namespace.
- 因冲突域不同,不能错误合并成单一锁路径;应共享lease引擎/schema,每个资源key保留独立锁文件.

### 阶段状态

- [x] 阶段1:读取当前实现、规格和Rust版本约束.
- [ ] 阶段2:跨进程验证互斥、异常退出释放和unlink语义.

### 状态

**阶段2进行中**:编译临时标准库锁probe,不修改仓库生产代码.

## [2026-07-18 10:59:26] [Session ID: omx-1784304547353-h5409r] [状态更新]: 跨进程文件锁实验通过

### 动态证据

- 活跃exclusive holder存在时,第二进程exclusive probe返回 `WouldBlock`.
- 同一状态下shared probe也返回 `WouldBlock`,可用于只读client判断exclusive owner活跃.
- holder被SIGKILL后,新进程在同一锁文件上立即获得exclusive lock.
- holder活跃时unlink锁路径,新进程会创建新inode并成功拿锁;实验inode从512118831变为512118834.

### 已验证结论

- OS文件锁能覆盖PID复用和异常退出释放,主假设成立.
- 锁绑定inode而不是路径;持锁期间或Drop时删除锁文件会破坏互斥,正式实现必须永久保留lock file.
- 元数据更新不能通过rename替换被锁文件;local-default需要稳定lock file + 原子metadata record,两者职责必须单一.

### 阶段状态

- [x] 阶段2:跨进程验证互斥、异常退出释放和unlink语义.
- [ ] 阶段3:完成lease schema、资源identity、迁移与时序规格.

### 状态

**阶段3进行中**:设计稳定lock file与原子metadata record的兼容契约.

## [2026-07-18 11:03:04] [Session ID: omx-1784304547353-h5409r] [状态更新]: lease规格完成

### 规格决策

- stable PID file只承载OS lock与旧版兼容PID,不得unlink.
- metadata通过同目录temp + sync + rename发布,不替换被锁inode.
- local-default保持v1核心字段,lease字段作为旧版可忽略扩展.
- managed record由lock判断liveness;legacy record继续回退PID + uplink.
- 新旧版本并发迁移使用PID内容保护旧daemon,新版本用lease metadata识别已释放的受管owner.

### 验证

- flowchart与sequenceDiagram均通过 `beautiful-mermaid-rs --ascii`,exit 0.
- 同一进程两个独立handle实验返回 `SAME_PROCESS_BLOCKED`.

### 阶段状态

- [x] 阶段3:完成lease schema、resource identity、迁移和时序规格.
- [ ] 阶段4:TDD实现共享lease抽象并迁移三条调用路径.

### 状态

**阶段4进行中**:先修改生命周期断言跑RED,再实现共享lease模块.

## [2026-07-18 11:05:20] [Session ID: omx-1784304547353-h5409r] [状态更新]: lease生命周期测试进入RED

### RED证据

- 两个精确nextest均失败,整体 `0 passed,2 failed`,exit 100.
- path测试失败点:`owner lease file应该永久保留`.
- local-default测试失败点:`namespace lease file应该保留`.
- 失败发生在旧Drop删除文件的断言,不是测试环境或编译错误.

### 实现范围

- 增加共享 `ProcessLease`:stable lock file、managed metadata、legacy PID迁移、active shared probe.
- Unix service-name、path owner、local-default namespace迁移到共享lease.
- local-default JSON增加可选lease字段并改为原子写;client按managed/legacy两条路径验证.
- client不再删除任何stable lock file.
- Cargo声明Rust 1.89最低版本,与标准库lock API稳定版本一致.

### 状态

**阶段4进行中**:开始生产代码实现,完成后先重跑同两项RED测试.

## [2026-07-18 11:12:47] [Session ID: omx-1784304547353-h5409r] [状态更新]: 三类guard迁移进入GREEN

### 已完成实现

- 新增共享 `ProcessLease`,使用stable lock inode、exclusive/shared probe、legacy PID兼容和原子JSON发布.
- canonical path guard已迁移,Drop后文件保留并可重新接管.
- local-default namespace guard与registry已迁移,managed记录用lock判断活跃,legacy记录保留PID fallback.
- Unix service-name guard已迁移;Windows继续使用原路径,未扩大未验证平台语义.
- `Cargo.toml`声明 `rust-version = "1.89"`.

### 当前GREEN证据

- process lease单测:3 passed.
- path RED测试:1 passed.
- local-default RED测试:1 passed.
- runtime + process lease:35 passed.
- shared explicit path e2e:1 passed.
- duplicate daemon e2e:1 passed.

### 状态

**阶段4继续**:增加SIGKILL后三类lease残留的真实接管e2e.

## [2026-07-18 11:15:43] [Session ID: omx-1784304547353-h5409r] [状态更新]: 实现与扩大回归通过

### 动态验证

- SIGKILL崩溃恢复e2e:1 passed,确认service/path/local-default三类artifact残留时可接管并恢复ping.
- runtime + process lease:35 passed.
- 完整unixpipe e2e:12 passed.
- 短路径隔离router-client:26 passed,2 skipped.
- `cargo check --all-targets` exit 0,本轮新增warning已清零,仅保留既有control-act基线.

### 阶段状态

- [x] 阶段4:TDD实现共享lease抽象并迁移三条调用路径.
- [ ] 阶段5:代码审查、build/install与真实迁移/重复启动/崩溃恢复smoke.

### 状态

**阶段5进行中**:审查最终diff与结构影响,然后切换真实daemon.

## [2026-07-18 11:17:54] [Session ID: omx-1784340333160-6bwnss] [继续]: 阶段5生产审查与live迁移验证

### 恢复点

- 上一Session已经完成阶段1到阶段4,其记录作为背景参考.
- 本Session从阶段5继续,重新读取生产diff并用新验证证据确认结论.

### 本轮审查清单

- [ ] 检查metadata原子发布的崩溃一致性,包括rename后的目录持久化边界.
- [ ] 证伪metadata发布失败后同进程重试是否会被自己写入的legacy PID阻塞.
- [ ] 检查managed与legacy识别是否会误判旧daemon或无关PID.
- [ ] 明确旧二进制仍会unlink稳定锁路径时的新旧版本并存边界.
- [ ] 完成格式化、focused tests、完整unixpipe e2e、router-client、check与build.
- [ ] 安装当前二进制,完成旧daemon拒绝、新daemon重复启动、ping和SIGKILL接管live验证.
- [ ] 更新支线记录、主线摘要并创建显式文件清单的scoped commit.

### 当前状态

**阶段5生产审查中**:先审查`ProcessLease`与三条调用路径,不在静态证据不足时修改代码.

## [2026-07-18 11:21:30] [Session ID: omx-1784340333160-6bwnss] [状态更新]: metadata发布失败RED已确认

### 验证结果

- [x] metadata发布失败后同进程重试缺口已由精确单测复现.
- [ ] local-default registry与lease ID未绑定的候选误判仍待RED验证.

### 遇到错误

- 第一次追加笔记的`apply_patch`缺少有效hunk锚点,工具拒绝且未产生文件改动;读取文件尾部后重新应用.

### 当前状态

**阶段5生产审查中**:继续做local-default lease ID关联的最小证伪实验.

## [2026-07-18 11:24:27] [Session ID: omx-1784340333160-6bwnss] [状态更新]: 两项生产审查RED均成立

### 验证结果

- [x] metadata发布失败会留下self-blocking legacy PID.
- [x] local-default旧registry会在不同lease ID但相同PID的active lock下被误判为当前owner.

### 修改范围

- 改良共享`ProcessLease`,让未提交lease自动回滚原lock内容.
- 为local-default拆分lease sidecar与业务registry,客户端做完整lease identity关联.
- 补齐rename后的父目录sync并同步规格中的迁移边界.

### 当前状态

**阶段5修复中**:按两项RED实施单一修复,随后先重跑两个精确测试.

## [2026-07-18 11:25:35] [Session ID: omx-1784340333160-6bwnss] [错误记录]: GREEN首次编译被可变性错误阻断

- `publish_metadata`改为更新lease提交状态后,两个测试仍使用不可变`first`绑定.
- 编译器报告2个`E0596`;测试尚未执行,不能声称GREEN.
- 处理:只把对应测试绑定改为`mut`,不改变生产逻辑,然后原样重跑两个精确测试.

## [2026-07-18 11:26:26] [Session ID: omx-1784340333160-6bwnss] [状态更新]: 两项审查回归进入GREEN

- metadata发布失败回滚测试:`1 passed`,确认同进程可立即重试.
- local-default lease ID关联测试:`1 passed`,确认旧registry不能冒充不同lease owner.
- [x] metadata原子发布的普通I/O失败回滚已验证.
- [x] managed registry的lease identity关联已验证.

### 当前状态

**阶段5 focused验证中**:扩大到`process_lease`、`zenoh_runtime`和target resolve相关测试.

## [2026-07-18 11:27:42] [Session ID: omx-1784340333160-6bwnss] [状态更新]: focused suite通过并进入scoped格式/规格验证

- `zenoh_runtime` focused suite:37 passed.
- target resolve focused suite:2 passed.
- 崩溃e2e已补充service/path/local-default三个`.lease.json` sidecar存在性断言.

### 遇到错误

- `cargo fmt --all -- --check`被本任务外control-act工作线的既有格式差异阻断;禁止运行全仓自动格式化,改为只检查并格式化本任务Rust文件.
- Mermaid fence搜索命令在双引号中包含反引号,触发shell command substitution并报`command not found`;命令未写文件,后续统一使用单引号pattern.

### 当前状态

**阶段5规格与格式验证中**:scoped rustfmt、两个Mermaid代码块语法验证、diff复审.

## [2026-07-18 11:28:52] [Session ID: omx-1784340333160-6bwnss] [状态更新]: scoped格式与规格语法通过

- 本任务5个Rust文件已单独运行rustfmt并通过二次`--check`.
- `git diff --check`通过.
- flowchart与sequenceDiagram均经`beautiful-mermaid-rs --ascii`成功渲染.
- 规格已记录sidecar关联、目录sync、失败回滚和旧版并发迁移边界.

### 当前状态

**阶段5完整回归中**:重跑runtime suite、完整unixpipe e2e和router-client短路径.

## [2026-07-18 11:31:02] [Session ID: omx-1784340333160-6bwnss] [状态更新]: 编译回归与旧daemon兼容门通过

### 自动验证

- runtime:37 passed.
- unixpipe e2e:12 passed.
- router-client:26 passed,2 ignored.
- `cargo check --all-targets`:exit 0.
- `cargo build --package rustdog --bin rdog`:0 errors.
- 现有warning仅来自未改动的control-act基线,lease路径无新增warning.

### live前置证据

- 旧安装版PID 69053由`rdog-daemon` tmux持有,UDP 7447正在监听.
- 旧安装版bare `@ping`返回`pong`.
- 新构建版面对活跃旧daemon时exit 1,按legacy PID guard拒绝重复service-name.

### 当前状态

**阶段5安装迁移中**:备份旧二进制、安装工作树,保持旧daemon运行并复验互操作.

## [2026-07-18 11:34:15] [Session ID: omx-1784340333160-6bwnss] [状态更新]: 正常迁移与新daemon重复启动验证通过

### 安装与迁移证据

- installed binary与`target/release/rdog` SHA-256一致;旧安装版已备份到`/tmp/rdog.pre-atomic-lease-20260718-1131`.
- 安装后旧PID 69053继续ping成功;installed新版本面对旧daemon仍按legacy guard拒绝.
- Ctrl-C后旧PID、UDP 7447和tmux session全部释放.
- 新lease daemon PID 39323 ready,service/path/local-default三把stable lock及三个sidecar、registry共7个artifact存在.
- `jd`比较local-default sidecar与registry引用字段exit 0且无diff.
- 三类sidecar的resource kind/key、PID正确;重复启动exit 1,前后bare ping均返回pong.

### 遇到错误

- JSON展示管道误把stdin传给只接受文件参数的`rtk json`,exit 127;改用`jq`直接读取后字段输出正常,不影响此前`jd`比较结果.
- Ctrl-C后tmux server自动退出,首次组合检查末尾capture返回`no server running`;随后分别检查PID、UDP端口和tmux,三项均确认释放.

### 当前状态

**阶段5 SIGKILL live验证中**:杀死PID 39323,检查artifact/inode并同identity重启.

## [2026-07-18 11:38:15] [Session ID: omx-1784340333160-6bwnss] [状态更新]: clean-code收口发现部分managed误降级

- SIGKILL live接管已通过:stable inode不变,PID 39323 -> 84240,lease ID更新,`jd`一致,ping与重复保护正常.
- 新RED确认部分managed字段会错误降级为legacy PID active.
- 修复只区分"全空v1"与"部分/非法managed",不改变完整managed和旧v1路径.

### 当前状态

**阶段5最终回归中**:重跑第三项GREEN、完整runtime/e2e/check/build,然后重新安装并重启live daemon.

## [2026-07-18 12:37:37] [Session ID: omx-1784340333160-6bwnss] [状态更新]: 最终自动回归通过

- 部分managed字段精确GREEN:1 passed.
- runtime:38 passed.
- unixpipe e2e:12 passed.
- router-client:26 passed,2 ignored.
- scoped rustfmt与`git diff --check`通过.
- all-targets check与release build exit 0,0 errors;既有control-act warning不在本任务文件.

### 当前状态

**阶段5最终安装中**:安装release,正常重启PID 84240并完成最后live smoke.

## [2026-07-18 12:40:06] [Session ID: omx-1784340333160-6bwnss] [状态更新]: 阶段5完成,进入记录与提交

### 最终live结果

- release重新安装成功,installed与`target/release/rdog`哈希一致.
- 最终daemon PID 29465 ready,local-default sidecar/registry `jd`无diff.
- duplicate启动exit 1,前后bare ping均返回pong.

### 最终阶段

- [x] 阶段1:读取当前三类guard/registry实现、规格与项目Rust版本约束.
- [x] 阶段2:最小跨进程实验验证文件锁互斥、SIGKILL释放和unlink语义.
- [x] 阶段3:完成lease schema、resource identity、迁移与时序规格.
- [x] 阶段4:TDD实现共享lease抽象并迁移service/path/local-default.
- [x] 阶段5:完成单测、e2e、router-client、编译、安装和live迁移/崩溃恢复.
- [ ] 阶段6:更新长期记录并创建独立scoped commit.

### 当前状态

**阶段6进行中**:写入支线交付记录,执行提交前fresh gate并显式提交.

## [2026-07-18 12:42:59] [Session ID: omx-1784340333160-6bwnss] [状态更新]: 提交前fresh gate通过

- scoped rustfmt、`git diff --check`和Mermaid sequence验证通过.
- runtime:38 passed;unixpipe e2e:12 passed;router-client:26 passed,2 ignored.
- all-targets check与release build:exit 0,0 errors.
- installed daemon live ping返回pong.

### 当前状态

**阶段6提交中**:使用显式文件清单stage,复核cached边界后创建scoped commit.

## [2026-07-18 12:44:29] [Session ID: omx-1784340333160-6bwnss] [完成]: 原子process lease任务收口

- [x] 阶段6:长期记录已更新,显式14文件scoped commit已创建.
- cached边界与whitespace检查通过,未使用`git add .`.
- 最终installed daemon保持运行,交付前再做status与ping确认.

### 状态

**全部阶段完成**:等待amend纳入本条完成记录并执行最终只读确认.
