# P1 iroh 只读闭环临时契约

状态：私有候选，限 IPv4 回环，待 [#113](https://github.com/raiscui/rustdog/issues/113) 与 [#114](https://github.com/raiscui/rustdog/issues/114) 评审。不是生产跨网接口或授权政策。

## 入口和目标键

同一 `rdog` 提供 `rendezvous -c <profile>`、`daemon --transport iroh -c <profile>`、`control --transport iroh --iroh-config <profile> NAMESPACE/NAME @ping#42`。配置是显式 JSON，各角色字段以 [src/control_iroh.rs](../src/control_iroh.rs) 的 profile 类型为准。目标键校验由同文件的 `name_valid` 定义。启用 iroh 必须显式指定 transport，不会改变既有默认选择；不接受交互 stdin、PTY、UI script 或其他命令。目录、daemon 与 relay 在本阶段只允许 IPv4 回环。客户端指定目录 IPv4 地址和预置 iroh 公钥，通过 iroh 验证目录身份。

## 注册与租约

`Registry::request` 把目标键与配置中批准注册的 iroh 身份绑定。注册时验证握手认证的远端身份、宣告的 endpoint ID、一个回环 IP 候选和本进程的 TLS relay 地址。目录只允许已配置的 lookup 身份查询，返回尚未过期的条目；查不到有效条目返回 `iroh_target_not_registered`，与后续传输失败或准入拒绝分开。租约与续租的默认值分别以 `LEASE` / `RENEW` 为准，目录重启清空条目。daemon 持有进程级 iroh endpoint，单条 stream/connection 完成不会关闭 endpoint。注册失败会阻止初始上线；续租失败到本地租约期满后停止接纳新会话。

这是 #114 的单进程、静态名称所有权验证。不实现全局唯一性、分布式仲裁、同一密钥被多个进程复用时的实例 fencing、注销协议或生产离线语义。

## 会话准入

目录查找、relay 接入、daemon 的目标授权分别判定。允许查找或使用 relay 不等于有权向目标发请求。daemon 在 iroh 握手完成后根据 `Connection::remote_id()` 和本目标名单判定，不信任请求里自行声明的身份。拒绝发生在解析应用帧之前，使用 `DENIED_CODE` / `DENIED`，没有成功 pong。获准会话也只接受一条通过既有 parser 的 `ControlCommand::Ping`。回复由既有 ping handler 和 `ControlPeerSession` 经 `ControlTransport::Iroh` 发送；没有通用 dispatcher、shell、文件或其他命令接入。

这是 #113 的临时静态身份验证。内嵌 HTTPS relay 用显式信任证书，QAD 复用服务端 TLS 配置；目录及数据通道采用 iroh 的身份握手。没有实现云端 usrpwd/mTLS 分发与轮换、短期一次性会话 grant、生产撤销、审计或部署配置。身份种子从显式文件读取，测试凭据仅用于单次本机实验。

## 路径策略

本轮只有小量只读 `@ping`。客户端 profile 的 `direct` 不配置 relay，`relay` 不建立直连 IP 传输。客户端核对实际选中的路径；失败就失败，不回退 TCP 或 Zenoh。本阶段只有发起方本地策略选择，不宣称已经实现云端有效策略下发。RAI-169 证实共享的可中继 QUIC 连接无法保证单独 bulk 流全程直连；本阶段不开放 bulk，也不把 direct-only 降为尽力直连。
