# 任务计划: local-default legacy退役

## [2026-07-18 12:51:22] [Session ID: omx-1784340333160-6bwnss] [计划]: managed-only运行契约与升级门

### 目标

将`rdog.process-lease.v1`设为Unix本地运行态的唯一正常契约;旧v1 PID/registry只允许作为一次性升级检测输入,不再作为control client可用owner来源.

### 两个方向

1. 最佳方案:managed-only client + active legacy fail-closed升级门 + stopped legacy原地迁移 + 真实旧版/新版矩阵测试.
2. 过渡方案:只更新部署文档并扫描旧二进制,保留v1 client fallback;上线快,但产品契约仍包含PID liveness与迁移窗口.

### 当前决策

- 用户选择后续建议1,采用最佳方案.
- 不直接删除active legacy PID检查.该检查不是真相源,而是防止升级时与未持OS lock的旧daemon并行运行的安全门.
- managed local-default client只接受完整registry + sidecar identity + active lock.
- 所有lease字段全空的v1 registry从正常发现路径退役;stopped legacy guard由新daemon在stable inode上原地升级,不unlink.

### 阶段

- [ ] 阶段1:审计Unix三类legacy分支、现有安装副本与启动路径.
- [ ] 阶段2:最小旧版/新版实验确认支持矩阵与能够证伪双owner的条件.
- [ ] 阶段3:TDD固定managed-only client、active legacy拒绝和stopped legacy原地迁移.
- [ ] 阶段4:实现契约收紧,同步规格与部署说明.
- [ ] 阶段5:运行runtime、unixpipe e2e、router-client、check/build和live迁移验证.
- [ ] 阶段6:更新支线记录,清理已完成LATER项并创建scoped commit.

### 关键问题

1. 哪些`process_exists`调用是升级安全门,哪些仍把PID错误当作正常liveness?
2. 新client拒绝v1 registry后,空target在旧daemon升级窗口应返回什么明确错误?
3. stopped legacy PID文件能否在不unlink stable path的情况下原地迁移为managed lease?
4. 旧二进制是否还有可执行副本、launch配置或测试fixture会重新引入unlink行为?
5. Windows仍使用旧create/unlink guard,是否属于本轮Unix契约范围还是需要单独平台计划?

### 验证纪律

- 现象:旧二进制会unlink它判断stale的PID path,但已升级的新版本不会.
- 主假设:退役v1 client fallback并保留active legacy fail-closed门,可以让正常运行路径完全由OS lock决定,同时保持安全升级.
- 最强备选:只要旧二进制仍可执行,任何新版代码都无法阻止它忽略lease;需要将二进制清理和部署检查作为验收条件.
- 推翻主假设的证据:新client仍接受纯v1 record、stopped legacy无法原地迁移、active old/new可以同时ready,或升级后bare ping无法恢复.

### 状态

**阶段1进行中**:读取精确调用链并审计本机旧二进制/启动副本.

## [2026-07-18 12:54:20] [Session ID: omx-1784340333160-6bwnss] [状态更新]: 阶段1完成

### 静态调用链

- `ProcessLease::acquire`中的legacy PID检查发生在exclusive lock已获取、metadata不匹配时,用于拒绝仍活跃但不持OS lock的旧daemon.
- `LocalDefaultDaemonRecord::owner_is_active`仍把纯v1记录交给`process_exists`,这是正常client路径中的PID liveness fallback.
- 非Unix `acquire_daemon_name_guard`仍使用create/unlink,不属于本轮已验证的Unix process lease契约.

### 本机部署审计

- PATH与常见安装目录只发现`/Users/cuiluming/.cargo/bin/rdog`.
- cargo安装表只有当前`rustdog v3.0.0`;installed/release哈希此前已验证一致.
- 运行中只有tmux `rdog-daemon`及其PID 29465,没有rdog/rustdog LaunchAgent或LaunchDaemon.

### 遇到错误

- 第一次`cargo install --list`后的`rtk grep`参数不正确,误扫描仓库并产生大量无关输出;未改文件.改用raw cargo输出加`rg`后得到可信安装清单.

### 阶段状态

- [x] 阶段1:审计Unix三类legacy分支、现有安装副本与启动路径.
- [ ] 阶段2:最小旧版/新版实验确认支持矩阵与能够证伪双owner的条件.

### 当前状态

**阶段2进行中**:验证v1 registry拒绝是否会被唯一FIFO fallback绕过,并建立真实旧版矩阵.

