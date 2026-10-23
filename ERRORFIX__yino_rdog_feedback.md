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

## [2026-10-24 00:44:01] [Session ID: 01a168b3-33af-7c53-9c6b-0fb808d7e9dc] 正式更新与空间清理验收

部署工具错误均已分类并处理: 未锁依赖安装已精确停止并改--locked; SSH旧stderr编码首次解析失败改显式UTF8输出并保留原始工具证据; 旧daemon不支持@task-list, 未据此推断registry为空, 使用无后代/无Established连接与日志核对排空. 部署脚本空集合/条件括号、SID与默认权限、枚举字符串及PS5.1无BOM中文解码问题均在停旧服务前阻断并修正, Windows真实只读预检通过. 初次脚本调用被ExecutionPolicy拒绝, 没有执行部署体; 改为仅该PowerShell进程的ExecutionPolicy Bypass后执行成功, 未修改系统/用户策略. 本机Zenoh client关闭时出现一条session closed发布ERROR, 请求已成功且daemon持续ready, 记录为客户端关闭日志, 未据此重启服务或扩大修复.

记账守卫首次严格检查exit1: 守卫只读取根级固定六文件, 支线WORKLOG中的254a275引用不会被识别. 将在根WORKLOG补引用并提交纯根账本, 不修改守卫或放宽检查.
