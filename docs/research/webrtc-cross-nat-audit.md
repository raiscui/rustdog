> **本文件是 wayfinder research 产出,仅存在于 `research/webrtc-cross-nat-audit` 分支,不会被合并。**
> 关联 ticket: #108(审计:WebRTC/ICE/STUN/TURN 路线的证据与反例) · 所属 map: #106
>
> **中文摘要**
>
> - **技术可行,但"云端只做握手"只成立于幸运路径。** RFC 4787 REQ-1:NAT 若不满足 EIM 映射,"**will force the use of a UDP relay**";§5:ADPF 过滤必然要中继。Tailscale 自己也说中继"picks up the slack when traversal fails, **as it likely will for a long tail of cases**";Headscale 内置 DERP 的文档原话:"**There are no speed or throughput optimisations**"。
> - **rdog 特有论据(本报告最重要的一条)**:rdog 的主力负载是**多 MB 的 base64 JPEG**(`MAX_CONTROL_MESSAGE_BYTES` = 8 MiB,`@savefile` 单行)。别的工具做 P2P 是**省延迟**,rdog 做 P2P 是**省带宽** —— 1 Mbps 上行下单张 8 MiB 截图 ≈ **67 秒独占上行**。这把 P2P 从"锦上添花"提到了"架构必需"的位置。
> - **可行组件**:`webrtc` 0.21.1(纯 Rust、rustls 0.23、**无 OpenSSL**、823 万下载、5.1k star);`str0m` 0.24.1(Sans-I/O,更贴合 rdog 的**线程式** daemon,不需要在 `src/` 引入 async runtime);`coturn` 一个进程同供 STUN + TURN,UDP/TCP 3478 + TLS 5349,**RFC 6062 的 TCP 中继默认开启**(封 UDP 的企业网逃生口),TURN REST API(`timestamp:username` + `base64(HMAC-SHA1)`)正好对上已锁定的"数据通道短期凭证"。crates.io **无 Rust TURN server crate**(`rtc-turn` 只是客户端,`pion/turn` 是 Go)→ 自研 TURN server 是高风险。
> - **关键冲突**:SCTP 默认消息上限 256 KiB,RFC 8831 §6.6 要求"未实现 RFC 8260 interleaving 时 SHOULD ≤ 16 KB",而 rdog 行上限 8 MiB = **32× 默认值** → WebRTC lane 必须传**字节流 + 传输层分帧**,不能传行;一旦加了分帧层,就是在重新实现 TCP 免费给的东西。
> - **最强反例**:①**直接 TCP/WS 中继** —— rdog 已有 `ControlTransportKind::Tcp` 与 `::WebSocket` 两条 lane,零新依赖、8 MiB 行、PTY 双向流、封 UDP 网络全都能过,唯一损失是 P2P;②**Headscale/DERP**(44.8k star)—— 但 DERP 本身**不给 P2P**(Tailscale 的 P2P 来自 WireGuard + disco + STUN),且要引入 Go 二进制 + VPN 模型,rdog 自己的 usrpwd/mTLS 变成第二层。
> - **仓库有利事实(降低实施成本)**:`ControlTransportKind { Tcp, WebSocket }`(`src/control_transport.rs:42`)+ 共享 `send_control_lines_*` 管线(`specs/control-line-protocol.md:960`)→ 新增第 4 条 lane 是**机械性低成本**;rdog 已有应用层分块 `@file-chunk` / `@file-read`(chunk ≤ ~6.4 MiB)可直接复用;zenoh session channel 的 payload 本来就是一行文本,**信令不用自造协议**。
> - **明确未核验**:真实网络下的 P2P 成功率**无 rdog 自身实测**(矩阵仅由 RFC 推导);用户侧 VPS 上行带宽与月流量额度未核验 —— 二者都是 ticket #117(实测环境)要补的洞。

<task_output status="succeeded">
<task_output_cursor next_offset="65536" truncated="true" />
I'll start by reading the repo files that ground the constraints, then move to external evidence gathering.
Repo indexed. Now reading the transport abstraction and config surface to ground constraints.
Zenoh 1.8.0 with serial/tcp/udp/tls transports — noted. Now the config surface and protocol spec.
8 MiB max single line is the key payload constraint. Now let me check screenshot sizing and the config surface in detail.
Now the protocol specs — I need the exact wire semantics that must be preserved.
Now the PTY plan and the Zenoh control-plane plan (the relay baseline I need to evaluate).
Repo constraints are clear. Let me confirm the Zenoh session-channel keyexpr layout and the existing WebSocket transport, then move to external evidence.
Repo grounding is complete. Now the external evidence — starting with the `webrtc` crate, coturn, and Headscale.
Strong data on the `webrtc` crate and coturn. Now the load-bearing protocol facts: DataChannel message limits and NAT behavior discovery.
datatracker is Cloudflare-gated; switching to rfc-editor.org (the canonical RFC source).
Critical finding on message size limits. Now checking the Sans-I/O core's DataChannel config and whether rdog already carries an async runtime.
Tokio is already in the tree via Zenoh. Now the SCTP message-size limit and the NAT behavior matrix.
Now the SCTP config defaults, plus the Headscale/DERP and Zenoh NAT-traversal questions.
Excellent — SCTP defaults confirmed. Now the Headscale/DERP refutation evidence and the relay-cost question.
Now the Headscale/DERP architecture, DERP relay behavior, and Rust TURN-server alternatives.
Strong evidence base now. Final repo verification on the Zenoh session-channel data flow and the transport framing contract.
One last check — rdog's existing application-level chunking protocol, which bears directly on the SCTP message-size limit.
Repo grounding is complete. One final check on screenshot payload sizing and any prior WebRTC thinking in the repo.
I have a complete evidence base across repo constraints, the Rust WebRTC stack, coturn, RFCs, and the Tailscale/Headscale refutation. Here is the report.

---

# WebRTC Data-Channel Lane for rdog Cross-NAT Control — Evidence Report

Repo: `/Users/cuiluming/local_doc/l_dev/my/rust/rustdog` @ v3.0.0. No repo file was modified.

---

## 1. Verdict

A WebRTC data-channel lane is **technically sound and implementable in Rust** — the `webrtc` crate 0.21.1 is a pure-Rust, actively-maintained stack with working TURN-relay, STUN-srflx, ICE-restart, and data-channel flow-control examples, and coturn serves STUN+TURN from one process with a TURN REST API for short-lived credentials. But the **"cloud node = handshake server only" premise is the weak link, not the transport**: RFC 4787 states plainly that any NAT failing Endpoint-Independent Mapping "will force the use of a UDP relay", and Tailscale's own engineering writeup says the relay picks up "the slack when traversal fails, as it likely will for a long tail of cases" — Headscale ships an embedded DERP relay precisely because direct often fails, and describes it as having "no speed or throughput optimisations". So the cloud is not a handshake server; it is **a mandatory relay plus an optional handshake**, and the handshake only ever *removes* the relay when the NATs cooperate. For rdog specifically the P2P case is **stronger than usual** — the dominant payload is a multi-MB base64 JPEG `@savefile` frame, which is exactly the traffic that would saturate a small VPS uplink — so P2P has real value, but it is an *optimization on top of* a relay, not a substitute for one. Meanwhile the SCTP message ceiling (256 KiB default, "SHOULD limit to 16 KB" without RFC 8260 interleaving, which `rtc-sctp` does not implement) collides head-on with rdog's 8 MiB single-line limit, and coturn's 300 s permission / 3600 s allocation lifetimes impose keepalive obligations that a DERP-style relay does not. **Recommendation by evidence: build the cloud relay first (it is required anyway and is ~90 % specified in `specs/zenoh-control-plane-plan.md`), and add the WebRTC P2P lane as an optional `ControlTransportKind` variant later** — because the transport abstraction in `src/control_transport.rs:42` and the shared `send_control_lines_*` pipeline in `specs/control-line-protocol.md:960` make the P2P lane a drop-in that preserves the line protocol byte-for-byte either way.

---

## 2. Repo-grounded constraints (verified, not assumed)

A new transport lane must satisfy all of these:

| Constraint | Evidence | Consequence for a WebRTC lane |
|---|---|---|
| Transport is **message-oriented**: one line per `write_message`/`read_message` | `src/control_transport.rs:42-47` (`ControlTransportKind { Tcp, WebSocket }`), `:107-119` | Adding a lane = add a `ControlTransportKind` variant + a `ControlTransport` arm. Protocol untouched. |
| **Single-line ceiling = 8 MiB**, single source of truth | `src/control_line_read.rs:30` — `MAX_CONTROL_MESSAGE_BYTES = 8*1024*1024`; comment: "既能容纳 4K 截图的 base64 载荷（数 MB 量级）" | **Directly collides with SCTP message limits.** A WebRTC lane must chunk at the *transport* layer (byte-stream framing) or route bulk through rdog's existing chunked path. |
| WebSocket payload capped at the same 8 MiB | `src/control_transport.rs:652` | Precedent: a non-TCP transport already carries the same payload class. |
| **rdog already has application-level chunking** | `src/control_file_transfer.rs:8-13` (`@put-file`/`@file-chunk`/`@file-end`, `@get-file`/`@file-read`), `:42-44` (`CHUNK_BASE64_LIMIT = 8MiB*4/5 ≈ 6.4 MiB`, comment "chunk 永远塞得进一行") | Big payloads already have a chunked representation. A WebRTC lane can borrow it rather than inventing one. |
| **The transport seam already exists for 3 transports** | `specs/control-line-protocol.md:960-963`: "统一复用 `send_control_lines_*` 管线(TCP / WebSocket / Zenoh 三套入口都对应同名函数)" | A 4th lane is mechanically cheap. This is the single most important repo fact for the whole decision. |
| PTY frames are **base64 text lines**, bidirectional, long-lived | `specs/control-line-protocol.md:621-668`; `specs/pty-control-plan.md` frames `@pty-ready`/`@pty-stdin`/`@pty-resize`/`@pty-output`/`@pty-exit`/`@pty-detached`/`@pty-attached` | A reliable-ordered bidirectional byte lane satisfies this; SCTP adds nothing PTY-specific. |
| `@screenshot` returns **multiple `@savefile` frames then a final `@response`** | `specs/control-line-protocol.md:721-722` | Multi-frame ordering must be preserved — easy over a reliable ordered channel, awkward over a message channel with size caps. |
| Zenoh session channel already carries a **line** in a text payload | `src/zenoh_control/session_payload.rs:64-65` — `render_session_bridge_payload` = `"__rdog_session__:{session_id}\n{line}"`; keyexprs `rdog/{ns}/session/{id}/to-daemon|to-control` (`src/zenoh_identity.rs:231-264`) | **The crossover is already half-built** (see §9). |
| Zenoh 1.8.0 features: `transport_serial, transport_tcp, transport_udp, auth_usrpwd, transport_tls` (+`transport_unixpipe` on unix). **No `transport_websocket`.** | `Cargo.toml` | No zenoh-native WebSocket or NAT-traversal transport to lean on. |
| Tokio 1.51.0 **already in the lockfile** (via zenoh, sck-rs, cidre, tls-listener); rdog's own `src/` uses **zero** tokio | `Cargo.lock` (tokio pulled by 19 packages); `grep -rn tokio src/` → only a comment | `webrtc` (runtime-tokio) adds **no new runtime dependency** — but rdog's daemon is thread-based and would need to host a tokio runtime. |
| **No prior WebRTC/ICE/TURN thinking exists in the repo** | `grep -rniE "webrtc|data channel|ice\b|turn server"` over `src/ specs/ docs/` → no hits | Greenfield. Any WebRTC lane is new surface area, not an extension of existing work. |
| `DEFAULT_SCREENSHOT_QUALITY = 75`; JPEG, base64-encoded, one line | `src/control_protocol.rs:143`; `src/screenshot.rs:335-345` | Screenshot is the dominant payload. |

---

## 3. A. WebRTC data channel feasibility

### A1. The `webrtc` crate — mature enough, pure Rust, but actively churning

| Property | Value | Source |
|---|---|---|
| Version | **0.21.1**, published 2026-10-11 | crates.io API `crates/webrtc` |
| License | **MIT / Apache-2.0** (GitHub reports Apache-2.0) | crates.io API; github.com/webrtc-rs/webrtc |
| Popularity | 8,234,620 total downloads; 3,063,366 in recent window; 72 versions; created 2018-08-16 | crates.io API |
| Popularity (repo) | **5,161 stars, 524 forks, 24 open issues**, last push 2026-10-11 | github.com/webrtc-rs/webrtc API |
| Architecture | Thin async layer over a **Sans-I/O** core (`rtc` 0.21.1) with a driver-based `PeerConnectionDriver` | docs.rs/webrtc/0.21.1/webrtc/ |
| **Pure Rust? YES** | `rtc` 0.21.1 deps: `rtc-crypto`, `rtc-dtls` (**rustls 0.23**), `rtc-sctp`, `rtc-ice`, `rtc-stun`, `rtc-turn`, `rtc-sdp`, `rcgen`, `x509-parser` — **no OpenSSL**. Crypto via `crypto-ring` (default) or `crypto-aws-lc-rs`. | docs.rs/rtc/0.21.1/rtc/ dependency list |
| Runtime | `runtime-tokio` (default), `runtime-smol`, `runtime-mock`; also a `custom-runtime` trait | crates.io features; docs.rs |
| Crate size | 394,500 bytes (unpacked) | crates.io API |
| Release cadence | 0.21.0-rc.1 (Aug 2026) → rc.2 (Sep) → 0.21.0 (Sep 19) → 0.21.1 (Oct 11). **~3 releases/quarter, with RCs.** | crates.io version list |

**Caveats (honest):**
- **Fast-moving API.** 0.20.5 → 0.21.0 was a breaking rewrite (edition 2021 → 2024, 20,783 → 8,554 LoC across 86 → 22 files). Pin exactly; expect churn.
- **0.17.2 (the version pinned in the `rtc` dev-deps) had `openssl` / `vendored-openssl` features** — meaning the OpenSSL-free pure-Rust posture is a *0.20+* property, not a historical one. Verified from crates.io version feature lists.
- **24 open issues** on a 5k-star repo is healthy, not alarming.
- **A serious alternative exists**: `str0m` 0.24.1 (2,614,881 downloads, 38 versions) — "WebRTC library in Sans-IO style" by algesten, plus `is` 0.11.1 (ICE Sans-I/O) from the same author. Sans-I/O fits rdog's thread-based daemon far better than an async runtime, and its author is also behind `librice`/`rice-c` (ICE RFC 8445). **If a WebRTC lane is chosen, `str0m` deserves a serious bake-off against `webrtc`.**

### A2. DataChannel semantics vs. what rdog needs — the message-size problem is real

**What rdog needs:** a reliable, ordered, bidirectional, long-lived byte stream with backpressure, carrying lines up to 8 MiB.

**What SCTP/DataChannel gives:**
- Reliable + ordered: `RTCDataChannelInit { ordered: true, max_retransmits: None }` → reliable (`docs.rs/rtc/0.21.1/rtc/data_channel/`, example in module docs). ✅
- Backpressure: `webrtc` `data_channel` trait exposes `send`, `try_send`, and **send-buffer back-pressure** (module doc, "Where to Start" table). Upstream examples `data-channels-flow-control`, `data-channels-flow-control-multi`. ✅
- **Message size: THIS IS THE PROBLEM.** Verified from `rtc-sctp` 0.21.1 source (`raw.githubusercontent.com/webrtc-rs/rtc/main/rtc-sctp/src/config.rs`):

```rust
pub(crate) const DEFAULT_MAX_MESSAGE_SIZE: u32 = 262144;   // 256 KiB
pub(crate) const INITIAL_MTU: u32 = 1191;
pub(crate) const RECEIVE_MTU: usize = 8192;
pub(crate) const INITIAL_RECV_BUF_SIZE: u32 = 1024 * 1024;  // a_rwnd
```

  rdog's ceiling is **8,388,608 bytes** — **32× the SCTP default.** And RFC 8831 §6.6 is explicit:

  > "The SCTP base protocol specified in [RFC4960] does not support the interleaving of user messages. Therefore, sending a large user message can monopolize the SCTP association. To overcome this limitation, [RFC8260] defines an extension to support message interleaving, which SHOULD be used. **As long as message interleaving is not supported, the sender SHOULD limit the maximum message size to 16 KB to avoid monopolization.**"
  > "It is recommended that the message size be kept within certain size bounds, as applications will not be able to support arbitrarily large single messages. This limit has to be negotiated, for example, by using [RFC8841]." (RFC 8841 = SDP `max-message-size`)

  `rtc-sctp`'s own crate doc lists only **RFC 3758 (PR-SCTP)** and **RFC 6525 (stream reset/reconfiguration)** as extensions — **RFC 8260 interleaving is not listed**. So the 16 KB SHOULD applies in practice, and the 256 KiB default already violates it.

  **Consequence:** an 8 MiB `@savefile` line cannot be a single DataChannel message. Three options, all of which must be decided in a spec:
  1. **Transport-layer chunking** — the lane carries a length-prefixed byte stream; the receiver reassembles into a byte stream; the existing `read_line_limited` does the rest. **Preserves the line protocol byte-for-byte.** This is the correct answer, and it costs one framing layer inside the transport, not in the protocol.
  2. **Raise `max_message_size`** via `TransportConfig::with_max_message_size` — allowed by the API, but re-introduces head-of-line blocking on the association (a screenshot transfer would stall PTY output on the same SCTP association).
  3. **Separate DataChannels** (bulk vs. control/PTY) — SCTP has multiple streams; `data-channels-flow-control-multi` exists upstream. Mitigates but does not remove the shared congestion window / shared path.

