# local-default 原子lease研究笔记

## [2026-07-18 10:57:07] [Session ID: omx-1784304547353-h5409r] 笔记: 标准库锁可行性

### 来源

- 本机 `rustc --version --verbose`:Rust 1.96.0,aarch64-apple-darwin.
- 本机官方标准库源码 `library/std/src/fs.rs`.
- CodeGraph当前调用链与guard定义.
- `specs/zenoh-unixpipe-fast-path-plan.md` 当前ownership契约.
- 当前会话没有Context7或exa_code可调用工具,因此使用本机官方源码稳定性标记与后续编译/跨进程实验.

### 标准库结论

- `File::try_lock` / `try_lock_shared` 自1.89稳定,不需要新增crate.
- exclusive lock跨进程互斥;Unix当前使用 `flock(LOCK_EX|LOCK_NB)`.
- 锁随所有关联descriptor关闭自动释放,理论上覆盖正常退出和异常进程终止.
- `TryLockError::WouldBlock` 可与真实I/O错误分开映射.

### 当前实现缺口

- service-name、path owner、local-default都使用create_new PID文件 + `kill -0`.
- PID可复用;create后write存在空文件窗口;Drop直接unlink可能与竞争者open产生inode竞态.
- local-default元数据是 `lab.json`,liveness是 `lab.pid`,写入不是一个原子状态.
- client验证组合PID存活、schema、启动宽限和uplink存在,无法确认PID仍属于创建记录的daemon实例.

### 初步设计约束

- 三个资源key必须继续独立互斥,不能合并冲突域.
- 共享的应是 `LeaseRecord` schema、lock acquisition、active probe和错误映射.
- 锁文件在正常退出或崩溃后应保留,新owner在同一inode上接管并重写record;持锁期间禁止unlink.
- local-default client可用非阻塞shared lock探测exclusive owner是否存在,但必须处理daemon刚拿锁尚未完成record写入的短窗口.
- v1 `.pid` / `.json` 兼容需要单独定义,尤其要避免新旧二进制并行时互相忽略.

## [2026-07-18 10:59:26] [Session ID: omx-1784304547353-h5409r] 笔记: 跨进程最小实验

### 实验方法

- 用Rust 1.96标准库编译临时probe,分别执行 `hold`、`try` 和 `shared`.
- probe不写入仓库,结束时删除临时binary、lock和输出文件.
- 分别验证活跃holder、SIGKILL holder和unlink活跃lock path三种状态.

### 关键输出

- `active_exclusive=BLOCKED`.
- `active_shared=SHARED_BLOCKED`.
- `after_sigkill=ACQUIRED`.
- `after_unlink=ACQUIRED`.
- unlink前后inode:512118831 -> 512118834.

### 设计结论

- lock file必须是稳定inode,不得在Drop、stale recovery或cleanup时删除.
- guard对象持有 `File`,生命周期结束只依靠descriptor close释放lock.
- path owner只需要稳定lock file + 诊断PID内容.
- local-default需要稳定namespace lock file负责liveness,另一个通过同目录temp+rename发布的JSON负责metadata;它们不是两个liveness真相源.
- client先用shared probe判断lock是否被exclusive持有,再读取metadata;对旧v1记录保留PID验证fallback.

## [2026-07-18 11:03:04] [Session ID: omx-1784304547353-h5409r] 笔记: schema与兼容边界

### 同进程语义

- 一个handle持exclusive lock时,同进程另一个独立open的handle执行shared probe返回WouldBlock.
- unit test不必强制fork子进程即可验证active/inactive状态;SIGKILL释放仍由跨进程probe覆盖.

### 文件职责

- `*.pid`:稳定lock inode + 单行PID.新代码不删除,旧代码仍可读.
- service/path `*.lease.json`:原子诊断metadata,帮助新代码区分受管stale owner与legacy PID guard.
- local-default `lab.json`:业务registry + 可选lease字段,继续兼容旧v1 reader.

### local-default验证

- lease字段存在时:shared probe WouldBlock表示active;lock可获取表示stale,不得因PID复用判active.
- lease字段缺失时:按legacy PID + uplink + startup grace逻辑处理.
- active lock但record缺失/PID不匹配时:视为startup窗口,返回None且不删除任何lock file.

### 规格落点

- 已更新 `specs/zenoh-unixpipe-fast-path-plan.md` 3.5节.
- graph固定daemon acquisition顺序;sequence固定client shared probe与SIGKILL接管语义.

## [2026-07-18 11:05:20] [Session ID: omx-1784304547353-h5409r] 笔记: TDD RED

- `register_local_default_daemon_should_fail_when_same_namespace_guard_is_alive` 在Drop后PID文件不存在处失败.
- `prepare_unixpipe_listener_should_recover_stale_owner_guard_and_files` 在Drop后owner文件不存在处失败.
- 两项失败直接命中待替换行为,证明测试不是在验证实现细节之外的无关条件.

## [2026-07-18 11:12:47] [Session ID: omx-1784304547353-h5409r] 笔记: 实现边界

