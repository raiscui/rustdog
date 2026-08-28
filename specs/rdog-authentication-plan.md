# rdog 认证层方案 (usrpwd 先行 + TLS 机密性)

> 正式 spec 同步自 issue #79, 实施以 issue 为准。

## Problem Statement

跨主机场景下 rdog 的信任模型完全等于网络可达性: Zenoh 明文, 任何能连上 daemon router (tcp 7447 / UDP multicast / serial) 的主体都能下发任意 shell 与 GUI 控制。同时截图/键盘内容以明文流经网络 (截图天然含敏感信息)。伴生 agent 与 mailbox 上线后, 消息与卡片同样是明文广播 — 认证与机密性是 A2A 语义层 (Phase 4) 之前必须补的洞。

## Solution

分层两阶段, 先认证后机密性:

- **Phase A (认证, userpwd)**: 启用 zenoh 内置 session 层认证 (INIT/OPEN 扩展, 不依赖传输) — 覆盖 tcp/udp/unixpipe/**serial** 全部 transport, 配置面小, 是'轻量网络内信任'的正解 (非 Web PKI)。
- **Phase B (机密性, TLS)**: tcp transport 加 transport_tls, mTLS 双向认证; 截图/键盘/agent 消息的机密性。unixpipe 本机同用户天然私有, serial 视部署场景后议。

默认安全策略 (关键设计): daemon 启动即启用认证, 凭证自动生成于 `~/.rdog/auth.toml` (随机 user+password)。本机 local-default 场景 client 与 daemon 同用户读同一文件 — **零配置本机安全**; 远程 target 需要显式分发凭证 (安全默认, 不静默降级为无认证)。

## User Stories

1. As a 主机主人, I want daemon 拒绝没有正确凭证的连接, so that LAN 内任意主体不能控制我的机器。
2. As a 本机用户, I want `rdog control` 在 local-default 场景零额外配置即可通过认证, so that 本机工作流不被安全层拖累。
3. As a 远程操作者, I want 把目标主机的凭证装进我的 rdog 配置, so that 我能安全控制远程 target。
4. As a 操作者, I want 凭证不出现在 CLI 参数里, so that 进程列表不泄漏 secret。
5. As a 操作者, I want 凭证可经环境变量覆盖文件配置, so that CI/脚本注入不必落盘。
6. As a 伴生 agent, I want 携带凭证连接 daemon, so that agent 会话同样受认证保护。
7. As a 操作者, I want daemon 日志在认证失败时给出可诊断的拒绝原因 (不泄漏 secret 本身)。
8. As a 开发者, I want 测试基建能注入测试凭证, so that 全部 e2e 在认证开启下运行。
9. As a 主机主人, I want tcp 上的流量加密 (Phase B), so that 截图/键盘内容不被 LAN 嗅探。
10. As a 早期用户, I want 一个显式的过渡开关让我先关认证跑通, so that 迁移不被硬断 (带响亮警告日志)。

## Implementation Decisions

- **usrpwd 而非 pubkey 作为 Phase A**: pubkey 需要双向各持密钥对且 known_keys 管理 (上游 issue #1339 反映其配置陷阱); userpwd 的 shared-secret 模型与'网络内信任'心智一致, 分发简单。
- **zenoh 配置挂点**: daemon 侧 `transport/auth/usrpwd/users_file`; client 侧 config 注入 user/password。rdog 现有 figment 配置层加 `[auth]` 段 (figment 优先级: env > cwd toml > ~/.rdog/toml)。
- **凭证文件**: `~/.rdog/auth.toml` (0600), daemon 首次启动生成随机凭证; `RDOG_AUTH_USER`/`RDOG_AUTH_PASSWORD` env 覆盖。
- **过渡开关**: `[auth] enabled = false` 显式关闭, 启动时 WARNING 日志; 默认 true。e2e 测试基建默认带测试凭证。
- **unixpipe**: 同用户文件系统天然边界 + 凭证文件同读, 不需要特判。
- **Phase B 边界**: transport_tls feature + 证书生成/分发命令 (`rdog auth init-tls` 之类, 细节 Phase B 启动时再 spec), 不在本 spec 展开。

## Testing Decisions

- 单测: 凭证文件生成/权限/解析; env 覆盖优先级。
- e2e: 认证开启下全链 (control/PTY/task/agent messaging) — 改造现有 helper 注入凭证; 反向用例: 错误凭证被拒 + daemon 日志含拒绝原因。
- 先例: zenoh_router_client 子进程 e2e 模式; daemon-log sentinel 契约。

## Out of Scope

- Phase B TLS 实施细节 (独立 spec)
- Web PKI / OAuth 类企业体系 (A2A 调研已否)
- 旁观 topic / A2A 语义层 (Phase 4)
- serial transport 的加密 (无 TLS 语义, 仅认证覆盖)

## Further Notes

- 调研来源: zenoh 官方 [usrpwd](https://zenoh.io/docs/manual/user-password/) / [TLS](https://zenoh.io/docs/manual/tls/) / [authentication spec](https://spec.zenoh.io/spec/1.0.0/security/authentication.html)
- 关联: specs/rdog-task-spawn-control-plan.md (Phase 4 前置声明), specs/rdog-agent-messaging-plan.md
