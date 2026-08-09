# 任务计划: zenoh_runtime职责拆分

## [2026-07-18 16:13:17] [Session ID: omx-1784304547353-h5409r] [计划]: 保持interface不变的深模块重构

### 目标

把1928行`src/zenoh_runtime.rs`收敛为稳定门面,将session、unixpipe、local-default和测试实现放入职责明确的子模块,不改变调用者可见interface、错误文案、配置优先级或运行时行为.

### 两个方向

1. 最佳方案:拆出`session`、`unixpipe`、`local_default`和`tests`,保留`process_lease`;门面只声明模块并`pub use`现有公开项.
2. 过渡方案:只搬单元测试和local-default;改动较小,但session/unixpipe仍耦合在接近1000行的生产文件中.

### 当前决定

- 用户明确要求拆分上一轮指出的1928行坏味道,直接采用最佳方案.
- 本轮是行为保持重构,禁止顺带改协议、错误文案、配置、lease语义或测试期望.
- 子模块interface优先保持私有;只有当前跨crate调用所需符号由门面重导出,不扩大public surface.

### 阶段

- [ ] 阶段1:建立symbol/调用者/可见性清单,确定无环模块依赖.
- [ ] 阶段2:跑行为基线并固定文件行数、测试数量和正式daemon状态.
- [ ] 阶段3:按垂直切片搬迁local-default、unixpipe、session与tests,每步编译/测试.
- [ ] 阶段4:审查interface、依赖方向、文件行数、文档/索引和代码坏味道.
- [ ] 阶段5:运行完整验证矩阵、release构建与live smoke.
- [ ] 阶段6:更新支线记录,清理已完成LATER项并创建scoped commit.

### 关键问题

1. 哪些符号是真正跨模块interface,哪些只是当前大文件内的实现泄漏?
2. local-default对unixpipe路径/存活检查的依赖如何保持单向,避免循环依赖?
3. session endpoint composition与unixpipe fast path之间的配置依赖应放在哪个seam?
4. inline测试依赖多少私有符号,搬到`sibling tests`后需要`pub(super)`到什么最小程度?
5. 拆分后每个Rust文件是否低于1000行,目录文件数是否保持合理?

### 验证纪律

- 现象:`src/zenoh_runtime.rs`为1928行,生产实现、平台分支和单元测试集中在同一门面.
- 主假设:按真实职责拆分并由父模块重导出,可以在不改调用者的情况下提升locality并保持全部行为.
- 最强备选:现有私有符号耦合过密,一次拆三块会迫使大量`pub(super)`和循环依赖;若出现,应退回以`unixpipe`为内部根模块、local-default作为其子模块的层级结构,而不是制造互相调用的siblings.
- 推翻主假设的证据:调用者必须改用新路径、出现循环依赖、测试只能通过扩大`pub(crate)`surface、错误输出变化或live fast path退化.

### 停止条件

- 任一切片出现无法解释的行为差异时,回退该切片并重新划分seam,不在重构中修bug.
- 最终所有阶段勾选、工作区验证通过且正式daemon smoke正常后才交付.

### 状态

**阶段1进行中**:用CodeGraph建立结构清单,再读取完整文件分段与外部调用点.

## [2026-07-18 16:16:59] [Session ID: omx-1784304547353-h5409r] [阶段完成]: 阶段1完成

### 结构结论

- 最终内部依赖为`session -> unixpipe`、`local_default -> unixpipe + process_lease`、`unixpipe -> process_lease`.
- 父模块重导出现有公开符号,外部调用者不改路径.
- 测试跟随三个实现模块,父模块只保留共享环境变量mutex.
- 不增加trait、adapter、feature flag或兼容wrapper,避免把文件拆分变成interface膨胀.

### 阶段状态

- [x] 阶段1:建立symbol/调用者/可见性清单,确定无环模块依赖.
- [ ] 阶段2:跑行为基线并固定文件行数、测试数量和正式daemon状态.

### 当前状态

**阶段2进行中**:运行HEAD基线测试、check和live ping,记录拆分前证据.

## [2026-07-18 16:20:21] [Session ID: omx-1784304547353-h5409r] [阶段完成]: 阶段2完成

### 拆分前基线

- 行数:`zenoh_runtime.rs` 1928,`process_lease.rs` 440.
- process lease:4 passed;runtime:34 passed.
- unixpipe e2e:12 passed;router-client:26 passed,2 ignored.
- all-targets check:0 error;10条warning仍全部来自未改动的control-act基线.
- 正式daemon PID 19047,bare ping返回`@response "pong"`.