- **A genuine strength of the WebRTC design** (and an argument *for* it): `INITIAL_MTU = 1191` is explicitly derived as `1280 - 40 IPv6 - 8 UDP - 4 TURN ChannelData - 37 DTLS` — "the exact TURN-relayed IPv6 budget (pion/sctp#476, webrtc-rs/webrtc#807)". The upstream author already did the TURN-relay MTU derivation, chose the value to fit the **IPv6 minimum MTU of 1280**, and every SCTP packet becomes exactly one DTLS record in one UDP datagram → **no IP-layer fragmentation at all**. That directly dodges the "UDP fragmentation filtering" killer (RFC 5780 §3.5: some NATs "discard fragments under load"). This is a real, verified engineering advantage over a bespoke UDP approach.

### A3. Simpler alternatives that keep "handshake then direct path" without WebRTC

| Alternative | Reality | Evidence |
|---|---|---|
| **Bespoke UDP hole punching + STUN** | Reimplementing ICE candidate exchange, peer-reflexive candidates, nomination, keepalive, and re-punching after NAT rebinding. This is the exact work ICE standardized. **Do not hand-roll.** | RFC 5780 §1: "NAT devices do not behave consistently enough to predict future behavior"; §2: "under load NATs may transition to the most restrictive filtering and mapping behavior". Tailscale: "This is all quite complicated! It's one of those problems that are fun to explore, but quite fiddly to get right." |
| **QUIC / HTTP3** | QUIC gives a reliable multiplexed UDP transport with its own connection migration, but **no NAT traversal**: you still need a server to meet, and QUIC has no STUN/TURN/ICE equivalent. Using QUIC + a rendezvous server + custom hole punching = reimplementing ICE on top of QUIC. | Absence of NAT-traversal features in QUIC; the only QUIC-native NAT mechanism is a plain address-exchange handshake, and symmetric↔symmetric still fails. |
| **Plain TCP relay over the cloud** | Satisfies *every* rdog requirement (reliable, ordered, bidirectional, 8 MiB+ lines, PTY) with **zero new dependencies** — rdog already has `ControlTransportKind::Tcp` and `::WebSocket`. Loses only the P2P optimization. | `src/control_transport.rs:42-47` |
| **WireGuard mesh (Tailscale-style)** | Solves the whole problem, but binds rdog to a full VPN stack and a kernel/userspace tunnel, and changes the security model (network-level vs. application-level auth). | `specs/rdog-authentication-plan.md` (usrpwd session auth) and `specs/rdog-tls-plan.md` define an application-level auth model that a VPN layer would sit awkwardly on top of. |

**Verdict on A3:** if P2P is genuinely required, use an *existing* ICE implementation (`webrtc` or `str0m`). If the requirement can tolerate a relay, skip it entirely.

---

## 4. B. STUN/TURN server side (the cloud node)

### B4. coturn — one process, STUN + TURN, UDP + TCP + TLS, TURN REST API

All from the official `README.turnserver` (raw.githubusercontent.com/coturn/coturn/master/README.turnserver):

- **One process serves both roles.** Default `-p/--listening-port` **3478** for UDP **and** TCP; `--tls-listening-port` **5349** for TLS/DTLS. ALPNs `stun.turn` and `stun.nat-discovery`. `--stun-only` / `--no-stun` to split roles if desired. **Note: "The TURN server 'automatically' recognizes the type of traffic"** — the plain and TLS ports are functionally equivalent.
- **TURN REST API (the standard way an app issues short-lived TURN creds)** — exact mechanics quoted from the README:
  - Enable with **`--use-auth-secret`**; supply the secret via **`--static-auth-secret=<secret>`** (or the `turn_secret` DB table / Redis).
  - `temporary-username = "<timestamp>" + ":" + "<username>"` where timestamp = seconds since 1970. The separator is configurable via **`--rest-api-separator` (default `:`)**.
  - `temporary-password = base64(hmac-sha1(shared-secret, temporary-username))`.
  - **`--realm` is required** alongside the REST API. The client's key is `MD5(username ":" realm ":" SASLprep(password))` (RFC 5389 §10.2 / §15.4).
  - Source: "This is all formally described in Justin's Uberti TURN REST API document" (the `draft-uberti-behave-turn-rest-api` series; never became an RFC — it remains an expired I-D). **Caveat to flag: the TURN REST API is a de-facto standard, not a Standards Track RFC.**
- **Long-lived-session obligations (important, and easy to miss):**
  - `--max-allocate-lifetime` default **3600 s** → the TURN allocation is destroyed after an hour and must be re-created.
  - `--permission-lifetime` default **300 s**, and the README says this "MUST not be changed for production purposes" → permissions must be refreshed at least every 5 minutes.
  - `--channel-lifetime` default **600 s**.
  - `--stale-nonce` default 600 s.
  - **A long-lived rdog control session must run TURN keepalive/permission-refresh timers, and an allocation expiry forces an ICE re-negotiation or a session break.** A DERP-style relay has no equivalent obligation — it is one long-lived HTTPS/WebSocket connection. This is a genuine, concrete operational cost of the TURN approach for rdog's long-running daemons.
- Other relevant keys: `--min-port`/`--max-port` (default 49152–65535, the RFC 5766 relay range), `--max-bps`/`--bps-capacity`/`--user-quota`/`--total-quota`, `--rfc5780` (NAT behavior discovery — **requires ≥2 listening IPs of the same family**; the README notes RFC 5780 adds attributes that "increase the possibility of an amplification attack"), `--alt-listening-port`, `--mobility` (MICE), `--denied-peer-ip`/`--allowed-peer-ip`, `--allow-loopback-peers` (test only), `--no-udp`/`--no-tcp`/`--no-tls`, `--prometheus`.
- **Build requirement: OpenSSL 3.0+** (noted three times in the README). This is on the VPS, not in rdog's binary — acceptable, but it means the TURN server is a C binary with an OpenSSL dependency on the cloud side.
- COTURN is C + libevent2. The wiki page (`TURN-Performance-and-Load-Balance`) states allocations are cheap ("an allocation does not have its own thread or process... shares lots of system resources"), that scaling is primarily a *network throughput* problem, and that the "TURN server performance mostly depends on how efficiently the operational system handles the TCP/IP stack operations" (RSS/RPS). It also confirms the TURN-TCP (RFC 6062) interop constraint: TCP relay and RTP/RTCP pairs from the same client IP must land on the same server.

### B5. Alternative TURN servers

| Server | Status | Evidence strength |
|---|---|---|
| **coturn** (C) | De-facto standard. STUN+TURN, UDP/TCP/TLS, TURN REST API, RFC 5780, MICE. | **Verified** from official README. |
| **eturnal** (Erlang/OTP) | Real alternative; I did **not** fetch its docs in this pass, so I cannot assert its feature parity on the TURN REST API or RFC 5780. | **Unverified** — flagged, not asserted. |
| **Rust TURN server** | **No TURN *server* crate surfaced on crates.io.** A crates.io search for `stun turn ice` returned only *client/parsing* crates: `webrtc`, `str0m`, `is`, `rtc`, `stun_codec`, `stun-rs`, `stun-types`, `stun-proto`, `librice`/`rice-c`/`rice-proto`, `rustrtc`, `apprtc`. Inside the webrtc-rs stack, `rtc-turn` is a **client** library. `pion/turn` is Go. | **Verified by negative search** (crates.io API, 66 total matches scanned). Conclusion: **write your own TURN server in Rust = do not.** Use coturn. |
| **Embedded relay instead of TURN** | Headscale's DERP is a *different* relay protocol (HTTPS/WSS + STUN), not RFC 5766 TURN. See §6. | Verified. |

### B6. TURN over TCP (RFC 6062) — supported by coturn **by default**

From the coturn README's flag list:
- **`--no-udp-relay`** — "Do not allow UDP relay endpoints defined in RFC 5766, **use only TCP relay endpoints as defined in RFC 6062**."
- **`--no-tcp-relay`** — "Do not allow TCP relay endpoints defined in RFC 6062, use only UDP relay endpoints as defined in RFC 5766."

Because both `--no-*-relay` flags exist and are off by default, **both UDP and TCP (RFC 6062) relay endpoints are enabled by default.** No config change is needed to enable the TCP escape hatch; the key to *disable* TCP relay is `--no-tcp-relay`.

**Important non-obvious advantage of TURN-TCP here:** RFC 6062 relays a UDP-like datagram stream over TCP, so it does **not** suffer the TCP-over-TCP meltdown that plagues a naive "TCP relay in front of a TCP session" design. If rdog's relayed payload is SCTP/DTLS (as in a WebRTC lane) or a raw byte stream, TURN-TCP is viable. This is a point in favor of the WebRTC/TURN lane over a plain-TCP-relay lane for the *same* relay semantics — though the plain-TCP relay is simpler and has no such problem to solve because it *is* the TCP.

---

## 5. C. NAT reality — the part that decides everything

### C7. Authoritative behavior matrix

**Use RFC 4787's taxonomy, not cone terminology.** RFC 4787 §3 explicitly deprecates the cone language:

