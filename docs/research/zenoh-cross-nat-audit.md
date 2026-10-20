> **本文件是 wayfinder research 产出,仅存在于 `research/zenoh-cross-nat-audit` 分支,不会被合并。**
> 关联 ticket: #107(审计:zenoh 1.8 跨 NAT 能力边界) · 所属 map: #106
>
> **中文摘要**
>
> - **结论:zenoh 1.8 没有"握手服务器"这种角色。** `WhatAmI` 只有 Router / Peer / Client 三个值(`zenoh-protocol-1.8.0/src/core/whatami.rs:40-45`),不存在 broker / access 第四角色;整个 crate 的公开 API 根只有 `Config` / `Session` / `open` / `scout` / `init_log_from_env_or` / `try_init_log_from_env`。所谓 "Access API" 在 1.8.0 中**查无实据**,应视作伪前提。
> - **零 NAT 穿越。** 全仓搜 `hole punching|stun|nat traversal|upnp|port forward|external address|public ip` **零命中**;`zenoh-config` 也没有 `transport/link/udp/*` 配置面(UDP 只是 transport,不是穿越机制)。
> - **最接近"握手服务器"的形态是 `mode=peer` 云节点。** `gossip.rs` 的 `GossipNet` 会带 `locators: true` 转发对端 locator;而 `mode=router` 用的 `network.rs` 里**没有任何一处** `locators: true` —— router 只转发拓扑并**剥掉** locator,远端 peer 连尝试直连都不会发生。
> - **但直连在 NAT 后依然会失败。** gossip 广播的是节点自己的 listen locator(私网 `192.168.x.x`)。`#loc_addr=`(PR #2039)合入时间 2026-03-16,比 1.8.0 发布(2026-03-13)晚 3 天,**1.8.0 没有**;`#advertise_addr=observed`(PR #2776)仍 open 且明确"不影响 gossip"。→ **rdog 必须自己在应用层交换公网可达端点。**
> - **运行时无法给活着的 session 加 connect endpoint。** `Notifier::insert_json5` 对非 `plugins/` 前缀直接 bail(`src/api/config.rs:271-282`),且 `connect/endpoints` 全 crate 只在 session 启动时读一次 → 运行时发现的对端只能**另开一个 `zenoh::open()` session**。
> - **ACL 是唯一原生的"只握手"手段**:`access_control` 按 key-expression 限制 router 转发范围。但粒度是 key-expression 而非 per-session,于是"直连失败后回落到同一 router 中继 payload"会很别扭(需要独立 router 实例或独立 namespace + 规则集)。
> - **纯中继路线的真实成本**(由配置默认值推导,非实测):每链路每优先级约 128 KB 在途(`tx/queue/size` 2×65535);背压下 `wait_before_drop=1000µs` → **静默丢包**(zenoh pub/sub 无应用层重传);rdog 的 `Cargo.toml` 用 `default-features = false` → **无 payload 压缩、无 SHM**;云端 egress 计费 + 每消息多一个 WAN RTT。

# Zenoh 1.8.0 as a Public-Cloud Rendezvous Broker for rdog — Capability Audit

**Sources read in full (ground truth):** vendored `zenoh-1.8.0`, `zenoh-config-1.8.0`, `zenoh-protocol-1.8.0`, `zenoh-link-1.8.0`, `zenoh-link-udp-1.8.0`, `zenoh-transport-1.8.0` under `~/.cargo/registry/src/rsproxy.cn-e3de039b2554c837/`; official `DEFAULT_CONFIG.json5` at tag `1.8.0`; published public API index `docs.rs/zenoh/1.8.0/zenoh/all.html`; GitHub issue/PR search on `eclipse-zenoh/zenoh`. No repo file was modified.

Path shorthand used below: `Z=…/zenoh-1.8.0`, `ZC=…/zenoh-config-1.8.0`, `ZP=…/zenoh-protocol-1.8.0`.

---

## 1. Verdict

**Zenoh 1.8 cannot natively do broker-only rendezvous plus a true peer-to-peer data path across two NATs — the combination you want does not exist as a zenoh feature, and the closest pieces each fail on a different axis.** There is no "Access API", no broker/relay-only mode, and no fourth node type: `WhatAmI` has exactly three variants — `Router`, `Peer`, `Client` (`ZP/src/core/whatami.rs:40-45`), and the entire published public API of `zenoh 1.8.0` is `Config`, `Session`, `open`, `scout`, `init_log_from_env_or`, `try_init_log_from_env` plus modules (`docs.rs/zenoh/1.8.0/zenoh/all.html`). Zenoh also has **zero** NAT-traversal machinery — an exhaustive search of `Z/src` for `hole punching|stun|NAT traversal|upnp|port forward|external address|public ip` returns **no matches**, and there are no `transport/link/udp/*` config keys at all (the string `udp` does not occur in `ZC/src`). The nearest native approximation is a **two-tier design**: use zenoh pub/sub (or any small metadata channel) over the cloud node to exchange *rdog-level* endpoints, then open a **second, dedicated zenoh session** whose `connect/endpoints` carry the discovered peer endpoint — because zenoh 1.8 offers **no public API to add connect endpoints to a live session**: `Notifier::insert_json5` hard-bails for any key not starting with `plugins/` (`Z/src/api/config.rs:271-282`), and nothing in the runtime re-reads `connect/endpoints` after startup (the sole config-notification subscriber in the whole crate is the plugin manager, `Z/src/net/runtime/adminspace.rs:240`). A single `mode=router` cloud node can be made *policy-wise* control-plane-only via `access_control` ACLs, but it still physically carries payload and adds a WAN RTT to every byte.

---

## 2. Option table

| Option | Mechanism | Cloud carries payload? | True P2P possible? | NAT / port-forward requirement | Exact config keys / Rust API | Confidence |
|---|---|---|---|---|---|---|
| **A. Multicast scouting** | UDP `224.0.0.224:7446` Scout/Hello exchange; Hello carries the responder's own listen locators | No (no relay at all) | Yes, **if** the two daemons are on the same L2/L3 multicast domain | **Yes** — consumer NAT + host firewall blocks multicast; `ttl: 1` keeps it L2-local | `scouting/multicast/{enabled,address,interface,ttl,autoconnect,autoconnect_strategy,listen}`; `zenoh::scout(WhatAmI, Config)` | **Verified** (source + official config) |
| **B. Gossip through a `mode=router` cloud node** | LinkState OAM messages over existing transport links; router relays peer linkstate via `Network` | Relays linkstate **topology only** — `Network` never sets `locators: true` (see §3) | **No** — locators are stripped at the router, so no direct-link attempt is even made | n/a | `scouting/gossip/{enabled,multihop,target,autoconnect,autoconnect_strategy}`; `routing/peer/mode` | **Verified** (source-level, exhaustive `Details {` grep) |
| **C. Gossip through a `mode=peer` cloud node** | `GossipNet` re-broadcasts the *immediate source's* state **including locators** (`Details{zid:true, locators:true, links:false}`) to all its other links | Relays **locators only** (small); no data payload | **Mechanically yes** — the learning peer then calls `open_transport_unicast()` on the advertised locator. **But** the advertised locator is the daemon's own *listen* address (private IP behind NAT) → the direct link attempt **fails** unless the daemon has a public IP or a port-forward | **Yes** — each daemon needs a publicly reachable `ip:port` (or the cloud node must proxy it) | `mode:"peer"` + `scouting/gossip/enabled:true` + `scouting/gossip/multihop:false` (default) + `scouting/gossip/autoconnect:["router","peer"]` | **Verified** for the mechanism; **inferred** that the direct attempt fails behind NAT (private locator) — needs a live test |
| **D. `routing/peer/mode:"linkstate"`** | Peers compute shortest paths over the linkstate graph and forward over *existing* links | Yes, if it is the shortest path | **No** — linkstate routes over links that already exist; it never *creates* a link. Locator discovery is still gossip's job | Same as C | `routing/peer/mode:"linkstate"`, `routing/router/linkstate/transport_weights`, `routing/peer/linkstate/transport_weights` | **Verified** |
| **E. Router as relay, ACL-restricted to rendezvous keys** | Cloud runs `mode=router`; ACL denies payload key-expressions, allows only `rdog/rendezvous/**` | **Physically yes** (it forwards whatever ACL permits); policy intends control-plane only | No — all data flows via the cloud | None for the rendezvous channel | `access_control/{enabled,default_permission,rules,subjects,policies}`; `transport/link/protocols`; `low_pass_filter` | **Verified** (schema + interceptor wiring); **unverified** end-to-end behaviour of a router under ACL |
| **F. Two-phase: app-level rendezvous over zenoh pub/sub + second dedicated zenoh session** | Daemons exchange `ip:port` over a zenoh key-space on the cloud; each then opens a **new** `zenoh::open()` session with `connect/endpoints=[<peer endpoint>]` | Rendezvous channel: negligible payload. **Main session: cloud carries nothing** — direct peer↔peer link | **Yes**, if the exchanged endpoint is publicly reachable from the other side | Each daemon needs a publicly reachable endpoint (public IP, port-forward, reverse tunnel, or a relay the daemons can both reach) | `Session::put` / `declare_subscriber` / `Liveliness::declare_token`; `zenoh::open(Config)` with `connect/endpoints` | **Verified** (API + config schema); "public endpoint strategy" is rdog-side design |
| **G. UDP transport + rdog-implemented hole punching** | zenoh 1.8 **has** a unicast UDP transport (`transport_udp` feature, `zenoh-link-udp-1.8.0`) but **no** address discovery or punching logic | No (if punching succeeds) | Possible, but **entirely rdog-side work** | Symmetric NAT defeat requires a STUN-like server + simultaneous UDP send | `transport_udp` feature; per-endpoint `#bind=`, `#dscp=`, `#iface=` | **Verified** for UDP transport existence; **no zenoh support** for punching (verified by exhaustive search) |
| **H. `loc_addr` locator override** | Rewrite the advertised locator to an externally-visible address | No | Yes | Requires knowing the external address in advance | **NOT IN 1.8.0** — PR #2039 merged **2026-03-16**, three days *after* 1.8.0 (published **2026-03-13**). Zero `loc_addr` occurrences in `Z/`, `ZLINK/`, `ZTRANS/`, `ZC/` | **Verified** (crate publish date + PR close date + grep) |
| **I. `advertise_addr=observed`** | Use the source IP observed in the Hello packet | No | Partial | Only helps behind 1:1 NAT with port preservation; explicitly does not affect gossip | **NOT IN 1.8.0** — PR #2776 still **open**; "does not affect gossip mode enumeration, only direct scouting" | **Verified** |

---

## 3. Evidence per claim

### 3.1 There is no "Access API" / broker-only / rendezvous mode in zenoh 1.8

```
ZP/src/core/whatami.rs:40-45
pub enum WhatAmI {
    Router = 0b001,
    #[default]
    Peer = 0b010,
    Client = 0b100,
}
```

Published public API root of `zenoh 1.8.0` (`docs.rs/zenoh/1.8.0/zenoh/all.html`, root-level items):

```
Structs: Config, Session
Functions: init_log_from_env_or, open, scout, try_init_log_from_env
Enums: config::WhatAmI, qos::{CongestionControl, Priority, Reliability}, ...
Modules: bytes, cancellation, config, handlers, key_expr, liveliness, matching, pubsub, query, sample, scouting, session, shm, time
```

No `access`, `broker`, `relay`, `rendezvous`, `add_link`, or `connect` item exists. A case-insensitive search for `access|Access` across `Z/src` returns only unrelated matches (`access_control` ACL interceptor, doc prose about "accessing" buffers). GitHub issue search `repo:eclipse-zenoh/zenoh "access api"` returns 15 hits, **all** unrelated (e.g. #1568 "core vs optional API", #837 "core API / optional API" — these are about `Sample` getters, not a broker mode).

> **Explicitly marked as inference:** the phrase "Access API" appears to be a mis-recollection of Zenoh 1.x's naming of the high-level `zenoh::open()`/`Session` API (which replaced the removed `zenoh::net` API). I found **no** zenoh 1.8 artifact, doc, or issue using "Access API" to mean a broker-only node. Treat the premise as unverified.

The closest thing to "broker-only" in 1.8 is **policy-level**, via ACL — `ZC/src/lib.rs:882-889`:

```rust
pub access_control: AclConfig {
    pub enabled: bool,
    pub default_permission: Permission,
    pub rules: Option<Vec<AclConfigRule>>,
    pub subjects: Option<Vec<AclConfigSubjects>>,
    pub policies: Option<Vec<AclConfigPolicyEntry>>,
},
```
with `AclConfigRule { id, key_exprs, messages, flows, permission }` (`ZC/src/lib.rs:186-192`) and `AclConfigSubjects { id, key_exprs, cert_common_names, usernames, link_protocols, zids }` (`ZC/src/lib.rs:195-203`). Wired as a transport interceptor at `Z/src/net/routing/interceptor/mod.rs:142` (`res.extend(acl_interceptor_factories(config.access_control())?)`). Official 1.8.0 config doc states default `enabled: false`, `default_permission: "deny"` — "default permission is deny (even if this is left empty or not specified)" (`DEFAULT_CONFIG.json5` @1.8.0).

### 3.2 Gossip = LinkState OAM over existing transport links; locators propagate **one hop via `GossipNet` only**

Two implementations, selected by `Gossip::new` (`Z/src/net/protocol/gossip.rs:59-83`):

```rust
if gossip_multihop {
    Self::Network(Network::new(..., gossip, gossip_multihop, gossip_target, autoconnect, HashMap::new()))
} else {
    Self::Gossip(GossipNet::new(..., gossip, gossip_target, autoconnect, wait_declares))
}
```

**`GossipNet` carries locators** — `Z/src/net/protocol/gossip.rs:441-455`:

```rust
if self.gossip {
    if let Some(idx) = idx {
        self.send_on_links(
            vec![(idx, Details { zid: true, locators: true, links: false })],
            |link| link.zid != zid,
        );
    }
}
```

**`Network` never carries locators.** Exhaustive list of every `Details { … }` construction in `Z/src/net/protocol/network.rs` — lines 235, 598, 649, 665, 876, 884, 894, 923, 958, 976 — **none sets `locators: true`**, and `Details` derives `Default` (`network.rs:48-53`), so `..Default::default()` ⇒ `locators == false`. `make_link_state` (`network.rs:338-346`) only emits locators when `details.locators` is true.

`Gossip`/`GossipNet` is instantiated **only** by the peer HAT (`Z/src/net/routing/hat/p2p_peer/mod.rs:138-150`); the router HAT creates only `Network` instances (`Z/src/net/routing/hat/router/mod.rs:360-387`), and the router's `handle_oam` dispatches OAM_LINKSTATE into `routers_net` (router links) or `linkstatepeers_net` (peer links) — both `Network` (`Z/src/net/routing/hat/router/mod.rs:629-660`).

**Consequence:** a `mode=router` cloud node relays peer **topology** but **strips locators**, so the far peer never learns a reachable address and never attempts a direct link. A `mode=peer` cloud node (GossipNet) **does** relay the source peer's locators to its other links.

### 3.3 Gossip discovery *does* drive direct transport links (when locators are known)

`Z/src/net/protocol/gossip.rs:458-484` (GossipNet):

```rust
if self.gossip && self.autoconnect.should_autoconnect(zid, whatami) {
    // Connect discovered peers
    ...
    if runtime.manager().get_transport_unicast(&zid).await.is_none()
        && runtime.connect_peer(&zid, &locators).await …
```

`Z/src/net/protocol/network.rs:610-613` (Network) and `Z/src/net/runtime/orchestrator.rs:933-1038` (`Orchestrator::connect`) then call `self.manager().open_transport_unicast(endpoint)` — i.e. a **new direct transport link** to the advertised locator, not routing through the intermediate node.

The advertised locator is the node's own listen locator: `Z/src/net/protocol/gossip.rs:276` → `Some(self.runtime.upgrade().unwrap().get_locators())`; `Z/src/net/runtime/orchestrator.rs:1172-1177` (Hello) likewise carries `locators: self.get_locators()`. For a daemon behind a consumer NAT these are `192.168.x.x` / `10.x.x.x` → unreachable from the other LAN.

Autoconnect defaults (`ZC/src/defaults.rs:104-131`): `gossip.enabled = true`, `gossip.multihop = false`, `gossip.target = {router:["router","peer"], peer:["router","peer"]}`, `gossip.autoconnect = {router: [], peer: ["router","peer"]}`. The 1.8.0 config doc explicitly warns `greater-zid` "may not be suited if one of the node is not reachable by the other one, for example because of a private IP".

### 3.4 Linkstate does not create links

`routing/peer/mode` selects the peer HAT (`Z/src/net/routing/hat/mod.rs:272-284`):

```rust
WhatAmI::Peer => {
    if unwrap_or_default!(config.routing().peer().mode()) == *"linkstate" {
        Box::new(linkstate_peer::HatCode {})
    } else { Box::new(p2p_peer::HatCode {}) }
}
```

Default is `"peer_to_peer"` (`ZC/src/defaults.rs:160`). The linkstate peer computes a next hop over the **linkstate graph**, whose edges are *existing transports* (`Z/src/net/routing/hat/linkstate_peer/pubsub.rs:800-838`, `insert_faces_for_subs` walks `net.trees[…].directions` and looks up `tables.get_face(&net.graph[direction].zid)`). Nothing in that path opens a transport. Link creation comes only from `connect/endpoints` or gossip-autoconnect.

Official 1.8.0 doc for `routing/router/peers_failover_brokering` (default `true`): *"a router will forward data between two peers directly connected to it if it detects that those peers are not connected to each other. The failover brokering only works if gossip discovery is enabled and peers are configured with gossip target 'router'."*

### 3.5 No NAT traversal, no hole punching, no STUN — and `loc_addr` is post-1.8.0

Exhaustive search of `Z/src` for `hole[ _-]?punch|\bstun\b|nat[ _-]?traversal|upnp|port[ _-]?forward|\bNAT\b|external[ _-]?address|public[ _-]?ip` → **no matches**. Search of `ZC/src/{lib.rs,defaults.rs,mode_dependent.rs}` for `udp` → **no matches** (so `transport/link/udp/*` does not exist as a config surface).

GitHub search, `repo:eclipse-zenoh/zenoh`:
- `hole punching` → `total_count: 0`
- `NAT` → 6 items; the load-bearing ones:
  - **PR #2039 "Locator address overriding"** — adds `#loc_addr=192.168.0.200` to endpoints. `closed_at: 2026-03-16`. zenoh **1.8.0 was published `2026-03-13T19:46:12Z`** (crates.io API). Grep for `loc_addr` across `zenoh-1.8.0/`, `zenoh-link-1.8.0/`, `zenoh-transport-1.8.0/`, `zenoh-config-1.8.0/` → **zero hits**. ⇒ **not in 1.8.0**.
  - **PR #2776 "`#advertise_addr=observed` connection param"** — `state: "open"` (created 2026-09-08). Body: *"This does not affect gossip mode enumeration, only direct scouting."*
  - **Issue #78 "Routing behind NAT with ROS"** — 2021, closed 2022; the old, unsolvable-NAT workaround is a router + explicit `peer=tcp/<ip>:7447`.

A UDP unicast transport *does* exist in 1.8.0 — `transport_udp` feature (`crates.io/api/v1/crates/zenoh/1.8.0` feature list; `zenoh-link-1.8.0/Cargo.toml:46` → `zenoh-link-udp`; `zenoh-link-1.8.0/src/lib.rs:54-57,252-253` → `LinkKind::Udp`). It is a transport, not a traversal mechanism.

### 3.6 No public API to add connect endpoints at runtime

**Public API index** (`docs.rs/zenoh/1.8.0/zenoh/all.html`) — `Session` exposes no link/connect management.

**`Session::config()`** — `Z/src/api/session.rs:975-978`:

```rust
#[zenoh_macros::unstable]
pub fn config(&self) -> GenericConfig {
    self.0.runtime.get_config()
}
```

`GenericConfig` is `zenoh_config::GenericConfig(Arc<dyn IConfig>)` (`ZC/src/lib.rs:1794-1815`), whose `insert_json5` delegates through `IConfig` → `impl IConfig for Notifier<Config>` (`Z/src/net/runtime/mod.rs:183-185`) → `Notifier::insert_json5`, which **hard-bails** (`Z/src/api/config.rs:271-282`):

```rust
pub fn insert_json5(&self, key: &str, value: &str) -> ZResult<()> {
    if !key.starts_with("plugins/") {
        bail!("Error inserting conf value {} : updating config is only \
                supported for keys starting with `plugins/`", key);
    }
    self.lock_config().insert_json5(key, value)?;
    self.notify(key);
    Ok(())
}
```

⇒ `session.config().insert_json5("connect/endpoints", …)` **fails**.

**Even a successful tree mutation would not be acted upon.** The *only* `subscribe()` on the config notifier in the entire crate is the plugin manager (`Z/src/net/runtime/adminspace.rs:240`, `#[cfg(all(feature = "plugins", feature = "runtime_plugins"))]`). `connect/endpoints` is read exactly once, during session start (`Z/src/net/runtime/orchestrator.rs:234-263` → `connect_peers(&peers, false)`).

**The admin-space runtime-config path is blocked by the same guard** — `Z/src/net/runtime/adminspace.rs:386-428` accepts `PUT @/<zid>/<whatami>/config/<key>` and calls `self.context.runtime.state.config.insert_json5(key, json)`, i.e. the same `Notifier::insert_json5` ⇒ `connect/endpoints` is equally unreachable there.

**Internal-only escape hatches:** `TransportManager::open_transport_unicast` exists (`zenoh-transport-1.8.0/src/unicast/manager.rs:772`) but the manager lives inside zenoh's `Runtime`; `net` is a private module (`Z/src/lib.rs:218` `mod net;`), `IRuntime` (`Z/src/net/runtime/mod.rs:132`) is unreachable externally, and `zenoh::internal::runtime` exposes only `AdminSpace, DynamicRuntime, Runtime, RuntimeBuilder` with read/close/config methods (`Z/src/lib.rs:1064-1068`).

### 3.7 Router-as-relay: no control-plane-only mode, but enforceable ACLs

No config key in `ZC/src/lib.rs` or `DEFAULT_CONFIG.json5` @1.8.0 restricts a router to control-plane traffic. The closest mechanisms:

| Mechanism | Config | Effect |
|---|---|---|
| ACL | `access_control.enabled`, `default_permission:"deny"`, `rules[{key_exprs,messages,flows,permission}]`, `subjects[{usernames,cert_common_names,interfaces,link_protocols,zids}]`, `policies[]` | Drops non-permitted messages at the router, per key-expression / message-type / subject |
| Low-pass filter | `low_pass_filter[{key_exprs,messages,flows,size_limit,interfaces,link_protocols}]` | Drops messages above a serialized size |
| Downsampling | `downsampling[{rules:[{key_expr,freq}],…}]` | Rate-limits per key-expression |
| Transport whitelist | `transport/link/protocols: ["tls"]` | Restricts accepted link protocols |
| QoS overwrite | `qos/network[{key_exprs,messages,flows,qos:{priority,congestion_control,…}}]` | Forces e.g. `background` priority on the relay link |

Relay-path limits that are **config-derived, not benchmarked** (all from `DEFAULT_CONFIG.json5` @1.8.0):
- `transport/link/tx/queue/size.<priority> = 2` batches × `batch_size = 65535` ⇒ **~128 KB in flight per priority per link**. On a slow WAN this saturates immediately.
- `transport/link/tx/queue/congestion_control.drop.wait_before_drop = 1000` µs ⇒ **silent drops** under back-pressure (zenoh pub/sub has no application-level retransmission; `Reliability` is a transport-level flag — `ZP/src/core/mod.rs:476-485`: `BestEffort`/`Reliable`).
- `transport/link/rx/buffer_size = 65535`; `transport/link/rx/max_message_size = 1 GiB`.
- `transport/unicast/max_sessions = 1000`; `transport/unicast/max_links = 1` (one zenoh session per TCP connection).
- The cloud node **terminates** each daemon's TCP/TLS and **re-originates** the payload ⇒ cloud egress bytes ≈ ingress bytes, and cloud egress is billed.
- +1 WAN RTT per message; the embedded router is a single point of failure.

rdog's own build (`Cargo.toml:53`, `:79`) uses `default-features = false`, so **`transport_compression`, `transport_multilink`, `shared-memory`, `transport_quic`, `transport_ws`, and `auth_pubkey` are all OFF** — a WAN relay therefore has no payload compression and no SHM fast path.

### 3.8 App-level metadata propagation over a router works (Q7)

The local application is modelled as a **Client face** in every node mode — `Z/src/net/routing/router.rs:94-105`:

```rust
FaceState::new(
    fid,
    zid,
    WhatAmI::Client,      // ← local app face
    primitives.clone(), …
```

Therefore a peer's own publication is routed by the peer HAT toward its **router face** — `Z/src/net/routing/hat/p2p_peer/pubsub.rs:566-588`:

```rust
if source_type == WhatAmI::Client {
    for face in tables.faces.values() {
        if face.whatami == WhatAmI::Router {
            route.try_insert(face.id, …)
        } else if face.whatami == WhatAmI::Peer
            && initial_interest(face).is_some_and(|i| !i.finalized) { … }
    }
}
```

and the router relays it to the other peer via `hat/router/pubsub.rs:1100+` (`routers_net` / `linkstatepeers_net` trees). ⇒ **A peer can publish an arbitrary rendezvous key (e.g. `rdog/rendezvous/<id>`) and a peer on the far side of a cloud router will receive it.** The router treats payloads as opaque bytes.

Wire-format / lifetime constraints:
- Payload = bytes + optional `Attachment`; routing key = the zenoh key-expression. `Encoding`/`Attachment` are public (`docs.rs` `bytes::Encoding`).
- `put` is **transient** — no storage, no redelivery, no history. Retransmit is rdog's job.
- `Liveliness::declare_token(key)` (`Z/src/api/liveliness.rs:114`) gives presence + automatic undeclare on session close — the natural fit for "who is online" plus a heartbeat.
- `Queryable` + `get` gives request/response; `queries_default_timeout = 10000` ms default.
- `namespace` config prefixes all key-expressions per session — the clean way to isolate rdog's rendezvous namespace on a shared cloud node.

---

## 4. Recommended building blocks for rdog

### Reusable as-is (zenoh provides it)

1. **Cloud node = `mode:"router"`** as the rendezvous relay for *metadata only*, with `access_control` locking it to `rdog/rendezvous/**` (allow `put`/`query`/`reply`/`declare_queryable`/`liveliness_token`), `default_permission:"deny"`, and `transport/link/protocols:["tls"]`. Subjects keyed on the daemon's `auth_usrpwd` username (rdog already has `auth_usrpwd`).
2. **Metadata channel semantics** — use `Liveliness::declare_token("rdog/rendezvous/<node-id>")` for presence + `put` for endpoint advertisements, `get`/`Queryable` for request/response. Set `namespace` so rdog's keys are `rdog/rendezvous/**` end-to-end.
3. **A second, dedicated zenoh session** for the actual peer-to-peer link, opened with `connect/endpoints: ["tls/<peer-public-endpoint>"]` — this is the only supported way to reach a runtime-discovered peer.
4. **`scouting/gossip/enabled:true` on the daemons** is still useful *inside* a LAN (fast local peer discovery without a cloud hop) — just do not expect it to cross NATs.

### Requires rdog-side code (zenoh provides nothing)

5. **A public-endpoint discovery strategy** — zenoh will not tell a daemon its own public address. rdog must obtain one: static public IP + port-forward, a reverse tunnel (SSH/WireGuard/rathole/frp) to a cloud-side TCP port, or an rdog-implemented STUN-like probe.
6. **The handshake protocol** — version, capability negotiation, session/channel ID exchange, and (if the public endpoint differs from the zenoh listen endpoint) a mapping from "advertised endpoint" → "zenoh connect endpoint".
7. **A rendezvous lifetime model** — zenoh `put` does not expire. Use liveliness tokens + periodic refresh + TTL semantics in the payload, and tolerate silent disappearance.
8. **If true P2P across symmetric NAT is a hard requirement**: rdog must implement hole punching itself (UDP is available — `transport_udp` is already enabled in rdog's Cargo.toml) **or** ship a relay/fallback. Zenoh will contribute only the transport, not the traversal.

### If the "cloud carries no payload" requirement is hard

9. **Do not use a zenoh router as the cloud node.** Use it only for the tiny rendezvous exchange, then have each daemon open a **direct** session to the other. If the direct link cannot be established, fall back to routing the payload through the cloud router — and accept that the cloud then carries the payload (document this explicitly; the ACL can be relaxed per-session, but zenoh ACL is key-expression-based, not per-session, so a payload fallback through the same router is awkward — a *separate* router instance or a separate `namespace`+ACL rule set is needed).

### Minimal open questions the design spec still has to answer

| # | Question | Why zenoh cannot answer it |
|---|---|---|
| Q1 | How does each daemon learn its own publicly reachable `ip:port`? | No STUN / external-address discovery in 1.8.0; `loc_addr` is manual and post-1.8.0; `advertise_addr=observed` is unmerged and does not cover gossip. |
| Q2 | Is the cloud node `mode:"router"` (metadata-only, ACL-locked) or `mode:"peer"` (which can relay *locators* via GossipNet)? | A design choice with different consequences: router strips locators (§3.2); peer relays them but then also sits in the data path (peer_to_peer mode does not route peer↔peer data at all — `hat/p2p_peer/pubsub.rs:547-564` only routes to Client faces — so a peer cloud node would need `routing/peer/mode:"linkstate"` to relay data as a fallback). |
| Q3 | Is the rendezvous channel zenoh-native (put/get/liveliness over the cloud router) or a separate minimal protocol? | Both are viable; zenoh-native reuses rdog's existing transport/auth but couples the rendezvous to the cloud's availability and to the ACL key space. |
| Q4 | What is the fallback when the direct link cannot be established — relay through the cloud, or fail closed? | Purely an rdog product/security decision; note the cloud then sees plaintext-or-encrypted payload and becomes a bottleneck/SPOF. |
| Q5 | Does the direct peer session use a second `zenoh::open()` or replace the cloud session? | Zenoh 1.8 cannot add endpoints to a live session (§3.6), so a **second session** is forced. That means two sessions per daemon, with separate key spaces (use `namespace` to keep them apart) and separate auth identities. |
| Q6 | Transport for the direct link — TCP/TLS (reliable, works through most NATs only with a stable public port) or UDP (needs punching)? | Zenoh has both transports but no traversal; the choice is rdog's. |
| Q7 | Does the ACL apply cleanly to a *peer-mode* cloud node? | ACL is a transport interceptor (`net/routing/interceptor/mod.rs:142`) and is mode-agnostic, but **this specific configuration was not exercised in this audit** — treat as unverified. |
| Q8 | What is the cloud node's capacity/enforcement posture? | `transport/unicast/max_sessions = 1000` per node; with `auth_usrpwd` a shared dictionary file (`transport/auth/usrpwd/dictionary_file`) is available for per-daemon credentials. |

---

### Things I could **not** verify (explicitly marked)

- Whether `GossipNet`'s locator relay actually reaches a peer behind the cloud in a live 3-node test (mechanism verified in source; end-to-end not executed).
- Whether the router's `access_control` correctly denies payload while allowing a rendezvous key space in a live test (interceptor wiring verified; behaviour not executed).
- Any zenoh throughput/latency benchmark numbers — all relay figures above are **derived from configuration defaults**, not measured.
- The claim that "Access API" means something in zenoh 1.x — **no supporting artifact found**; treated as a false premise.
- zenoh 1.9+/main-branch behaviour (e.g. whether `loc_addr` or `advertise_addr` land in 1.9) — out of scope for this audit, but relevant to a version-bump decision.