- `ProcessLease::acquire`先锁stable PID file,再判断旧metadata是否属于managed owner.
- lock可获取 + metadata匹配旧PID时,旧owner已由OS判定死亡,即使PID数值仍存活也允许接管.
- metadata缺失/不匹配 + PID存活时按legacy daemon处理并拒绝,保护升级期间的旧版本实例.
- local-default JSON保留v1 schema与核心字段,新增lease字段使用serde optional扩展.
- managed client通过shared probe + PID匹配判断active;legacy client路径继续使用 `kill -0`.
- client不再删除stale record或lock file,消除probe释放后与新owner发布之间的unlink竞态.

## [2026-07-18 11:15:43] [Session ID: omx-1784304547353-h5409r] 笔记: 崩溃恢复证据

- 第一daemon ready后由测试fixture发送SIGKILL,子进程不会运行Rust Drop.
- service PID file、path owner PID file、local-default PID/JSON均在崩溃后保留.
- 第二daemon使用同namespace、daemon name、socket path和state home启动成功.
- 第二daemon完成stale FIFO cleanup、覆盖新metadata并恢复空target ping.
- 这条e2e同时证明lock由OS释放,而不是依赖guard Drop或PID文件删除.

## [2026-07-18 11:21:30] [Session ID: omx-1784340333160-6bwnss] 笔记: 生产审查发现未提交PID回滚缺口

### 现象

- `ProcessLease::acquire`成功获取exclusive lock后立即把stable lock file改写为当前PID.
- service-name和unixpipe-path随后调用`publish_metadata`;local-default随后单独发布registry JSON.
- 任一发布发生I/O错误时,调用路径返回错误并释放lock,但stable file仍保留当前进程PID.

### 最小动态证据

- 测试把metadata目标预先创建为目录,稳定触发rename失败.
- 发布失败并drop lease后,同一进程立即重试.
- 命令结果:`0 passed, 1 failed`;重试返回`发现仍存活的legacy PID guard`,PID正是测试进程自身.

### 已验证结论

- metadata发布失败确实会把未提交PID误分类为legacy活跃owner,不是纯静态猜测.
- 正式修复需要在exclusive lock仍持有时回滚未提交的旧lock内容,不能删除stable path.
- 回滚只覆盖未成功发布metadata的lease;已发布lease退出后仍保留managed PID和metadata,供下一owner安全接管.

### 第二项候选假设

- local-default registry包含`lease_id`,但当前client没有把它与独立lease metadata比较.
- 当前只验证active lock和PID相等;PID复用或同进程重新注册窗口内,旧registry可能被误认成新owner.
- 下一步用不同lease ID的sidecar + 同PID active lock做最小RED,验证该路径是否真实误判.

## [2026-07-18 11:24:27] [Session ID: omx-1784340333160-6bwnss] 笔记: local-default lease ID关联RED

### 动态证据

- 先保留旧managed registry,再为同namespace写入不同lease ID的metadata sidecar.
- stable lock由同一测试进程持有,因此lock内PID与旧registry PID相同.
- 当前`owner_is_active`返回true,精确测试结果为`0 passed, 1 failed`.

### 已验证结论

- 当前client确实没有验证registry `lease_id`属于正在持锁的owner.
- 只比较PID会在PID复用或同PID重新注册的发布窗口发生ABA误判.
- local-default不能继续把registry同时当业务发现记录和唯一lease metadata;需要使用guard旁独立sidecar作为lease identity,registry引用并校验它.

### 正式修复设计

- `ProcessLease`保存获取锁前的原始内容;metadata未成功发布时,Drop在仍持exclusive lock期间恢复原内容.
- local-default与service/path一致,先发布`<guard>.lease.json`,再原子发布业务registry.
- client必须比较sidecar与registry中的schema、ID、resource kind/key、创建时间和PID,随后再用shared lock验证liveness.
- JSON rename后同步父目录,把原子可见性扩展为目录项持久化边界.
- 旧v1 registry没有lease字段时仍走PID + uplink兼容路径.

## [2026-07-18 11:38:15] [Session ID: omx-1784340333160-6bwnss] 笔记: 部分managed字段不能降级为legacy

- clean-code收口审查发现`lease_metadata()`的`None`同时表示合法v1和损坏managed记录.
- 最小测试移除active managed registry的`lease_id`,其余字段和当前PID保留.
- 当前实现错误返回active,精确测试结果:`0 passed, 1 failed`.
- 修复规则:五个lease扩展字段全部缺失才走legacy PID;任一字段出现但无法构成完整合法metadata时直接判inactive.

## [2026-07-18 12:40:06] [Session ID: omx-1784340333160-6bwnss] 笔记: 最终live证据

- 旧daemon PID 69053运行时,新lease二进制按legacy PID guard拒绝,前后ping均成功.
- 新daemon PID 39323首次启动后,三把stable lock、三个sidecar和registry全部存在.
- local-default sidecar与registry通过`jd`比较无diff,lease ID一致.
- SIGKILL PID 39323后,PID与UDP 7447释放,7个artifact保留;三把stable lock inode保持`512111038`、`512111044`、`512111045`.
- 同identity重启得到PID 84240,lease ID从`f37a519f-303e-47be-8af3-21095abf76a2`更新为`fa978b5e-3120-40c5-81e9-85259aaab566`,ping恢复.
- 收紧部分managed字段判别后重新安装,最终daemon PID 29465 ready;installed/release SHA-256一致,重复启动拒绝,前后ping成功.