> "STUN [RFC3489] used the terms 'Full Cone', 'Restricted Cone', 'Port Restricted Cone', and 'Symmetric' ... **Unfortunately, this terminology has been the source of much confusion, as it has proven inadequate at describing real-life NAT behavior.** This specification therefore refers to specific individual NAT behaviors instead of using the Cone/Symmetric terminology."

RFC 4787 §4.1 mapping: **Endpoint-Independent Mapping (EIM)** / **Address-Dependent Mapping (ADM)** / **Address-and-Port-Dependent Mapping (ADPM)**. §5 filtering: **Endpoint-Independent Filtering (EIF)** / **Address-Dependent Filtering (ADF)** / **Address-and-Port-Dependent Filtering (ADPF)**. "Symmetric NAT" ≈ ADPM with per-destination port randomization.

**The two authoritative rules that decide everything:**

> **RFC 4787 §4.1, REQ-1:** "A NAT MUST have an 'Endpoint-Independent Mapping' behavior. Justification: In order for UNSAF methods to work, REQ-1 needs to be met. **Failure to meet REQ-1 will force the use of a UDP relay, which is very often impractical.**"

> **RFC 4787 §5:** "When the endpoints use ICE, **if NAT-A uses Address and Port-Dependent Filtering, connectivity will require a UDP relay.** However, if NAT-A uses Endpoint-Independent Filtering or Address-Dependent Filtering, ICE will ultimately find connectivity without requiring a UDP relay."

**Matrix (both endpoints behind NAT; ICE symmetric role/regular nomination; TURN available as fallback):**

| Mapping (A) | Mapping (B) | Direct P2P? | Notes & authority |
|---|---|---|---|
| EIM | EIM | **Yes** | Both have a stable reflexive address; ICE forms srflx↔srflx and host↔srflx pairs. RFC 4787 REQ-1 compliant. |
| EIM | ADM | **Usually yes** | B's mapping to A's address:port differs from B's mapping to the STUN server, but ICE's peer-reflexive candidate + simultaneous opening covers it. See the cross-check note below. |
| EIM | ADPM ("symmetric") | **Probabilistic / usually no** | B's mapping to A ≠ B's mapping to the STUN server, so the srflx guess misses. Tailscale's "birthday paradox" port-prediction trick makes it *probabilistic*, not reliable. RFC 4787 REQ-1: failure "will force the use of a UDP relay". |
| ADM | ADM | **Usually yes** | ICE simultaneous opening; peer-reflexive candidates. |
| ADPM | ADPM | **No** | **The canonical failure case.** Each side's mapping to the other is undiscoverable from the STUN server's view. Relay is the only reliable path. |
| Any | Any, but one side **ADPF-filtering + ADPM-mapping** | **No** | RFC 4787 §5: ADPF → "connectivity will require a UDP relay". |

**Cross-check note (official vs. real-world disagreement, flagged as requested):** RFC 4787 §5's blanket statement that ADPF filtering "will require a UDP relay" is stricter than real-world ICE behavior — in practice ICE establishes **direct** connections between two port-restricted-cone (EIM+ADPF) NATs via *simultaneous* opening plus peer-reflexive candidate handling, which is exactly why WebRTC works for most consumer users. Resolution: treat RFC 4787 §5 as the *guaranteed* bound and simultaneous opening as the *practical* improvement on top. **Do not rely on the practical improvement for correctness** — the relay must exist regardless.

