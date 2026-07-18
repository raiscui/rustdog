# local-default 原子lease后续计划

## [2026-07-18 12:40:06] [Session ID: omx-1784340333160-6bwnss] 后续: 退役legacy unlink迁移窗口

### 当前边界

- 旧二进制不理解OS lease,仍会在判断PID stale后unlink guard path.
- 因此旧版和新版同时争抢同一个stale资源时,无法仅靠新版代码建立跨版本原子互锁.
- 当前规格与live流程已要求升级时先停止旧daemon,再启动新版.

### 后续触发条件

- 所有部署节点都升级到支持`rdog.process-lease.v1`的版本后.

### 建议措施

- 移除legacy create/unlink兼容分支,或提供一次性迁移命令检查并固化stable lock path.
- 增加跨主机版本矩阵测试,明确允许的滚动升级组合.

## [2026-07-18 12:40:06] [Session ID: omx-1784340333160-6bwnss] 后续: 拆分超长zenoh_runtime模块

- `src/zenoh_runtime.rs`已明显超过项目建议的1000行上限.
- 本轮已把process lease独立成子模块,但unixpipe path、local-default registry和session discovery仍集中在同一文件.
- 建议另开架构任务,按`session`、`unixpipe`、`local_default`职责拆分,保持当前public API不变并以现有38项runtime测试护航.