### 阶段状态

- [x] 阶段2:跑行为基线并固定文件行数、测试数量和正式daemon状态.
- [ ] 阶段3:按垂直切片搬迁local-default、unixpipe、session与tests,每步编译/测试.

### 当前状态

**阶段3切片1**:先搬session实现与3项session测试,父模块重导出现有interface,随后立即编译和运行session测试.

## [2026-07-18 16:21:44] [Session ID: omx-1784304547353-h5409r] [遇到错误]: session切片脚本首次未完成

- 现象:Python在重算父测试marker时把整数offset传给`str.index`,抛`TypeError`.
- 影响:只生成了尚未接入模块树的`session.rs`;父`zenoh_runtime.rs`没有删除或替换实现,运行路径未变化.
- 处理:移除无效的重复定位语句,从原父文件重新按字符串marker生成两份文件;命令增加`set -e`,脚本失败时不再继续rustfmt.

## [2026-07-18 16:24:40] [Session ID: omx-1784304547353-h5409r] [切片完成]: session模块

### 结果

- 新增`src/zenoh_runtime/session.rs`,298行.
- 父模块从1928行降至1650行,并重导出原有4个session interface.
- `cargo check --package rustdog --bin rdog`:0 error,只剩既有control-act warning.
- `cargo test --package rustdog --bin rdog -- zenoh_runtime::`:38 passed.

### 纠错

- session搬走后父模块的`Instant`只被尚未搬迁的测试使用,首次check产生1条新warning.
- 已把`Instant`移入测试module import,再次check没有`zenoh_runtime` warning.

### 当前状态

**阶段3切片2**:搬迁unixpipe路径、ownership、probe、listener composition及对应测试;增加共享test_support确保环境变量测试仍串行.

## [2026-07-18 16:29:54] [Session ID: omx-1784304547353-h5409r] [切片完成]: unixpipe模块

### 结果

- 新增`unixpipe.rs` 719行与`test_support.rs` 28行;父文件降至935行.
- unixpipe focused测试18 passed;全runtime仍为38项.
- package bin check为0 error,没有新增zenoh_runtime warning.

### 新暴露的测试隔离问题

- 现象:unixpipe focused 18项通过,但38项默认并发稳定在`prepare_unixpipe_listener`报`NotFound`;串行38项通过.
- 静态证据:`unique_test_dir()`读取进程级`TMPDIR`,该测试未持共享环境mutex;其他local-default测试会切换并删除临时TMPDIR.
- 已验证结论:目录被创建在另一测试的临时TMPDIR下并随后删除;不是生产lease错误,也不是毫秒目录名碰撞.
- 修复:该测试复用现有`env_test_guard`;默认并发38项连续5轮全部通过.

### 当前状态

**阶段3切片3**:搬迁local-default registry/managed owner/FIFO诊断及13项相关测试,把父文件收敛为门面.

## [2026-07-18 16:33:49] [Session ID: omx-1784304547353-h5409r] [阶段完成]: 阶段3完成

### 最终模块

- `zenoh_runtime.rs`:22行稳定门面.
- `local_default.rs`:926行;`session.rs`:298行;`unixpipe.rs`:719行.
- `process_lease.rs`:445行;`test_support.rs`:28行;目录共5个文件.
- 所有Rust文件低于1000行,目录文件数低于8.

### local-default切片验证

- focused local-default:13 passed.
- package bin check:0 error,无新增zenoh_runtime warning.
- process-lease测试接入共享TMPDIR mutex后,默认并发38项连续5轮通过.

### 阶段状态

- [x] 阶段3:按垂直切片搬迁local-default、unixpipe、session与tests,每步编译/测试.
- [ ] 阶段4:审查interface、依赖方向、文件行数、文档/索引和代码坏味道.

### 当前状态

**阶段4进行中**:收紧内部可见性,同步AGENTS/skill reference/fast-path规格,验证Mermaid与全仓旧路径.

## [2026-07-18 16:36:01] [Session ID: omx-1784304547353-h5409r] [行动调整]: 测试实现再下沉一层

