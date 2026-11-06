# 持续 OMP coding 会话

`rdog coding` 通过已认证的 Zenoh router 控制 Windows 上新建的 OMP 会话. 每个会话由独立 Host 持有. 控制客户端断开或 daemon 重启不会主动关闭该 Host. 重新连接同一 daemon 状态目录后, 可以查询会话、任务、交互和分页输出.

协议与配置的唯一真相源分别是 `src/coding/model.rs`、`src/coding/cli.rs`、`src/coding/store.rs`、`src/coding/manager.rs` 和 `src/coding/host.rs`. 本文描述使用方法和有意保留的边界, 具体字段以代码和 CLI `--help` 为准.

## 配置与信任

在 Windows daemon 的配置中启用 `[coding]`, 保持 `[auth]` 启用, 并在同一用户下配置可运行的 OMP profile 和模型. 默认关闭; 可设置 `enabled = true` 和所需的 `profile`. `omp_executable`、`state_dir` 和 `extensions` 如指定, 必须是绝对路径. `environment` 只提供给新 Host 及其子进程, 不写入会话 manifest 或 journal. 不要在命令行或公开文档里放凭据.

Host 当前仅支持 Windows. macOS 和 Windows 均可作为 Zenoh 客户端. 现有共享凭据认证提供的是现有链路的信任, 不是逐用户、租户隔离或端到端身份. 已有授权的同用户 shell 能力不因为其他入口拒绝 `@coding` 而消失. 如果 daemon 运行在不允许安全脱离的 Windows Job 中, 创建 Host 会明确失败. 应调整 daemon 的启动环境, 不要放宽 Host 生命周期检查.

Host 使用固定版本的 OMP Rust RPC SDK 和本机 stdio. 它创建新的 OMP, 不接管已有 Herdr 或交互式 OMP. Host 或机器崩溃后不会保证原进程继续运行, 也不会自动重发结果未知的任务.

## 提交与查询

以下示例中的 target、profile、路径和 request ID 需要替换为实际值. `--entry-point` 可按已有 Zenoh 部署需要提供.

```powershell
rdog coding --namespace lab --target windows.lab session open --request-id create-1 --cwd 'C:\work\project' --profile codex4
rdog coding --namespace lab --target windows.lab session status <session-id>
'请检查当前项目并报告结果' | rdog coding --namespace lab --target windows.lab task submit <session-id> --request-id task-1
rdog coding --namespace lab --target windows.lab task status <session-id> <task-id>
rdog coding --namespace lab --target windows.lab task output <session-id> <task-id> --limit 128
rdog coding --namespace lab --target windows.lab interaction list <session-id>
rdog coding --namespace lab --target windows.lab interaction respond <session-id> <interaction-id> --request-id answer-1 --kind confirm --confirmed false
rdog coding --namespace lab --target windows.lab task cancel <session-id> <task-id> --request-id cancel-1
rdog coding --namespace lab --target windows.lab session close <session-id> --request-id close-1
```

输出页返回不透明的 `next_cursor`. 后续请求使用 `--cursor <next_cursor>`. 空页且 cursor 不再变化表示当前没有更多已保存输出; 运行中任务稍后仍可能产生新内容. 不要把最后一页当成完整历史.

相同 request ID 和相同内容的重放返回原有裁决, 不再次派发. 相同 ID 对应不同内容会冲突. 创建成功的回执是稳定 handle, 不表示 Host 已经 ready; 实时状态由 `session status` 查询. 传输失败或 `unknown_outcome` 不证明请求未送达. 先用原 ID 查询或重放原请求, 不要换 ID 盲目再发.

## 交互与取消

`interaction list` 显示需要人工判断的交互. `confirm` 必须显式提供 true 或 false. `select` 的值必须是返回的选项之一. 对 select、input 和 editor 省略 `--value` 表示取消. 不自动批准安全请求. 对同一交互只能做一次有效裁决.

任务取消是协作式操作. 已接受取消不等于原生 OMP 已停止. Host 对尚未裁决的 dialog 发送取消响应, 再请求原生 abort. 只有相关结果或 abort 已确认, 后续新鲜的状态证明任务空闲, 且没有未决 dialog 时, 下一轮才可进入. 如果发送结果无法确认, Host 保守保留未知状态, 不重放请求或把未知写成完成.

OMP 的无 signal dialog 不一定因普通 abort 结束. Host 不能只凭 abort 请求或旧状态断言 dialog 已关闭. 相关判断和有序状态屏障由 `src/coding/host.rs` 与 `src/coding/store.rs` 实现.

`session close` 的 `closing` 回执表示关闭请求已接受, 不是进程已经退出. 后续使用 `session status` 核对关闭状态. 历史 journal 不因关闭自动删除. journal 不完整、损坏或超过配额时明确报错, 不静默截断或修复.

Host 因 journal 存储错误退出时, 如果 journal 可完整回放, 且预留的故障记录成功写入并通过身份及完整性校验, 离线的 session 和 task 状态会提供 `host_error` 对象, 包含 `code` 与 `message`. 任务的 `unknown` 仍表示结果未确认, 不能据此重发. 如果 journal 不完整或损坏, 查询保留对应的 journal 错误码; 状态查询在故障记录有效时将 Host 错误附在错误消息中. 如果预留记录也无法写入, 或 Host 异常消失时没有留下记录, 状态不能据此推断磁盘满. 故障记录的具体格式和校验以 `src/coding/failure.rs` 为准.
