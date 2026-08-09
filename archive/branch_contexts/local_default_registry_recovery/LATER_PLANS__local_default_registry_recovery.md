# local-default registry 后续计划

## [2026-07-18 10:22:47] [Session ID: omx-1784304547353-h5409r] 延期项: 生命周期与代码质量治理

- [ ] 把 local-default JSON、namespace PID guard和 path owner记录演进为可原子校验的单一状态源,同时处理 PID复用和中断写入.
- [ ] 为用户级 `zenoh-guards` 提供只读审计和按存活性定点清理工具;当前目录已有大量历史 stale PID文件,禁止无验证批量删除.
- [ ] 拆分1848行的 `src/zenoh_runtime.rs` 和1075行的 `src/zenoh_control.rs`,优先按 unixpipe registry/ownership与 router runtime职责拆模块.
- [ ] 清理 control-act工作线的6个 bin warning和8个 test warning,恢复 warning-free编译基线.
- [ ] 更新 lockfile中的已yanked依赖:`spin 0.9.8`、`spin 0.10.0`、`stabby 72.1.1`,完成兼容性回归后再提交升级.
- [ ] 评估并安全清理由旧 Zenoh client遗留的带数字后缀 FIFO;先确认1.8.0生命周期语义,不能按前缀直接删除活跃会话资源.