**Other authoritative constraints:**
- **NAT mappings expire.** RFC 4787 REQ-5: "A NAT UDP mapping timer MUST NOT expire in less than two minutes" (5 min+ RECOMMENDED); REQ-6: outbound refresh MUST be True. → **A long-lived rdog session with no outbound traffic loses its binding.** ICE keepalives / STUN bindings solve this, but it means a periodic-timer obligation on the daemon.
- **NATs can change behavior under load.** RFC 5780 §2: "under load NATs may transition to the most restrictive filtering and mapping behavior and shorten the lifetime of new and existing bindings... applications can discover how bad things currently are, but not how bad things will get." → **A path that works at connect time can silently degrade.** The design must include path monitoring and fallback, not just connect-time negotiation. Tailscale: "keep a set of 'warm' fallbacks ready to go."
- **Non-deterministic NATs exist.** RFC 4787 §8 / REQ-11: on a port conflict, a NAT may switch from EIM to ADPM "without 'Port Preservation' upon detection of these conflicting sessions establishments". → A working P2P path can break mid-session when an unrelated app on the LAN takes a port. **This is a strong argument for keeping a live relay path warm rather than tearing it down after negotiation.**
- **Fragment handling varies.** RFC 5780 §3.5/§11: "some platforms discard fragments under load"; the PADDING attribute exists to probe this. → favors designs that avoid IP fragmentation (see A2: webrtc-rs's 1191-byte MTU does exactly this).
- **CGN (RFC 6888) and double-NAT.** RFC 4787 §4.4 describes the ISP-private-IP + home-NAT case. Double NAT with EIM on both layers still permits hole punching; a **symmetric CGN** outer layer breaks it.

### C8. The realistic outcome for "home broadband behind NAT" + "cloud VPS"

**For the common case (consumer router, EIM mapping per RFC 4787 REQ-1, ADF or ADPF filtering): direct P2P is the expected outcome**, provided no layer is symmetric and no middlebox blocks UDP. Home routers overwhelmingly implement EIM mapping — that is what RFC 4787 REQ-1 mandates, and it is why "restricted cone" NATs still permit WebRTC.

**But the three-party shape matters:** the scenario has the control client in a *third, independent* NAT domain. The P2P session that matters is **client(NAT-C) ↔ daemon(NAT-B)** — a 2-party ICE session, with the cloud only signaling. So the cloud is a pure signaling server *for that session*, and the relay is used only if NAT-C or NAT-B is non-conforming. **However**, if a future feature needs daemon-A ↔ daemon-B (e.g. agent messaging or task spawn across daemons, per `specs/rdog-task-spawn-control-plan.md`), that is a *second* 2-party ICE session with its own negotiation and its own fallback — the cloud still only signals, but the number of independent negotiations grows.

**Which side uses a TURN allocation:** in ICE, whichever candidate pair wins. In the common home-broadband case, **neither** — the direct host/srflx pair wins and no TURN allocation is created. In the failure case, the TURN allocation is made by **one** side (the one that is not the "controlled" endpoint) and the traffic flows: client → TURN(cloud) → daemon (or client ← TURN ← daemon for the return direction, using the TURN allocation as a peer). **The cloud then carries 100 % of the payload for that session.**

**The honest summary for the user:** *the cloud becomes a relay in a minority of sessions but carries the entire payload in those sessions* — so "handshake only" is true only in the happy path, and the *aggregate* cloud bandwidth is dominated by the minority that fall back. This is precisely the Tailscale/DERP model, and precisely why the relay must be built first.

### C9. Killers to check

| Killer | Verdict | Evidence |
|---|---|---|
| **Corporate UDP blocking** | **Real and must be handled.** Tailscale: "We'll probably also still want fallback relays that use a well-like protocol like HTTP, to get out of networks that block outbound UDP." A corporate network that blocks outbound UDP entirely kills ICE (and TURN-over-UDP) — you need **TURN over TCP/TLS** (RFC 6062, on by default in coturn) or a DERP-style HTTPS relay. | Verified (coturn README flags; Tailscale blog) |
| **UDP fragmentation filtering** | **Low risk in a WebRTC design, higher risk in a bespoke UDP design.** webrtc-rs pins `INITIAL_MTU = 1191` (one DTLS record per UDP datagram, no IP fragmentation), sized to the TURN-relayed IPv6 minimum MTU. | Verified (rtc-sctp source) |
| **Symmetric NAT on both sides** | **Fatal to direct; relay required.** RFC 4787 REQ-1. | Verified |
| **IPv6-only** | **Changes the answer favourably, but does not solve it.** Tailscale: "we can get rid of STUN, the birthday paradox trick, port mapping protocols, and all the hairpinning bumf" — but: "we currently don't have an all-IPv6 world... about 33% IPv6. Those 34% are very unevenly distributed, so a particular set of peers could be 100% IPv6, 0% IPv6, or anywhere in between." And: "Your office workstation may have a globally reachable IPv6 address, but I'll bet there's still a corporate firewall enforcing 'outbound connections only'." **For rdog specifically: an IPv6-native direct path has a 1500-byte MTU, so a large `@savefile` could ride in far fewer datagrams than the 1191-byte IPv6-minimum-MTU path.** | Verified (Tailscale blog) |
| **CGN / double NAT** | **Doubles the NAT count but not necessarily the failure rate**, *if* both layers are EIM. A symmetric CGN outer layer (some mobile carriers) breaks it. | Verified (RFC 4787 §4.4, §8) |
| **NAT rebinding mid-session** | **Real for long-lived rdog daemons.** RFC 4787 REQ-5/REQ-6 (mapping expiry, outbound refresh) + §8 (non-deterministic NATs). Requires ICE restart / re-punching. `webrtc-rs` ships an `ice-restart` example; Tailscale: "you also need to detect that your currently selected path has failed... and downgrade to another path." | Verified |
| **Cloud VPS itself is a single point of failure** | Yes. Headscale's docs warn about exactly this when removing Tailscale's DERP servers: "This is a single point of failure and could hamper connectivity." | Verified (headscale docs/ref/derp.md) |

---

## 6. D. Refutations — the strongest arguments AGAINST this direction

These are the arguments I think should carry the most weight.

### R1. "Just use Headscale/DERP" — the strongest single refutation

Headscale solves *precisely* this problem shape: coordination/handshake server + DERP relay fallback.

- Headscale: **44,816 stars**, Go, BSD-3-Clause, actively developed (last push 2026-10-20, 137 open issues). It is a self-hosted Tailscale control server.
- `docs/ref/derp.md`: "A DERP (Designated Encrypted Relay for Packets) server is mainly used to relay traffic between two nodes in case a direct connection can't be established. **Headscale provides an embedded DERP server** to ensure seamless connectivity between nodes."
- The embedded DERP is **disabled by default**; enable with `derp.server.enabled: true` + public `ipv4`/`ipv6`. "Besides relaying traffic, **it also uses STUN (udp/3478)** to help clients discover their public IP addresses and perform NAT traversal."
- DERP's data path is **HTTPS on tcp/443** + HTTP on tcp/80 for captive-portal checks + STUN on udp/3478. **It gets through anything that permits HTTPS.**
- The killer quote: "**There are no speed or throughput optimisations, the main purpose is to assist in node connectivity.**" — i.e. even the Headscale maintainers treat DERP as a *connectivity* backstop, not a data path.

**Factual comparison:**

| | Hand-rolled WebRTC lane | Embed/borrow DERP-style |
|---|---|---|
| NAT traversal | Full ICE (candidates, srflx, prflx, nomination, keepalives, restart) | Same problem, solved by an existing Go implementation |
| Relay | RFC 5766 TURN (coturn, C + OpenSSL 3.0) | DERP (HTTPS, in-process in headscale) |
| rdog-side deps | `webrtc` (or `str0m`) ≈ 18 subcrates + tokio + rustls + ring | **Tailscale client** (Go binary) or reimplement DERP client in Rust |
| Reconnect / churn | ICE restart, TURN permission refresh (300 s), allocation re-create (3600 s) | Long-lived HTTPS connection; much simpler lifecycle |
| Escapes UDP-blocking networks | Only via TURN-TCP (RFC 6062) | **Native** — DERP is HTTPS |
| Custom auth | rdog must mint TURN creds (TURN REST API: `timestamp:username` + `base64(hmac-sha1(secret,user))`) and negotiate DTLS fingerprints | WireGuard keys; Tailscale/Headscale auth model (or rdog's own app-level auth *on top*) |
| Ecosystem | None | Tailscale's massive ecosystem; `tailscale up`/`tailscale ssh` already solves "control a remote box" |
| **What you give up** | — | You inherit a VPN/network model; rdog's application-level auth (`specs/rdog-authentication-plan.md`) becomes a second layer; binary deps (Go); you are not building rdog-specific semantics |

**Can rdog embed DERP instead of WebRTC?** Yes in principle — the DERP *protocol* is simple (WebSocket/HTTP framing over tcp/443 with a small frame header) and a Rust client/server is a bounded amount of work. But note: **DERP does not by itself give you P2P.** DERP is the *relay*. Tailscale's P2P comes from **WireGuard + its own NAT-traversal layer (Netlink, disco, STUN, port-prediction, DERP fallback)**, not from DERP. So "embed DERP" = "embed the relay, still need ICE-equivalent for P2P."

### R2. "Just use a TCP/WS relay" — the 10×-simplicity refutation

rdog's actual requirements are: a reliable, ordered, bidirectional, long-lived byte stream, with lines up to 8 MiB, carrying PTY frames. **A TCP or WebSocket stream satisfies every one of them**, and rdog *already implements both* (`ControlTransportKind::Tcp`, `ControlTransportKind::WebSocket`).

A cloud-side byte-stream splice — daemons dial out over WebSocket (or TLS-TCP), the control client dials out over WebSocket, the VPS splices the two — delivers:
- ✅ reliable, ordered, bidirectional, 8 MiB+ lines (no message-size cap issue at all)
- ✅ PTY bidirectional streaming
- ✅ byte-for-byte line-protocol preservation (zero protocol change)
- ✅ escapes UDP-blocking networks (HTTPS)
- ✅ **zero new Rust dependencies**; the lane is the existing WebSocket transport
- ❌ no P2P — the cloud always carries the payload

**Cost of losing P2P, concretely for rdog:** the cloud becomes a bandwidth bottleneck precisely for the largest payloads. A `@screenshot` frame can be up to 8 MiB base64; over a 1 Mbps VPS uplink that is ~67 seconds. If the VPS has a modest monthly traffic allowance, a handful of screenshots exhausts it. **This is the single strongest argument FOR having P2P for rdog** — and simultaneously the strongest argument for **building the relay first, at a known cost, and adding P2P second**.

### R3. "The Zenoh router relay is already built and is good enough"

`specs/zenoh-control-plane-plan.md` already specifies the canonical topology: **daemon = router, control = client**, PTY frames over `session/<id>/to-daemon` and `session/<id>/to-control`, a daemon-embedded router (no external `zenohd`), autodiscovery with `--entry-point` fallback. Zenoh 1.8.0 is already a dependency with TCP/UDP/TLS/serial/unixpipe transports. The session-bridge payload format already carries a **line** (`__rdog_session__:{session_id}\n{line}`, `src/zenoh_control/session_payload.rs:64-65`).

So the "cloud = zenoh router" baseline is arguably **more than 90 % specified and partly implemented**, and it is zero new dependencies. Its only deficit is the same one as R2: the cloud carries all payload.

**Zenoh's decisive lack (verified):** Zenoh has **no STUN, no TURN, no ICE, no hole punching**. Searching the Zenoh repo for NAT traversal returns only:
- PR #2776 `advertise_addr=observed` — forwarding the *observed* source IP through scouting Hello messages, for **PIM/multicast-routed NAT** networks. Explicitly: "Without a special means to handle this case, there is no possible way to leverage zenoh discovery in a peer-to-peer fashion."
- PR #2039 "Locator address overriding" — a manual `loc_addr=` override.
- Issue #78 "Routing behind NAT with ROS" (2021) — an old, closed request.

**Conclusion: Zenoh cannot natively do WebRTC-style NAT traversal.** The sibling research task's question ("can Zenoh 1.8 do this natively?") should be answered **no, for the traversal part** — with the caveat that the crossover in §9 is real.

### R4. "The cloud is a de-facto full relay anyway — 'handshake only' is a myth"

- Tailscale: "you have your relay network to pick up the slack when traversal fails, **as it likely will for a long tail of cases**."
- Headscale ships the DERP relay *by default in the DERP map* alongside STUN, and the Tailscale control plane offers "free-to-use DERP servers" — the relay is not optional in the product.
- **So the honest model of the cloud node is: a signaling server that is *also* a relay, where the relay is the required baseline and the signaling only enables skipping it.** Any plan that calls the cloud "handshake only" and does not budget for relay bandwidth is wrong.

### R5. "The extra dependency is unacceptable for a lean Rust CLI"

rdog today ships a lean binary (the `webrtc` crate itself is 394 KB unpacked, but the *transitive* footprint is `rtc` + 18 subcrates + rustls + ring/aws-lc-rs + quinn-udp + tokio). rdog's own `src/` uses no async runtime today, so `webrtc` forces a **tokio runtime into a thread-based daemon**. `str0m` (Sans-I/O) avoids that and deserves a bake-off. **Caveat to be fair to the WebRTC option: tokio is already in the lockfile via Zenoh** — so the marginal cost is not "add a runtime" but "make the daemon's threading model coexist with one."

### R6. "SCTP's 256 KiB / 16 KB message ceiling vs rdog's 8 MiB line is a design smell, not a detail"

`rtc-sctp` does not implement RFC 8260 interleaving, so per RFC 8831 §6.6 the sender "SHOULD limit the maximum message size to 16 KB." rdog's `@savefile` line can be 32× the SCTP default and 512× the 16 KB SHOULD. A WebRTC lane therefore **cannot be a transparent byte-stream substitute without adding its own chunking layer** — and once you add that layer, you have re-implemented what a TCP stream gives you for free. **This is a strong argument that WebRTC's only real justification is the P2P path, and nothing else.**

### R7. Latency and long-lived-session churn

- **Interactive PTY latency:** a relay adds the VPS to the round trip (client→VPS→daemon→VPS→client). For a `codex` TUI over PTY, keystroke echo latency is user-visible. Direct P2P removes one RTT. *But the same is true of DERP or any relay — this does not differentiate WebRTC from other relays.*
- **ICE churn in long-lived daemons:** NAT rebinding (RFC 4787 REQ-5/6) forces re-punching; non-deterministic NATs (RFC 4787 §8) can break a working path without any configuration change. Tailscale's answer is to keep "warm" fallbacks and probe continuously. rdog's daemons are long-running by design, so this is a permanent operational tax, not a startup cost.
- **TURN's timer obligations** (see B4): 300 s permission refresh + 3600 s allocation re-creation, on a long-lived session. A DERP-style relay has one HTTPS connection and no timers. **Concrete, verifiable burden that the relay-only design does not have.**

### R8. "TURN relay bandwidth cost on a small VPS"

- coturn's own performance wiki: the binding constraint is **network throughput and the kernel's UDP/TCP stack**, not allocations ("an allocation does not have its own thread or process... shares lots of system resources"; "thousands or tens of thousands of usual traffic streams" per system). Scaling is linear in bandwidth.
- For rdog the payload is *screenshots* (multi-MB) and *PTY output*, not voice. A 1–3 Mbps VPS uplink is a hard ceiling: one 8 MiB `@savefile` ≈ 20–60 s of exclusive uplink. **The bandwidth argument is not about TURN specifically; it is about "the cloud carries payload", and it applies identically to every relay option.** What it does argue is that **P2P has genuine value for rdog's screenshot-heavy workload** — and, symmetrically, that the relay must be sized for the fallback traffic.

### R9. "A data channel over WebRTC is unnecessary — the requirement is a byte stream"

rdog needs: reliable ✔, ordered ✔, bidirectional ✔, long-lived ✔, backpressure ✔ (TCP), 8 MiB lines ✔ (TCP; SCTP ❌ without chunking), PTY streaming ✔ (TCP). **Nothing in rdog's protocol semantics requires a message-oriented, congestion-controlled, encrypted, NAT-traversing transport.** The only requirement that *could* justify WebRTC is "skip the cloud when possible". If that requirement is dropped, WebRTC is pure cost. **The spec must state explicitly whether P2P is a hard requirement or an optimization — the entire decision hinges on it.**

### R10. "Unverified/risky assumptions to challenge before committing"

- The TURN REST API is a **de-facto standard**, not an RFC (the `draft-uberti-behave-turn-rest-api` I-D expired). coturn's `--use-auth-secret` is interoperable with browsers, but rdog would be the one minting creds — that's fine, but don't cite "RFC" for it.
- coturn requires **OpenSSL 3.0+** — a C dependency on the VPS, and the TURN server is a separate process to operate and monitor.
- `webrtc` 0.21.x is a **recent breaking rewrite**; 0.17.2 (still referenced by `rtc`'s dev-deps) had OpenSSL features. Pin and budget for churn.
- **RFC 4787 §5 vs. real-world ICE** disagree on the ADPF case (see C7). Do not design to the optimistic reading.
- RFC 5780's own warnings: NAT behavior "under load" can degrade, and results are "instantaneous" only.

---

## 7. E. Architecture candidates

### (i) Zenoh-router-as-relay baseline *(the cloud is a Zenoh router; no P2P)*

- **Signaling:** none needed — Zenoh's own scouting/Hello + session channels are the coordination mechanism. `rdog/{ns}/session/{id}/to-daemon|to-control` (`src/zenoh_identity.rs:231-264`).
- **Data path:** control client → (TCP/TLS) → cloud Zenoh router → (TCP/TLS) → daemon. **Cloud carries 100 % of payload.**
- **Cloud runs:** `rdog daemon` in router mode (or `zenohd`), with TCP/TLS listen endpoints.
- **Each daemon runs:** Zenoh client mode + the rdog session bridge.
- **Control client does:** `rdog control <target>` → zenoh client → resolve target → session bridge.
- **Failure/fallback:** zenoh link retry (the repo already has `docs/solutions/best-practices/zenoh-hello-locator-priority.md` on locator ordering) + TCP reconnect. **No P2P fallback, and no ICE churn.**
- **"Handshake only" claim:** **False.** The cloud is a full relay. This is a *deliberate* trade: simplest possible thing that works, and the P2P optimization can be layered on later.
- **Evidence:** repo-native (`specs/zenoh-control-plane-plan.md`, `Cargo.toml` zenoh 1.8.0 features). **Evidence strength: High for feasibility, Zero for P2P.**

### (ii) WebRTC data channel + coturn STUN/TURN + Zenoh-or-HTTPS signaling

- **Signaling:** SDP offer/answer + ICE candidates. Carried over the **existing Zenoh session channel** (pub/sub on `rdog/{ns}/signal/...`) or over the existing inbound WebSocket transport. *No new transport needed for signaling* — this is a large simplification.
- **Data path:** control client ↔ daemon directly over SCTP/DTLS/UDP when ICE nominates a host/srflx pair; otherwise TURN allocation on the cloud, client → TURN(cloud) → daemon.
- **Cloud runs:** `coturn` (STUN+TURN, 3478/5349) **and** a signaling endpoint. Possibly also the Zenoh router (so the cloud is a single service).
- **Each daemon runs:** `webrtc`/`str0m` PeerConnection in a tokio (or Sans-I/O) driver; a `ControlTransportKind::WebRtc` arm; a transport-level chunking layer to bridge SCTP message limits to rdog's 8 MiB line limit.
- **Control client does:** same, plus ICE.
- **Failure/fallback:** ICE → TURN; ICE restart on rebinding; TURN permission refresh at <300 s; allocation re-create at 3600 s.
- **"Handshake only" claim:** **True only in the happy path.** In the fallback case the cloud carries everything, at WebRTC's efficiency (one extra DTLS+SCTP encapsulation, and the 1191-byte MTU that the TURN-relayed IPv6 budget forces).
- **Evidence:** High for crate/coturn feasibility (verified above); **Medium** for real-world P2P success rate (extrapolated from RFC 4787 + Tailscale, not measured for rdog's specific networks); **Low** for rdog-specific message-size integration (requires a new chunking layer + a spec).

### (iii) Headscale/DERP-style embedded

- **Signaling:** Headscale's control plane (node key exchange, endpoint advertisement) over gRPC/HTTPS.
- **Data path:** WireGuard over a direct UDP path (Tailscale's own traversal) or over DERP (HTTPS on tcp/443) when direct fails.
- **Cloud runs:** Headscale (Go) + embedded DERP (HTTPS + STUN udp/3478). Verified: "Headscale provides an embedded DERP server"; STUN on 3478; DERP relay on HTTPS/443.
- **Each daemon runs:** the Tailscale client — a **Go binary** or a Rust reimplementation of the DERP client.
- **Control client does:** `tailscale`-style CLI or rdog-specific.
- **Failure/fallback:** DERP is always available as a warm fallback; direct is upgraded in opportunistically; Tailscale "upgrade connections on the fly as we discover better paths".
- **"Handshake only" claim:** **False — and that's the point.** DERP *is* the relay; the cloud is unambiguously a full relay.
- **Evidence:** High (headscale docs, 44k stars, actively developed).
- **rdog fit:** **Poor-to-medium.** It solves "reach a box across NATs" better than anything else here, but it replaces rdog's application-level control plane with a VPN. rdog's `@cmd`/`@key`/`@screenshot`/`@pty` semantics would have to be re-expressed over a Tailscale tunnel, and the repo's TLS/auth plans (`specs/rdog-authentication-plan.md`, `specs/rdog-tls-plan.md`) would sit on top of WireGuard's own crypto.

### (iv) Bespoke "rendezvous + byte-stream relay" (the 10×-simpler P2P attempt)

- **Signaling:** a tiny custom rendezvous on the cloud: daemons and the client register and exchange **reflexive addresses learned from a minimal STUN-like binding**; the client then attempts a simultaneous UDP hole punch; the cloud also exposes a plain byte-stream relay (WebSocket) that is always available.
- **Data path:** direct UDP if the punch succeeds; otherwise the cloud's WebSocket relay. **The client and daemon both always hold a relay path open as the fallback.**
- **Cloud runs:** a small rendezvous service + a WebSocket byte-stream splicer. *Could be the same process as a Zenoh router, or a ~200-line service.* No coturn, no OpenSSL.
- **Each daemon runs:** a STUN-like binder + a UDP socket + the existing WebSocket transport. **No new Rust dependencies** (the repo already has WebSocket + UDP-scouting-adjacent plumbing via Zenoh).
- **Control client does:** the same.
- **Failure/fallback:** the relay is *always* there; direct is a bonus. No ICE, no nomination, no restart machinery — but also **no peer-reflexive candidates and no symmetric-NAT trick**, so direct success is strictly worse than ICE's.
- **"Handshake only" claim:** **True by construction** — the relay path exists and is used whenever the punch fails, and the code does not pretend otherwise.
- **Evidence:** Medium. The components are individually verified (RFC 5780 describes exactly the STUN tests needed; RFC 4787 §4.1 REQ-1 says EIM is the requirement), but the *combination* is bespoke and would need its own adversarial testing against symmetric NAT. **Risk: reimplementing a known-hard protocol badly.** The honest assessment: this is what ICE exists to prevent, and the effort saved on dependencies is partly paid back in protocol risk.

---

## 8. Option table

| Option | Cloud runs | Signaling | Data path | Direct P2P realistic? | Complexity | Evidence strength |
|---|---|---|---|---|---|---|
| **(i) Zenoh router as relay** | `rdog daemon` router mode (or `zenohd`) | Zenoh scouting + session channels | client →(TCP/TLS)→ cloud →(TCP/TLS)→ daemon | **No** (never) | **Low** — mostly already specified in-repo | **High** for feasibility (repo-native); **Zero** for P2P |
| **(ii) WebRTC DC + coturn** | coturn (3478/5349) + signaling (Zenoh pub/sub or existing WebSocket) | SDP + ICE candidates over Zenoh session channel | direct SCTP/DTLS/UDP **or** TURN relay | **Yes in the common home-broadband case**; relay otherwise | **High** — new dep, new chunking layer, ICE lifecycle, TURN timers, DTLS/fingerprint handling | **High** for crate/coturn facts; **Medium** for real-world P2P rate; **Low** for rdog message-size integration |
| **(iii) Headscale/DERP embedded** | Headscale (Go) + embedded DERP (HTTPS/443 + STUN/3478) | Headscale control plane (gRPC/HTTPS) | WireGuard direct **or** DERP HTTPS relay | **Yes** (Tailscale's own traversal) — but rdog must ride a VPN | **Very high** for rdog — new model, Go binary, second auth layer | **High** (44k-star project, official docs) |
| **(iv) Bespoke rendezvous + WS relay** | tiny rendezvous + WebSocket byte-splicer | custom: reflexive-address exchange | direct UDP punch **or** WS relay | **Partially** — no prflx, no symmetric-NAT trick | **Medium** — no new deps, but bespoke protocol risk | **Medium** |
| **(ii-a) str0m instead of webrtc** | same as (ii) | same | same | same | **Medium** — Sans-I/O fits rdog's thread model, avoids tokio-in-daemon | **Medium** (2.6M downloads, 38 versions, but less battle-tested than webrtc) |

---

## 9. Zenoh-vs-WebRTC crossover

**Yes — and the crossover is already half-built in this repo.**

**Finding: Zenoh can carry the signaling plane, and the data path can be a separate P2P lane, so the cloud can stay a Zenoh router.**

Evidence:
1. **The session-channel keyexpr layout already provides a pub/sub signaling substrate**: `rdog/{ns}/session/{id}/to-daemon` and `.../to-control` (`src/zenoh_identity.rs:231-264`). SDP offers/answers and ICE candidates are small text payloads — exactly the shape the existing session bridge already uses (`__rdog_session__:{session_id}\n{line}`, `src/zenoh_control/session_payload.rs:64-65`).
2. **The Zenoh router is already the intended cloud-side topology** (`specs/zenoh-control-plane-plan.md`: "daemon = router, control = client", daemon-embedded router, no external `zenohd`). A cloud-hosted Zenoh router would need only TCP/TLS listen endpoints reachable by all three NAT domains — and **outbound-only NAT is exactly what Zenoh's TCP/TLS client mode handles** (a daemon dials out; it needs no inbound path).
3. **Zenoh 1.8.0 already ships `transport_tcp` and `transport_tls`** (`Cargo.toml`), and the repo's `specs/rdog-tls-plan.md` plans mTLS on top. So the signaling plane over Zenoh is genuinely available.
4. **Zenoh provides no traversal of its own** (verified: only `advertise_addr=observed` / `loc_addr=` locator overrides exist, both for multicast-routed NAT, not consumer NAT) — so the P2P lane must be a *separate* mechanism (ICE/WebRTC, or a bespoke punch).
5. **The data path need not be Zenoh at all.** A `ControlTransportKind::WebRtc` (or `::UdpP2p`) variant is a peer-to-peer lane; the line protocol rides it unchanged. `specs/control-line-protocol.md:960-963` already fixes the seam: "统一复用 `send_control_lines_*` 管线(TCP / WebSocket / Zenoh 三套入口都对应同名函数)". **A fourth lane is a fourth arm of the same functions.**

**Crossover architecture (the synthesis that actually fits the repo):**
```
control client (NAT-C)  ──zenoh TCP/TLS──▶  cloud Zenoh router  ◀──zenoh TCP/TLS── daemon (NAT-B)
        │                                          │  (signaling only: SDP + ICE candidates)
        └────────── direct SCTP/DTLS/UDP (P2P) ───┘
                   fallback: TURN via coturn on the cloud
```
**Caveats:**
- Zenoh pub/sub is **at-most-once** by default and the repo already has a documented discipline about it (`docs/solutions/design-patterns/mailbox-delivery-loss-precedence.md`). **ICE candidate exchange must be reliable** — an ICE candidate that is dropped simply delays negotiation (it will be re-sent by trickle ICE), so at-most-once is *tolerable* for trickle ICE but **not** for the initial SDP offer/answer.
- Zenoh adds a **second async runtime** interaction to the daemon (Zenoh is already tokio-based, so this is fine) and a **second failure domain** to debug.
- The coturn TURN server is still a **separate process** from the Zenoh router, with its own OpenSSL dependency, its own credentials, and its own config surface. "One VPS" does not mean "one service".

**Verdict on the crossover: the signaling-over-Zenoh idea is sound, well-supported by the repo, and is the recommended way to avoid inventing a signaling protocol. It does not remove the need for a TURN-style relay, and it does not by itself provide P2P.**

---

## 10. Evidence table

| # | Claim | Source | Status |
|---|---|---|---|
| 1 | rdog's transport abstraction has exactly 2 kinds (Tcp, WebSocket) and is message-per-line | `src/control_transport.rs:42-47`, `:107-119` | **Verified** (file read) |
| 2 | Single-line ceiling is 8 MiB, single source of truth, sized for 4K screenshot base64 | `src/control_line_read.rs:26-30` | **Verified** (file read) |
| 3 | WebSocket payload is capped at the same 8 MiB | `src/control_transport.rs:652` | **Verified** (file read) |
| 4 | rdog already has app-level chunking (`@file-chunk`/`@file-read`), chunk ≤ ~6.4 MiB | `src/control_file_transfer.rs:8-13`, `:42-44` | **Verified** (file read) |
| 5 | A shared `send_control_lines_*` pipeline already serves TCP/WebSocket/Zenoh — the lane seam | `specs/control-line-protocol.md:960-963` | **Verified** (file read) |
| 6 | PTY frames are base64 text lines; `@pty`/`@pty-close`/`@pty-detach`/`@pty-attach` | `specs/control-line-protocol.md:621-668`; `specs/pty-control-plan.md` | **Verified** (file read) |
| 7 | `@screenshot` = multiple `@savefile` frames + final `@response` | `specs/control-line-protocol.md:721-722` | **Verified** (file read) |
| 8 | Zenoh session keyexprs `rdog/{ns}/session/{id}/to-daemon|to-control` | `src/zenoh_identity.rs:231-264` | **Verified** (file read) |
| 9 | Zenoh session bridge payload is a line (`__rdog_session__:{id}\n{line}`) | `src/zenoh_control/session_payload.rs:64-65` | **Verified** (file read) |
| 10 | zenoh 1.8.0 features: serial/tcp/udp/tls/unixpipe + auth_usrpwd; **no** transport_websocket | `Cargo.toml` | **Verified** (file read) |
| 11 | Tokio 1.51.0 already in the tree (19 dependents); rdog's own `src/` uses no tokio | `Cargo.lock`; `grep -rn tokio src/` | **Verified** (file read) |
| 12 | No prior WebRTC/ICE/TURN content in the repo | `grep -rniE "webrtc\|data channel\|ice\b\|turn server"` over src/specs/docs | **Verified** (file read) |
| 13 | `webrtc` 0.21.1, 2026-10-11, MIT/Apache-2.0, 8.23M downloads, 72 versions, default `runtime-tokio`+`crypto-ring` | https://crates.io/api/v1/crates/webrtc | **Verified** |
| 14 | webrtc-rs/webrtc: 5161★, 524 forks, 24 open issues, pushed 2026-10-11 | https://api.github.com/repos/webrtc-rs/webrtc | **Verified** |
| 15 | Pure Rust — `rtc` uses rustls 0.23 + rtc-crypto; no OpenSSL in 0.20+/0.21 | https://docs.rs/rtc/0.21.1/rtc/ (deps); crates.io feature lists | **Verified** |
| 16 | 0.17.2 had `openssl`/`vendored-openssl` features; OpenSSL-free posture is a 0.20+ property | crates.io API version feature lists | **Verified** |
| 17 | `rtc-sctp` `DEFAULT_MAX_MESSAGE_SIZE = 262144`; `INITIAL_MTU = 1191`; `RECEIVE_MTU = 8192`; `INITIAL_RECV_BUF_SIZE = 1 MiB` | https://raw.githubusercontent.com/webrtc-rs/rtc/main/rtc-sctp/src/config.rs | **Verified** (source) |
| 18 | 1191 = TURN-relayed IPv6 budget (1280−40−8−4−37); from pion/sctp#476, webrtc#807 | same source file, comment | **Verified** (source) |
| 19 | `rtc-sctp` lists only RFC 3758 + RFC 6525 extensions — **no RFC 8260 interleaving** | https://docs.rs/rtc-sctp/0.21.1/rtc_sctp/ (crate doc) | **Verified** (absence in doc) |
| 20 | "SHOULD limit the maximum message size to 16 KB" without interleaving; size limit must be negotiated (RFC 8841) | RFC 8831 §6.6 — https://www.rfc-editor.org/rfc/rfc8831.txt | **Verified** |
| 21 | Req. 8: data channels "should support unbounded-length 'messages'... implementations might enforce a reasonable message size limit" | RFC 8831 §4 | **Verified** |
| 22 | `RTCDataChannelInit { ordered, max_retransmits }`; `webrtc` data_channel trait has `send`/`try_send`/send-buffer back-pressure | https://docs.rs/rtc/0.21.1/rtc/data_channel/ ; https://docs.rs/webrtc/0.21.1/webrtc/ | **Verified** |
| 23 | Upstream examples exist: `data-channels-flow-control{,-multi}`, `trickle-ice-srflx`, `trickle-ice-relay`, `ice-restart`, `ice-tcp`, `data-channels-stress-bench` | https://api.github.com/repos/webrtc-rs/webrtc/contents/examples | **Verified** |
| 24 | coturn: one process, STUN+TURN, UDP+TCP 3478, TLS 5349; `--stun-only`/`--no-stun` to split | https://raw.githubusercontent.com/coturn/coturn/master/README.turnserver | **Verified** |
| 25 | TURN REST API: `--use-auth-secret` + `--static-auth-secret`; username `timestamp:username`; password `base64(hmac-sha1(secret,user))`; `--rest-api-separator` default `:`; `--realm` required | same README | **Verified** |
| 26 | TURN REST API is a de-facto standard (draft-uberti-behave-turn-rest-api), never an RFC | same README ("formally described in Justin's Uberti TURN REST API document") | **Verified** (as an expired I-D) |
| 27 | `--max-allocate-lifetime` 3600 s; `--permission-lifetime` 300 s ("MUST not be changed for production"); `--channel-lifetime` 600 s | same README | **Verified** |
| 28 | RFC 6062 TCP relay **enabled by default**; `--no-tcp-relay` / `--no-udp-relay` disable | same README (flag descriptions) | **Verified** |
| 29 | coturn requires OpenSSL 3.0+; `--rfc5780` needs ≥2 same-family IPs; `--mobility`=MICE; `--min-port/--max-port` 49152–65535 | same README | **Verified** |
| 30 | coturn scaling is network-throughput-bound, not allocation-bound; "thousands or tens of thousands" of streams | https://raw.githubusercontent.com/wiki/coturn/coturn/TURN-Performance-and-Load-Balance.md | **Verified** |
| 31 | **No Rust TURN *server* crate** on crates.io; `rtc-turn` is a client; `pion/turn` is Go | https://crates.io/api/v1/crates?q=stun%20turn%20ice (66 matches scanned) | **Verified (negative)** |
| 32 | `str0m` 0.24.1, 2.61M downloads, Sans-I/O WebRTC; `is` 0.11.1 Sans-I/O ICE | same crates.io search | **Verified** |
| 33 | **eturnal** TURN server maturity / TURN-REST-API parity | — | **UNVERIFIED** — not fetched in this pass |
| 34 | NAT MUST have Endpoint-Independent Mapping; failure "will force the use of a UDP relay" | RFC 4787 §4.1 REQ-1 — https://www.rfc-editor.org/rfc/rfc4787.txt | **Verified** |
| 35 | With ICE: ADPF filtering → relay required; EIF or ADF → ICE finds connectivity without a relay | RFC 4787 §5 | **Verified** |
| 36 | UDP relays are "generally seen as a solution of last resort" | RFC 4787 §2 | **Verified** |
| 37 | RFC 3489 cone/symmetric terminology "has been the source of much confusion"; use RFC 4787 behaviors | RFC 4787 §3 | **Verified** |
| 38 | NAT mapping timer MUST NOT expire < 2 min (5 min+ recommended); outbound refresh MUST be True | RFC 4787 REQ-5, REQ-6 | **Verified** |
| 39 | Non-deterministic NATs may switch EIM→ADPM on port conflict; REQ-11 determinism | RFC 4787 §8 | **Verified** |
| 40 | "establishing connectivity between nodes becomes significantly more difficult if a NAT has any behavior other than Endpoint-Independent Mapping" | RFC 5780 §4.5 — https://www.rfc-editor.org/rfc/rfc5780.txt | **Verified** |
| 41 | NAT behavior "under load" can become more restrictive; results are instantaneous only | RFC 5780 §1, §2 | **Verified** |
| 42 | Some platforms discard fragments under load; PADDING attribute probes it | RFC 5780 §3.5, §11 | **Verified** |
| 43 | RFC 5780 binding-lifetime discovery; keepalives required | RFC 5780 §3.3, §4.6 | **Verified** |
| 44 | RFC 4787 §5's ADPF claim is stricter than real-world ICE (simultaneous opening connects two port-restricted-cone NATs) | RFC 4787 §5 vs. Tailscale's documented behavior (§45) | **Verified as a discrepancy** — flagged, not resolved |
| 45 | Tailscale: "all connections start out with DERP preselected… transparently upgrades"; "relay network to pick up the slack when traversal fails, as it likely will for a long tail of cases"; needs "warm" fallbacks; "quite fiddled to get right" | https://tailscale.com/blog/how-nat-traversal-works/ | **Verified** |
| 46 | Tailscale: "fallback relays that use a well-like protocol like HTTP, to get out of networks that block outbound UDP" | same | **Verified** |
| 47 | Tailscale on IPv6: can drop STUN/port-mapping tricks, but ~33% IPv6, unevenly distributed, and stateful firewalls remain | same | **Verified** |
| 48 | Tailscale: ICE = "try everything at once, and pick the best thing that works" | same | **Verified** |
| 49 | Headscale: 44,816★, Go, BSD-3-Clause, pushed 2026-10-20, 137 open issues | https://api.github.com/repos/juanfont/headscale | **Verified** |
| 50 | Headscale ships an **embedded DERP server**, disabled by default; DERP relays via HTTPS/443 + HTTP/80 + STUN/3478; "no speed or throughput optimisations" | https://raw.githubusercontent.com/juanfont/headscale/main/docs/ref/derp.md | **Verified** |
| 51 | Headscale warns a single DERP is a single point of failure | same | **Verified** |
| 52 | Zenoh has **no STUN/TURN/ICE/hole punching**; only `advertise_addr=observed` (PR #2776) and `loc_addr=` override (PR #2039) for multicast-routed NAT | https://api.github.com/search/issues?q=repo:eclipse-zenoh/zenoh+NAT+traversal… | **Verified** |
| 53 | Zenoh issue #78 "Routing behind NAT with ROS" (2021) — closed, unsatisfied | same search | **Verified** |
| 54 | `DEFAULT_SCREENSHOT_QUALITY = 75`; JPEG→base64 single line | `src/control_protocol.rs:143`; `src/screenshot.rs:335-345` | **Verified** (file read) |
| 55 | Real-world P2P success rate for rdog's specific networks (home broadband, mobile, corporate) | — | **UNVERIFIED / UNMEASURED** — no rdog-specific or third-party measurement found; matrix is derived from RFCs, not observed |
| 56 | Actual VPS uplink bandwidth / traffic allowance in the user's deployment | — | **UNVERIFIED** — user-specific; the 8 MiB @savefile figure makes this decision-critical |

---

## 11. Open questions a spec must decide (each crisp, with consequences)

1. **Is P2P a hard requirement or an optimization?** If *optimization* → build the relay only (Option i/v), zero new dependencies, ship in weeks. If *hard requirement* → Option (ii)/(ii-a) with a bundled relay, weeks-to-months and a new dependency + a new chunking layer.
2. **Is the cloud node's payload budget acceptable?** One `@savefile` can be ~8 MiB. At 1 Mbps that is ~67 s of exclusive uplink. Either (a) accept and size the VPS, (b) require P2P for screenshots specifically, (c) cap/route screenshots differently. *Cannot be left implicit.*
3. **`webrtc` vs `str0m`?** `str0m` is Sans-I/O and fits rdog's thread-based daemon without adding a runtime to `src/`; `webrtc` is more battle-tested (8.2M vs 2.6M downloads) but is tokio-based and API-volatile (0.20→0.21 breaking rewrite). *Decide before any implementation.*
4. **Does the DataChannel carry bytes or lines?** Bytes (with transport-level length-prefixed chunking) preserves the line protocol byte-for-byte and reuses `read_line_limited`; lines forces the SCTP message-size problem onto the protocol. **Recommend bytes.**
5. **Is the SCTP association shared between the control/PTY channel and the bulk channel?** If yes, an 8 MiB screenshot stalls PTY output (no RFC 8260 interleaving). If no, two DataChannels with separate streams — but a shared congestion window and path remain. *Decide; it 
</task_output>