## [2026-07-18 13:02:44] [Session ID: omx-1784340333160-6bwnss] [状态更新]: 阶段2完成

### 动态结论

- active旧版 -> 新版拒绝,升级安全门有效.
- stopped旧版 -> 新版保持三个inode原地迁移,managed lease恢复成功.
- active新版 -> 旧版拒绝,兼容PID发布后互操作安全.
- stopped新版 -> 旧版会unlink并更换三个inode,且覆盖回纯v1 registry.
- 当前新版client对纯v1 registry仍返回pong,唯一FIFO fallback也是legacy自动发现入口.

### 遇到错误

- 首次构建旧worktree时把尚未创建的目录用作`workdir`,exec在启动前失败且未执行命令;改为先创建worktree再单独build.
- 第一次tmux旧daemon启动没有`remain-on-exit`,session消失后丢失pane输出;重新以保留pane方式启动并成功捕获ready日志.

### 阶段状态

- [x] 阶段2:最小旧版/新版实验确认支持矩阵与能够证伪双owner的条件.
- [ ] 阶段3:TDD固定managed-only client、active legacy拒绝和stopped legacy原地迁移.

### 当前状态

**阶段3进行中**:先把纯v1 registry和唯一FIFO自动选择改成RED断言,再实施managed-only client.

## [2026-07-18 13:06:39] [Session ID: omx-1784304547353-h5409r] [行动计划]: 继续阶段3的RED验证

### 本轮目标

- 在现有本地daemon解析测试缝隙上固定managed-only契约.
- 先证明纯v1 registry和唯一unmanaged FIFO仍会被自动选择,再修改生产代码.
- 复用Unix process lease测试,确保active legacy拒绝与stopped legacy原inode迁移不退化.

### 执行顺序

1. 阅读`find_local_daemon_name`、`owner_is_active`及相关单元/e2e测试的完整上下文.
2. 将旧fallback成功断言改成明确的managed-only失败断言,运行focused test取得RED证据.
3. 只修改已被RED覆盖的发现逻辑,然后回跑focused test取得GREEN证据.

### 停止条件

- RED失败必须直接命中旧PID fallback或唯一FIFO自动选择,不能是测试fixture错误.
- 若RED不能证伪这两条路径,撤回当前假设并补充动态证据,不继续改生产代码.

## [2026-07-18 13:09:31] [Session ID: omx-1784304547353-h5409r] [状态更新]: 阶段3 RED完成

### 验证结果

- 纯v1 registry + matching FIFO用例按预期失败,exit 101.
- 唯一unmanaged FIFO用例按预期失败,exit 101,实际返回`Ok("findme.findunique")`.
- RED直接证实两条legacy自动发现路径仍在执行.

### 当前状态

**阶段3继续**:将client owner判定收紧为完整managed metadata + active OS lock,并把FIFO扫描降为只诊断、不选择.

## [2026-07-18 13:11:00] [Session ID: omx-1784304547353-h5409r] [状态更新]: focused GREEN完成

### 实现结果

- `owner_is_active`不再把纯v1 PID记录交给`process_exists`;缺失完整lease metadata一律不是client owner.
- FIFO扫描继续提供候选诊断,但一个或多个候选都返回`NotFound`,不再自动选择.
- focused legacy组合测试与唯一FIFO测试均为1 passed.

### 下一步行动

- 运行完整`zenoh_runtime::tests`,识别仍依赖旧fallback的测试.
- managed local-default成功场景改为持有真实`register_local_default_daemon` guard.
- unmanaged FIFO场景统一验证退役诊断,同时复验process lease的legacy安全门.

## [2026-07-18 13:13:58] [Session ID: omx-1784304547353-h5409r] [状态更新]: runtime契约测试完成

### 测试结果

- 首轮完整runtime:29 passed,5 failed;失败均为需要迁移的旧fallback断言.
- fixture已调整为真实managed guards或明确unmanaged诊断.
- 第二轮完整runtime:34 passed,0 failed.

### 遇到错误

- `cargo fmt -- src/zenoh_runtime.rs`额外改动24个此前clean的源码文件.
- 已按执行前状态证据显式恢复这些文件,当前生产代码改动重新收敛到`src/zenoh_runtime.rs`.

### 下一步行动

- 更新unixpipe e2e:无`local_default`的空target/self改为managed-only拒绝,有managed registry的成功场景保持不变.
- 运行完整unixpipe e2e,复验crash lease接管与duplicate拒绝.

