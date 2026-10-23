# yino 使用反馈修复记录

## [2026-10-23 23:24:29] [Session ID: 01a168b3-33af-7c53-9c6b-0fb808d7e9dc] 控制面诊断、后台取消与Windows终端窗口

### 现象与已验证机制

1. 后台cancel只kill父shell且未隔离Unix PGID, 旧回归中子进程取消后仍存活. 新实现复用共享进程树helper, registry锁外终止/收割, 失败保留child供重试. Native子树/无关进程及Windows双listener实测通过.
2. task/flow/裸shell未消费@cmd已有隐窗标志, taskkill辅助也未隐藏. 将隐藏策略收口到build_shell_command并覆盖taskkill. Windows builder/task/flow三处GetConsoleWindow都为0. 保留显式visible与独立PTY路径; 未复刻用户原命令/替换其共享daemon.
3. 新增队列遥测最初假设receiver drop会立即析构Flume缓存, 动态39/40推翻该假设. ChannelReceiver在同一诊断锁中按channelID清queued并阻止关闭后注册, active继续按执行生命周期计数. 原drop断言与关闭后发送断言在Native/Windows均通过.
4. 独立Ping与新observer连接绕过忙query FIFO, 业务请求仍顺序执行. 计数只覆盖已观测Zenoh ingress, 不含网络/内核缓冲和PTY原始stdin. 并发session-open持身份集合锁到订阅完成,失败撤销占位,避免提前ACK.

### 验证

Native40/40; Windows11项控制面全部通过, 加额外feature-off检查共12项. Windows真实observer open/Ping/close、目标listener关闭与无关listener保留均通过. 远端产物SHA与本地相同; 保护daemon字段jd无差异; 自有测试进程和临时目录已清理.

Windows无OCR构建暴露既有测试unwrap_err的Debug约束, 仅改err().expect提取错误值; 60s初始化等待已开 #127, 不宣称修复. 构建debug-map后处理与33条既有告警、全仓验证缺口见WORKLOG.