- 审查发现`local_default.rs`虽为926行但约半数是inline测试,未来很快会再次超过1000行.
- 最佳方案调整为`local_default/tests.rs`、`unixpipe/tests.rs`、`session/tests.rs`;父实现只保留`#[cfg(test)] mod tests;`.
- tests仍是实现模块child,可以访问私有项,不会增加`pub(super)`或父门面surface.
- 每个新增子目录只有1个文件,仍满足每层不超过8个文件.

## [2026-07-18 16:37:36] [Session ID: omx-1784304547353-h5409r] [阶段完成]: 阶段4完成

### 结构审查

- 父门面22行;生产模块最大457行;测试模块最大464行;process lease 445行.
- 顶层`src/zenoh_runtime/`有5个文件,三个测试子目录各1个文件.
- 依赖保持单向,没有循环module import.
- 父门面只重导出当前调用者实际使用的7个function/type入口;没有allow unused掩盖interface泄漏.
- process-lease对外路径保持`crate::zenoh_runtime::process_lease`,调用者无需修改.

### 文档与格式

- 更新AGENTS代码路径索引和skill workflow source anchor.
- fast-path规格新增runtime模块依赖图;3个Mermaid block全部通过CLI验证.
- 活动文档中旧`src/zenoh_runtime.rs::find_local_daemon_name`路径为0.
- modified Rust文件均由直接`rustfmt`格式化,没有运行会污染全仓的`cargo fmt`.

### 阶段状态

- [x] 阶段4:审查interface、依赖方向、文件行数、文档/索引和代码坏味道.
- [ ] 阶段5:运行完整验证矩阵、release构建与live smoke.

### 当前状态

**阶段5进行中**:运行全bin、runtime、unixpipe e2e、router-client、all-targets check和release build.

## [2026-07-18 16:39:08] [Session ID: omx-1784304547353-h5409r] [状态更新]: 自动验证矩阵完成

### 结果

- scoped rustfmt check通过.
- 全bin:612 passed,1 ignored;runtime:38 passed.
- unixpipe e2e:12 passed;router-client:26 passed,2 ignored.
- all-targets check与release build为0 error.
- 10条warning仍全部来自未改动的control-act基线,拆分模块无warning.

### live切换计划

- 当前安装版hash:`96955460e968cc8ccaf06c1b4fc2bce888e4c5564df5b6f0cac69e348249cc75`.
- 新release hash:`1cb163bd810c710bce67fc36c28a1132605f386df9104d9a33b0c5ab88be25e0`.
- 记录三个stable lease inode,停止PID 19047,原子安装新release并重启同名tmux session.
- 验证inode不变、bare/self/显式target ping和duplicate拒绝.

## [2026-07-18 16:43:50] [Session ID: omx-1784304547353-h5409r] [阶段完成]: 阶段5完成

### live验证结果

- 原子安装的release hash为`1cb163bd810c710bce67fc36c28a1132605f386df9104d9a33b0c5ab88be25e0`.
- 正式daemon已由`rdog-daemon` tmux session持有,当前PID为82774.
- service、unixpipe path、local-default三类stable lease inode在重启后保持不变.
- `rdog control @ping`、`rdog control self @ping`、`rdog control mac.lab @ping`均返回`pong`.
- duplicate daemon按预期以exit 1拒绝;失败后uplink inode未变,bare ping继续成功.

### 阶段状态

- [x] 阶段5:运行完整验证矩阵、release构建与live smoke.
- [ ] 阶段6:更新支线记录,清理已完成LATER项并创建scoped commit.

### 当前状态

**阶段6进行中**:先做HEAD旧文件与当前模块的symbol/interface等价审查,再执行clean-code复核和最终验证.

## [2026-07-18 16:47:12] [Session ID: omx-1784304547353-h5409r] [审查纠正]: symbol脚本补入共享测试模块

- 首次symbol集合脚本只纳入三个职责模块及其tests,漏掉`test_support.rs`,因此把原文件已有的`env_test_guard`、`unique_test_dir`与`LOCK`误报为缺失.
- 该输出是审查脚本范围错误,不是生产代码或测试实现遗漏.
- 下一轮把`test_support.rs`纳入完整集合,同时分别比较生产symbol、测试symbol和外部调用行,避免总量相同掩盖跨层误放.

## [2026-07-18 16:49:31] [Session ID: omx-1784304547353-h5409r] [审查通过]: symbol与外部interface等价

- 旧/新production symbol为46 function、5 type、4 constant,集合完全一致.
- 旧/新测试为34项,测试函数名称完全一致;测试辅助symbol也完整迁移.
- HEAD与工作树的外部`zenoh_runtime::`调用均为26行,无路径变化.
- 下一步按clean-code-guard检查门面大小、可见性、依赖无环、注释职责、死代码和新warning;发现行为性问题则停止提交.