## [2026-07-18 13:15:36] [Session ID: omx-1784304547353-h5409r] [状态更新]: focused e2e完成

### 结果

- self + namespace拒绝unmanaged daemon:1 passed.
- 空target + namespace拒绝unmanaged daemon:1 passed.
- 多个unmanaged FIFO只返回诊断、不自动选择:1 passed.

### 当前状态

**阶段3收口验证**:运行完整unixpipe e2e,确认managed成功路径与lease生命周期不退化.

## [2026-07-18 13:16:23] [Session ID: omx-1784304547353-h5409r] [阶段完成]: 阶段3完成

### 验证结果

- 完整`zenoh_runtime::tests`:34 passed.
- 完整`zenoh_unixpipe_fast_path`:12 passed.
- managed local-default成功、duplicate拒绝、crash后lease接管和unmanaged拒绝均通过.

### 阶段状态

- [x] 阶段1:审计Unix三类legacy分支、现有安装副本与启动路径.
- [x] 阶段2:最小旧版/新版实验确认支持矩阵与能够证伪双owner的条件.
- [x] 阶段3:TDD固定managed-only client、active legacy拒绝和stopped legacy原地迁移.
- [ ] 阶段4:实现契约收紧,同步规格与部署说明.

### 当前状态

**阶段4进行中**:同步源码注释、配置模板、fast-path规格与`rdog-control` skill,消除唯一FIFO fallback旧文案.

## [2026-07-18 13:19:26] [Session ID: omx-1784304547353-h5409r] [阶段完成]: 阶段4完成

### 同步范围

- `src/main.rs`:空target/self解析注释改为managed-only,FIFO仅诊断.
- `rdog_macos.toml` / `rdog_linux.toml`:明确`local_default = true`是空target/self正常入口.
- `specs/zenoh-unixpipe-fast-path-plan.md`:更新daemon lease、client发现、legacy迁移与验收契约.
- `.codex/skills/rdog-control/SKILL.md`:升级到1.7,移除唯一FIFO自动选择指引.

### 文档验证

- 旧"唯一FIFO fallback"措辞搜索结果为0.
- `beautiful-mermaid-rs --ascii`验证2个Mermaid block全部通过.
- `git diff --check`通过.

### 运行态基线

- 正式tmux daemon PID 29465仍存活,命令指向已安装`~/.cargo/bin/rdog`.
- 修改前安装版`rdog control @ping`仍返回`@response "pong"`.

### 阶段状态

- [x] 阶段4:实现契约收紧,同步规格与部署说明.
- [ ] 阶段5:运行runtime、unixpipe e2e、router-client、check/build和live迁移验证.

### 当前状态

**阶段5进行中**:先跑自动验证矩阵,全部通过后才替换正式安装版并重启daemon.

## [2026-07-18 13:21:18] [Session ID: omx-1784304547353-h5409r] [状态更新]: 自动验证矩阵完成

### 结果

- process lease 4 passed;runtime 34 passed;unixpipe e2e 12 passed.
- router-client 26 passed,2 ignored.
- all-targets check与release build均为0 error.
- 10条warning属于未改动的control-act基线,本轮无新增warning.

### 遇到错误

- `rtk find`忽略原生`-print`并输出大量历史guard;改用`rtk proxy find`精确过滤,没有清理或修改状态目录.

### 正式部署行动

1. 记录service/path/local-default三个stable lease inode和新旧二进制hash.
2. 终止`rdog-daemon` tmux并确认旧PID退出.
3. 原子替换`~/.cargo/bin/rdog`,重新创建同名tmux session.
4. 验证三个inode保持、managed registry/sidecar一致、裸ping与显式target均成功.
5. 启动第二实例验证duplicate拒绝,再复验裸ping未受破坏.

## [2026-07-18 13:23:54] [Session ID: omx-1784304547353-h5409r] [状态更新]: 正式二进制切换完成

### 切换证据

- 旧安装版hash:`db55502b5fac368b0df56b0c267bf1dd3f166574a5640a569aebb3df59aaa3bc`.
- 新release/安装版hash:`4f03fe2ff1a3575a79f5dcb3100a8a74a6e19fe907406d88558eb9bcc1d7a25f`.
- 旧PID 29465已随tmux session终止;新正式daemon PID 5232 ready.
- service/path/local-default inode分别保持`512111038`、`512111044`、`512111045`,切换没有unlink stable lease.

### 当前状态

**阶段5 live验证**:核对managed identity,执行bare/self/显式target ping和duplicate拒绝后的存活复验.

