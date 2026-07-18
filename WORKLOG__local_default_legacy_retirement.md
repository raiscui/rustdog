# local-default legacy退役工作日志

## [2026-07-18 13:26:23] [Session ID: omx-1784304547353-h5409r] 任务名称: managed-only本地daemon发现与legacy迁移收口

### 任务内容

- 将Unix空target/self的正常owner契约收紧为完整managed registry + sidecar identity + active OS lock.
- 退役纯v1 PID liveness和唯一FIFO自动选择,保留FIFO候选作为升级诊断.
- 保留active legacy PID fail-closed升级门,验证stopped legacy在stable inode上原地迁移.
- 同步runtime测试、unixpipe e2e、配置模板、fast-path规格与`rdog-control` skill.

### 完成过程

- 用commit `893398c`旧二进制建立隔离矩阵,验证active old/new互斥、stopped legacy原地升级和旧版stale unlink行为.
- TDD RED证明纯v1 registry与唯一FIFO都真实参与自动发现;随后只修改被证实的两条路径.
- runtime测试fixture改为真实managed guard,避免纯v1 fixture造成启动宽限期假阳性.
- 完成自动验证、release构建、正式安装与tmux daemon受控重启.
- service/path/local-default三个stable lease inode在正式切换前后保持不变.

### 验证结果

- process lease:4 passed.
- runtime:34 passed.
- unixpipe e2e:12 passed.
- router-client:26 passed,2 ignored.
- all-targets check与release build:0 error;10条warning均来自未改动的control-act基线.
- live bare/self/显式target ping全部返回pong;duplicate exit 1且未破坏FIFO;registry与sidecar经`jd`比较无差异.

### 总结感悟

- 升级兼容输入和正常liveness真相源必须分层.同一个PID字段不能同时承担两种语义.
- 文件存在性只能说明transport artifact存在,不能证明哪个进程拥有它.
- 跨版本安全不仅是代码问题,还包含部署节点上的旧二进制和自启动项清理.

### 交付补充

- 提交前审查发现`specs/zenoh-control-plane-plan.md`与skill workflow reference仍有旧fallback文案,已明确撤回早先不完整的"全仓清零"结论并完成同步.
- 最终安装版hash为`96955460e968cc8ccaf06c1b4fc2bce888e4c5564df5b6f0cac69e348249cc75`,正式daemon PID 19047.
- 最终二次自动矩阵与live duplicate/ping验证均通过.
- 已创建单一scoped commit,没有吸收仓库外或无关工作区改动.