## [2026-07-18 16:51:36] [Session ID: omx-1784304547353-h5409r] [工具纠正]: 静态grep表达式修正

- 首次`unwrap/expect`扫描把括号写成未闭合正则,rg返回parse error;此前同组的文件布局、依赖、TODO与`git diff --check`命令已独立完成.
- 改用两个fixed-string扫描重新执行,不会把错误命令的空结果当成审查证据.

## [2026-07-18 16:54:18] [Session ID: omx-1784304547353-h5409r] [审查结果]: clean-code通过,全仓Clippy基线不绿

- 门面最小、模块依赖无环、文件/目录规模合规,没有新增dead code allow或占位实现.
- `cargo clippy --all-targets --quiet`退出101;4个deny错误全部位于未修改PTY路径,不是本轮引入.
- zenoh_runtime的3条普通建议对应HEAD旧实现,本轮不把lint cleanup混入行为保持重构.
- 最终验证改用完整编译/测试矩阵作为提交门禁,并保留Clippy基线失败的诚实记录.

## [2026-07-18 16:55:00] [Session ID: omx-1784304547353-h5409r] [行动]: 提交前最终验证

- 重新执行scoped rustfmt、全bin、runtime 38项、unixpipe e2e、router-client、all-targets check与release build.
- 自动矩阵全部为0 error后,再验证已安装binary hash与三种live ping.
- 任一验证失败就停止记录/提交,先按现象、假设、最小验证处理.

## [2026-07-18 17:02:00] [Session ID: omx-1784304547353-h5409r] [收口]: 记录完成并准备显式暂存

- 更正阶段4数量口径:父门面重导出7个function与1个type,合计8个现有入口;没有新增入口.
- 支线WORKLOG、ERRORFIX与项目EXPERIENCE已写入,旧延期项已追加完成状态.
- 活跃支线没有新增LATER或EPIPHANY事项;TMPDIR测试规律已沉淀到EXPERIENCE,无需另建EPIPHANY.
- 下一步按文件清单显式暂存,检查staged diff、空白错误和submodule状态,不使用`git add .`.

## [2026-07-18 17:04:00] [Session ID: omx-1784304547353-h5409r] [提交门禁]: staged diff审查通过

- 暂存区共19个scoped文件,2477 insertions / 1924 deletions;新增行主要来自职责模块、原测试搬迁和支线上下文记录.
- `git diff --name-only`为空,没有漏掉未暂存改动;工作区也没有任务外文件.
- `git diff --cached --check`通过;仓库没有submodule.
- tracked文档只更新真实source anchor、runtime职责图和长期经验;process-lease生产代码未改,仅4项测试接入共享环境mutex.
- 现在创建`refactor(zenoh): split runtime responsibilities`提交;提交成功后再写阶段6完成与主线支线摘要,最后amend.

## [2026-07-18 17:05:00] [Session ID: omx-1784304547353-h5409r] [阶段完成]: 阶段6完成

### 收口结果

- scoped commit已创建,subject为`refactor(zenoh): split runtime responsibilities`.
- 支线WORKLOG、ERRORFIX、notes与长期EXPERIENCE均已落盘.
- 原`LATER_PLANS__local_default_atomic_lease.md`中的超长runtime拆分项已追加完成状态.
- 活跃支线没有未实施事项,无需创建`LATER_PLANS__zenoh_runtime_split.md`.
- 本轮没有未解决的架构灾难点或未来风险,无需创建`EPIPHANY_LOG__zenoh_runtime_split.md`.

### 最终阶段状态

- [x] 阶段1:建立symbol/调用者/可见性清单,确定无环模块依赖.
- [x] 阶段2:跑行为基线并固定文件行数、测试数量和正式daemon状态.
- [x] 阶段3:按垂直切片搬迁local-default、unixpipe、session与tests,每步编译/测试.
- [x] 阶段4:审查interface、依赖方向、文件行数、文档/索引和代码坏味道.
- [x] 阶段5:运行完整验证矩阵、release构建与live smoke.
- [x] 阶段6:更新支线记录,清理已完成LATER项并创建scoped commit.

### 状态

**任务完成**:等待amend纳入本条完成记录与主线摘要,随后只做HEAD和clean worktree验证.