## [2026-07-18 13:26:23] [Session ID: omx-1784304547353-h5409r] [阶段完成]: 阶段5完成

### live结果

- registry与local-default sidecar的6个lease identity字段经`jd`比较无差异.
- `rdog control @ping`、`rdog control self @ping`、`rdog control mac.lab @ping`均返回`@response "pong"`.
- duplicate daemon exit 1,由service-name active lease拒绝;正式uplink inode`512227946`未变.
- duplicate失败后bare ping再次返回pong,正式PID 5232保持运行.

### 验证脚本纠错

- 首次duplicate断言只接受"已存在",实际错误为"发现重复 service_name 活跃 member",导致脚本提前退出.
- 已明确撤回窄文案假设,改为验证exit code、namespace、service_name和重复owner语义;复跑通过.

### 阶段状态

- [x] 阶段5:运行runtime、unixpipe e2e、router-client、check/build和live迁移验证.
- [ ] 阶段6:更新支线记录,清理已完成LATER项并创建scoped commit.

### 当前状态

**阶段6进行中**:沉淀WORKLOG/ERRORFIX/LATER/EPIPHANY/EXPERIENCE,审查显式文件清单后提交.

## [2026-07-18 13:29:33] [Session ID: omx-1784304547353-h5409r] [验证纠正]: 文档搜索范围不完整

### 被新证据推翻的口径

- 13:19记录的"旧唯一FIFO fallback措辞为0"只覆盖主规格、skill主文件、配置和源码注释,不能代表全仓文档.
- 全仓长期入口搜索发现`specs/zenoh-control-plane-plan.md`和`.codex/skills/rdog-control/references/control-workflow.md`仍保留旧契约.

### 当前行动

- 同步这两个遗漏入口,并把control-plane中的PID guard旧术语一起更新为process lease.
- 重新对非archive/target范围做全仓搜索;只有旧正常路径表述清零后才允许提交.

## [2026-07-18 13:31:39] [Session ID: omx-1784304547353-h5409r] [状态更新]: 遗漏文档已同步

### 验证结果

- 活动契约范围的repo-wide搜索没有再发现唯一FIFO fallback、PID owner或client清理stale registry旧语义.
- 两份规格共2个Mermaid block通过`beautiful-mermaid-rs --ascii`.
- skill主文件、workflow reference和两份规格的Markdown fence均成对.

### 下一步行动

- 因`no_local_daemon_error`文案也做了可读性调整,重新运行完整自动矩阵与release build.
- 最终release hash变化后原子更新安装版,重启正式daemon并做最后bare ping.

## [2026-07-18 13:33:54] [Session ID: omx-1784304547353-h5409r] [提交前状态]: 最终部署与文件边界确认

### 最终部署

- 安装版与最终release hash均为`96955460e968cc8ccaf06c1b4fc2bce888e4c5564df5b6f0cac69e348249cc75`.
- 正式daemon PID 19047 ready;三个stable lease inode仍为`512111038/512111044/512111045`.
- 最终registry-sidecar identity一致;duplicate exit 1;uplink inode`512231003`保持;bare ping返回pong.

### 提交边界

- 仓库没有submodule.
- tracked改动12个,均属于实现、测试、规格/skill同步、长期经验或支线上下文.
- untracked新增6个,均为本支线按需创建的上下文记录.
- 下一步使用显式文件清单暂存,禁止`git add .`.

## [2026-07-18 13:35:10] [Session ID: omx-1784304547353-h5409r] [阶段完成]: 阶段6与任务完成

### 提交结果

- 已使用显式18文件清单暂存并通过staged diff check.
- scoped commit已创建,提交主题:`fix(zenoh): retire legacy local daemon discovery`.
- 收口记录将通过同一commit amend纳入,不另建日志提交.

### 最终阶段

- [x] 阶段1:审计Unix三类legacy分支、现有安装副本与启动路径.
- [x] 阶段2:最小旧版/新版实验确认支持矩阵与能够证伪双owner的条件.
- [x] 阶段3:TDD固定managed-only client、active legacy拒绝和stopped legacy原地迁移.
- [x] 阶段4:实现契约收紧,同步规格与部署说明.
- [x] 阶段5:运行runtime、unixpipe e2e、router-client、check/build和live迁移验证.
- [x] 阶段6:更新支线记录,清理已完成LATER项并创建scoped commit.

### 状态

**任务完成**:Unix正常运行契约已经managed-only,legacy只保留fail-closed升级门;最终正式daemon PID 19047运行正常.
