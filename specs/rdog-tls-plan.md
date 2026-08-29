# rdog 认证层 Phase B - TLS 机密性方案 (tcp transport)

> 正式 spec 同步自 issue #85, 实施以 issue 为准。

## Problem Statement

Phase A (usrpwd, PR #84) 解决了认证: 未带凭证的连接被拒。但网络流量仍是明文 — 截图 (天然含屏幕敏感内容)、键盘事件、agent 消息、PTY 输出都以明文流经 LAN。跨主机使用 rdog 的前提是这些载荷的机密性。

## Solution

tcp transport 启用 zenoh TLS: 传输层加密 + 服务器证书验证; mTLS 可选 (双向证书认证, 与 usrpwd 叠加不互斥 — 一个管传输层一个管 session 层, 配置面独立)。

证书策略延续零配置哲学: `rdog auth tls-init` 用 Rust 内建生成 (rcgen) 自建 CA + daemon 证书到 `~/.rdog/tls/`, 客户端分发 CA + 客户端证书 (mTLS 时)。

## User Stories

1. As a 主机主人, I want tcp 上的 rdog 流量加密, so that LAN 嗅探拿不到截图/键盘/消息明文。
2. As a 操作者, I want `rdog auth tls-init` 一条命令生成全部证书材料, so that 不依赖 minica/openssl 等外部工具。
3. As a 远程 client, I want 只装 CA 证书即可加密连接 (服务器验证模式), so that 最小分发面。
4. As a 高安全部署, I want enable_mtls 双向证书认证, so that 客户端也受证书约束。
5. As a 操作者, I want [tls] enabled=false 时行为与现状完全一致, so that 迁移渐进。
6. As a 操作者, I want 证书过期被监控并在到期时断链, so that 不会静默降级。
7. As a daemon, I want unixpipe (本机同用户) 不被 TLS 波及, so that 本机 fast path 零开销不变。
8. As a 串口用户, I want serial 继续只走 usrpwd (无 TLS 语义), so that MCU 接入不受影响。

## Implementation Decisions

- **cargo feature**: zenoh `transport_tls` 加入主依赖 features (rdog 目前 default-features=false 未含)。
- **配置键** (zenoh 原生, rdog 只做包装): `transport/link/tls/{root_ca_certificate, listen_private_key, listen_certificate, enable_mtls, connect_private_key, connect_certificate}`。
- **证书布局**: `~/.rdog/tls/`: ca.pem + ca-key.pem (init 机器持有), daemon: {cert,key}.pem, mTLS 客户端: client-{cert,key}.pem。
- **endpoint 切换**: [tls] enabled 时 listen/connect endpoints 前缀 tcp/ → tls/; scout protocols 约束为 ["tls"] (避免明文 scout 混入)。
- **与 Phase A 关系**: 叠加不替代 — usrpwd 留在 session 层 (serial 也靠它), TLS 管传输加密; mTLS 开启后 usrpwd 冗余但无害。
- **rcgen 而非外部 minica**: 对齐零配置哲学与单二进制分发。

## Testing Decisions

- e2e: tls-init 生成 → daemon tls listen → client CA-only 连接 @ping 通; 错 CA 拒绝。
- 现有 e2e (unixpath/self) 不受影响回归验证。
- 证书过期断链 (close_link_on_expiration) 用短时效证书单测验证。

## Out of Scope

- QUIC transport (zenoh 也支持, 需求出现再议)
- 证书轮换自动化 / ACME (Let's Encrypt 类)
- Web PKI 公网信任

## Further Notes

- 调研来源: zenoh [TLS 手册](https://zenoh.io/docs/manual/tls/) — 证书键名/endpoint 格式/mTLS 双套证书语义均以官方文档为准。
- 前置: Phase A (#79-#83, 已 merge PR #84)。
