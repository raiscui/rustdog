> **本文件是 wayfinder research 产出,仅存在于 `research/rustdesk-dependency-reuse` 分支,不会被合并。**
> 关联 ticket: #125(审计:RustDesk 自身能否作为 rdog 的 Rust 依赖直接复用) · 所属 map: #106
>
> **中文摘要**
>
> - **判定:rdog 不能把 RustDesk 当 crate 依赖。但决定性死因不是许可证,是结构性的。** rdog 真正想要的三个能力——**UDP 打洞**(`rustdesk/src/common.rs:2884`)、**NAT 分类**(`common.rs:705-756`)、**lane 状态机**(`client.rs:400-410,1429-1459`)——**全部住在 `rustdesk` 应用 crate 里,不在 `hbb_common` 库 crate 里**。`hbb_common/src/udp.rs` 全文 **171 行,零 NAT/打洞逻辑**。→ **"依赖 hbb_common 拿到 RustDesk 的 NAT 穿越"不成立。**
> - **许可证(第二道门,而且它过得去)**:`rustdesk` / `rustdesk-server` = **AGPL-3.0**;**`hbb_common` 从来没有 LICENSE 文件**(全 465 次提交历史 grep 为空),`Cargo.toml` 也无 `license` 字段 → 单独看保留一切权利。AGPL §13 ¶2 是**单向**许可(可与 GPLv3 链接交付,但组合作品整体按 §5(c) 授权为 AGPL)→ rdog 须改 `AGPL-3.0-only`;**双许可不可得**;**AGPL 传染单向且不可逆**。
> - **更正一个法律前提**:GPL-3.0 **没有 §4(d)**,也**不含 "remotely through a computer network"**;GPL 的网络相关条款是 §6(d) 且附着于**交付/分发**。真正新增触发条件的是 **AGPL §13 ¶1:不经分发、仅通过网络使用修改版**。→ 实际差别最大的是"改了 rdog、在 VPS 跑 daemon、**从不分发**":GPL 下无义务,AGPL 下团队成员成了 remote users 必须拿到源码。
> - **没有任何东西发布过**:crates.io 上 `hbb_common` / `hbbs` **不存在**;而 crates.io 上叫 **`rustdesk`** 的 crate 是**同名的无关 MIT 拼图窗口管理器**(46 行)——**重名陷阱**。`hbb_common` 的 `version = "0.1.0"` 从未变过 → 只能 pin 移动中 `main` 的 rev。
> - **依赖树无法 feature 化**:唯一 optional 是 webrtc;无条件带 tokio(full)、sodiumoxide、tokio-native-tls(**Linux 上拖 OpenSSL**)、**fork 的 tokio-tungstenite 0.29**——**rdog 已有 tungstenite 0.27 → 树里两个不兼容 major**——外加 6 个 git 依赖,其中 `sysinfo` pin 在**分支**不是 rev。
> - **`hbbr` 是 bin-only**(`relay_server` 不在 lib.rs)→ link 不进来。
> - **route (c) 协议对接折叠进 route (a)**:hbbs 只做信令,数据面是 RustDesk 的 KCP+secretbox 会话栈,rdog 要带自己的协议上去 = 重新实现 `rendezvous_server.rs`,比"写 rdog 自己的 rendezvous"更多工;且 **ID 空间映射有损**(RustDesk 扁平一机一串 vs rdog 层级式)。
> - **复用的正确边界比"读而不抄"更严**:逐字抄 120 行 `bytes_codec.rs` 就是衍生作品 → 依 §5(c) 整个 rdog 变 AGPL。正确线是 **"重实现算法与线格式设计;绝不逐字抄源码"**。值得干净室重写的只有两个小算法(约 200–300 行):①两目标端口 NAT 分类探针(约 40 行)②带 tid + 指数退避(20ms→200ms)+ 3s 绝对截止的 probe/ack 打洞握手(难点是并发纪律:绝对时刻而非相对 sleep、监听端在收到对端第一个真包时停)。
> - **继承的坑**:NAT 二分会把"EI 但 port-restricted"误判成 ASYMMETRIC、"AD 但 port-independent"误判成 SYMMETRIC → 选错传输;粘性降级(计数只记跳变、中继会话永不重置);1000 ms 超时地板**低于首个 SYN 重传**;**1 GiB 帧上限且 `max_packet_length` 默认 `usize::MAX`、`rustdesk-server/src` 从未调用 `set_max_packet_length`** → hbbr 每连接可缓冲约 1 GiB;`hbb_common/src/verifier.rs` 有 `NoVerifier`(接受任意证书);UDP 打洞探针的反射/放大面。
> - **webrtc fork churn 已在 `hbb_common` 实际发生**:webrtc 钉在 0.13 并留了一份**手写升级 checklist**;`2026-09-19 bump webrtc fork … (#16263)`。
> - **分层判定**:协议参考 ✅ / 逐字借用 ⚠️(法律代价大)/ **干净室重实现 ✅ 推荐** / crate 依赖 ❌ / 整进程集成 ❌。
> - **新决策点**:`hbb_common` 无 MSRV 声明、rustdesk app 1.75 → RustDesk 不是 MSRV 问题;但 **iroh 1.4.1 要 rustc 1.91 > rdog 1.89,而 iroh 0.98.2 是 1.89** → 抬 MSRV 还是锁旧版,已单开票。

# Can rdog depend on RustDesk's code? — Evidence Report

**Scope:** `rustdesk/rustdesk`, `rustdesk/hbb_common`, `rustdesk/rustdesk-server` @ `rustdesk`=2b6fedfc8e8e8ba35ec764dbcc81210cb5343333 (2026-10-22), `hbb_common`=ed0743102152d76dab9e4f7ec75f47a0174afa60 (2026-10-20), `rustdesk-server`=873230e35a8805de7fd057e7eb763c46f7a174ef (2026-10-10). Clones inspected read-only in sandbox temp; **no rdog file was modified**.

---

## 1. Verdict

**No — rdog cannot depend on RustDesk code as a crate dependency, and the license gate is not the one that decides it.** Four independent blockers exist, and the decisive one is structural: **every capability rdog would want (hole punch, NAT classification, lane state machine) lives in the `rustdesk` *application* crate, not in the publishable `hbb_common` library**, so "depend on `hbb_common` to get RustDesk's NAT traversal" is impossible — `hbb_common/src/udp.rs` is a 171-line socket helper with zero NAT logic, and the punch code sits in `rustdesk/src/common.rs:2884` / `rustdesk/src/rendezvous_mediator.rs:1042-1620` / `rustdesk/src/client.rs:400-1459`. Independently, **nothing is published**: `hbb_common` and `hbbs` do not exist on crates.io, and the `rustdesk` crate that does exist there is an unrelated 46-line MIT tiling window manager — so any dependency would be an unpinned `git = ...` pull of a moving `main` branch whose declared `version = "0.1.0"` has never moved. Independently again, **`hbb_common` has no license at all** — no LICENSE file in its entire 465-commit history and no `license` field in its `Cargo.toml` — so it is legally all-rights-reserved standalone, while `rustdesk` and `rustdesk-server` are **AGPL-3.0**, which would force rdog's combined work to AGPL-3.0 under AGPL §5(c). And independently fourth, **`hbb_common`'s dependency tree cannot be feature-gated down to the useful parts**: the only optional dependency is `webrtc`; `tokio` (full), `sodiumoxide`, `tokio-native-tls` (⇒ OpenSSL), a *forked* `tokio-tungstenite`, and five other git dependencies are unconditional. So route (a) is dead on four counts and should be closed, not deferred. **The license gate alone does not kill the family of options** — switching rdog to `AGPL-3.0-only` is a one-line `Cargo.toml` change and is legally sufficient to link AGPL code — but it is a *product* decision with real downstream cost, and it becomes moot once the code-location and publication facts are applied. **The correct layer for rdog is: RustDesk as protocol reference and algorithmic precedent, with a clean-room reimplementation of the two small algorithms that are worth having (the two-destination-port NAT classification, and the probe/acknowledgement punch handshake), and an independently written frame codec.** Route (b) whole-process integration is killed by the one-binary constraint; route (c) protocol interop has near-zero semantic value because RustDesk's rendezvous server only signals — the data path is RustDesk's own KCP/secretbox session stack, which rdog would have to reimplement anyway, thereby collapsing route (c) back into route (a).

---

## 2. License analysis

### 2.1 The actual licenses

| Repo | License file | License | Size | `license` field in `Cargo.toml`? |
|---|---|---|---|---|
| `rustdesk/rustdesk` | `LICENCE` (note spelling) | **AGPL-3.0** | 34,523 B | **No** — `rustdesk/Cargo.toml` has no `license` key |
| `rustdesk/rustdesk-server` | `LICENSE` | **AGPL-3.0** | 34,523 B | **No** — `rustdesk-server/Cargo.toml:1-15` has no `license` key |
| `rustdesk/hbb_common` | **none — never existed** | **undetermined / all-rights-reserved** | — | **No** — `hbb_common/Cargo.toml` has no `license` key |

- `rustdesk/LICENCE:1-2` → "GNU AFFERO GENERAL PUBLIC LICENSE / Version 3, 19 November 2007". https://github.com/rustdesk/rustdesk/blob/master/LICENCE
- `rustdesk-server/LICENSE:1-2` → identical AGPL-3.0 text. https://github.com/rustdesk/rustdesk-server/blob/master/LICENSE
- `hbb_common`: `git log --all --diff-filter=AD --name-only -- 'LICENSE*' 'LICENCE*'` over the **full unshallowed 465-commit history returns nothing**; root contains only `.gitignore`, `Cargo.toml`, `build.rs`, `examples/`, `protos/`, `src/`, `tests/`. There is also no README. The only license coverage is by inheritance from being a submodule (`rustdesk/.gitmodules`: `[submodule "libs/hbb_common"] url = https://github.com/rustdesk/hbb_common`) inside the AGPL `rustdesk` tree.
- No SPDX headers and no "or any later version" grants in `rustdesk/src/**` (`grep -rn "SPDX\|or any later version" src/` → none), and neither manifest declares a license. So "AGPL-3.0" here is, practically, **AGPL-3.0 with no "or later" grant**.

### 2.2 AGPL-3.0 §13's GPLv3-compatibility clause — precisely

`rustdesk/LICENCE:540-560`:

> **13. Remote Network Interaction; Use with the GNU General Public License.**
>
> Notwithstanding any other provision of this License, if you modify the Program, your modified version must prominently offer all users interacting with it remotely through a computer network (if your version supports such interaction) an opportunity to receive the Corresponding Source of your version by providing access to the Corresponding Source from a network server at no charge, through some standard or customary means of facilitating copying of software. This Corresponding Source shall include the Corresponding Source for any work covered by version 3 of the GNU General Public License that is incorporated pursuant to the following paragraph.
>
> Notwithstanding any other provision of this License, you have permission to link or combine any covered work with a work licensed under version 3 of the GNU General Public License into a single combined work, and to convey the resulting work. The terms of this License will continue to apply to the part which is the covered work, but the work with which it is combined will remain governed by version 3 of the GNU General Public License.

The second paragraph is the compatibility clause. It is **one-directional permission flowing AGPL → GPLv3**: it says you *may* link an AGPL "covered work" with a GPLv3 work and convey the result. It does **not** say the result may be conveyed under GPLv3. Combined with AGPL §5(c) (`rustdesk/LICENCE:210`):

> c) You must license the entire work, as a whole, under this License to anyone who comes into possession of a copy.

…the combined work must be offered to recipients under **AGPL-3.0**. rdog's own GPLv3-only portion remains governed by GPLv3 (GPLv3 downstream rights to that portion are preserved and cannot be revoked down to AGPL-only), but the *work as a whole* carries AGPL obligations.

**Consequence for rdog's manifest:** `license = "GPL-3.0-only"` → `license = "AGPL-3.0-only"`.

**Dual-licensing `AGPL-3.0-only OR GPL-3.0-only` is NOT available to rdog.** The AGPL-covered portion (hbb_common / the borrowed RustDesk fragments) can only be conveyed under AGPL-3.0 unless the copyright holder (Purslane Tech / RustDesk) grants a broader grant. They have not. (GPLv3 §13 at `rdog/LICENSE:552-562` is the mirror-image clause: it lets GPLv3 code be combined *with* AGPLv3 code, with "the special requirements of the GNU Affero General Public License, section 13, concerning interaction through a network will apply to the combination as such.")

### 2.3 The network clause — and correcting the premise in the question

**The premise that "GPL-3.0 §4(d) covers network distribution of the covered work" is factually wrong.** Verified against rdog's own bundled GPL-3.0 text (`rdog/LICENSE`):

- **GPL-3.0 §4 is "Conveying Verbatim Copies" and contains exactly two paragraphs. There is no §4(a)/(b)/(c)/(d).** (`rdog/LICENSE:195-207`.)
- **The phrase "remotely through a computer network" does not occur anywhere in GPL-3.0.** (`grep -n "remotely through a computer network" rdog/LICENSE` → no match.)
- GPL-3.0's network-adjacent clause is **§6(d)** (`rdog/LICENSE:275`), and it is about *conveying object code* ("Convey the object code by offering access from a designated place (gratis or for a charge) … If the place to copy the Corresponding Source is a network server, the Corresponding Source may be on a different server …"). It attaches when you **convey** the work.

**The real delta:** AGPL-3.0 §13 ¶1 adds a trigger GPL-3.0 does not have — **network use without distribution**.

| Scenario | GPL-3.0-only (today) | AGPL-3.0-only (if rdog switches) |
|---|---|---|
| Ship unmodified `rdog` binary | Ship source alongside (§4/§6) | Same |
| **Patch rdog, run `rdog daemon` on the VPS, never distribute the binary** | **No source obligation at all** | **Must offer the Corresponding Source to every remote user of that daemon** (§13 ¶1) |
| Patch rdog and distribute inside a company | Source to recipients (§6) | Source to recipients (§6 + §13) |
| Patch rdog, run it as a daemon for your own team, never distribute | No obligation | **Team members become "remote users" → source must be offered to them** |
| rdog links AGPL code | n/a | Combined work's Corresponding Source must **include the AGPL dependency's sources** (§13 ¶1 final sentence) |

**What concretely changes for rdog's users:** rdog's audience includes people who patch and self-host (reverse-shell / remote-ops tooling attracts exactly this). Under AGPL those users become obliged to publish their patches to their users. For a company running `rdog daemon` as a shared internal service, AGPL reaches the "run a modified daemon internally, don't distribute" case that GPL deliberately leaves alone — that is the single biggest practical change.

**What changes for self-hosted distribution:** rdog's `LICENSE` file must be swapped from GPL-3.0 (35,149 B) to AGPL-3.0 (34,523 B); every `Cargo.toml` in the workspace must change to `AGPL-3.0-only`; and downstream packagers who require GPL-3.0-only (Debian `main` policy, many enterprise allowlists) can no longer take rdog. It also means rdog can never be relicensed back to GPL-3.0-only once AGPL code is linked in — **the AGPL contagion is one-way and permanent**.

**Is switching acceptable?** It is *mechanically* trivial and *legally* sufficient, so the license gate by itself does not kill the "depend on RustDesk" family. But it is a strategic downgrade of rdog's license surface for zero functional benefit, because — as §3 shows — the code rdog would actually want is not in a linkable library. **Judgement: do not switch. The license gate is not the binding constraint; it is a second independent reason not to proceed.**

---

## 3. Capability-to-crate table

| Capability | Repo | Crate / target | Published? | File:line |
|---|---|---|---|---|
| UDP hole punch (probe/ack, backoff, absolute deadline) | `rustdesk/rustdesk` | `rustdesk` (app; bin + `librustdesk` lib) | **No** — path deps `libs/scrap` etc. | `rustdesk/src/common.rs:2884` `pub async fn punch_udp`; constants `:2849-2852` |
| Punch wire format (4-byte magic + 8-byte LE tid, 12 B) | `rustdesk/rustdesk` | `rustdesk` | No | `rustdesk/src/common.rs:2849` `PUNCH_PROBE = b"RDP?"`, `:2850` `PUNCH_ACK = b"RDP!"`, `:2852` `PUNCH_PACKET_LEN = 12`, `:2854` `punch_packet` |
| Punch orchestration / `punch_until` / TCP punch | `rustdesk/rustdesk` | `rustdesk` | No | `rustdesk/src/rendezvous_mediator.rs:1042` `handle_punch_hole`, `:1198` `punch_udp_hole`, `:1532` `punch_tcp_until_connected`, `:1620` `punch_until` |
| **NAT classification (asymmetric vs symmetric)** | `rustdesk/rustdesk` | `rustdesk` | No | `rustdesk/src/common.rs:705-756` `test_nat_type_` — two TCP conns to two dest ports from same local socket; `:747-751` `port1 == port2 → ASYMMETRIC else SYMMETRIC` |
| Proxy present ⇒ force SYMMETRIC | `rustdesk/rustdesk` | `rustdesk` | No | `rustdesk/src/common.rs:674-679` |
| **Symmetric ⇒ force relay (server side)** | `rustdesk/rustdesk-server` | `hbbs` | **No** | `rustdesk-server/src/rendezvous_server.rs:776` `ph.nat_type = NatType::SYMMETRIC.into(); // will force relay` |
| Symmetric ⇒ force relay (answerer side) | `rustdesk/rustdesk` | `rustdesk` | No | `rustdesk/src/rendezvous_mediator.rs:1091-1097` (`relay_v4`), `:1117-1131` |
| **Lane state machine / `direct_failures`** | `rustdesk/rustdesk` | `rustdesk` | No | `rustdesk/src/client.rs:400-410` (update logic), `:1429-1459` (timeout), `:3171` `set_direct_failure` |
| Direct-connect timeout floor 1000 ms | `rustdesk/rustdesk` | `rustdesk` | No | `rustdesk/src/client.rs:1431` `const MIN: u64 = 1000;`, clamped `:1457-1459` |
| Rendezvous protobuf messages | `rustdesk/hbb_common` | `hbb_common` | **No — not on crates.io** | `hbb_common/protos/rendezvous.proto` (310 lines); `:283-310` `RendezvousMessage` oneof (24 variants); `:15-18` `RegisterPeer{id,serial}`; `:96-101` `NatType` (3 states only) |
| **Relay frame codec** (4-byte varint header) | `rustdesk/hbb_common` | `hbb_common` | No | `hbb_common/src/bytes_codec.rs` — `:6` `MAX_PREALLOCATED_PAYLOAD_LEN = 256*1024`, `:8` `MAX_FRAME_LENGTH = 0x3FFF_FFFF` (~1 GiB), `:42` `set_max_packet_length`, `:63-68` capped reserve |
| Relay transport (`FramedStream` + `BytesCodec`) | `rustdesk/hbb_common` | `hbb_common` | No | `hbb_common/src/tcp.rs:66, 73, 137, 171`; used by `rustdesk-server/src/relay_server.rs:11` |
| **UDP socket helper** (the file the prior audit pointed at) | `rustdesk/hbb_common` | `hbb_common` | No | `hbb_common/src/udp.rs` — **171 lines total, no punch/NAT/classify logic**; `grep -rn "punch\|Symmetric\|nat_type" hbb_common/src/` → only `config.rs:244/994-1004` (a persisted `i32` config field) and `webrtc.rs:837` (a comment) |
| E2E transport crypto (NaCl secretbox handshake) | `rustdesk/hbb_common` | `hbb_common` | No | `hbb_common/src/tcp.rs:359-419` (`secretbox::KEYBYTES`, pk length checks) |
| App-level key exchange | `rustdesk/rustdesk` | `rustdesk` | No | `rustdesk/src/common.rs:2402` `create_symmetric_key_msg` |
| TLS | `rustdesk/hbb_common` | `hbb_common` | No | `hbb_common/Cargo.toml:54-58` `tokio-rustls 0.26` (ring, tls12), `:59` `tokio-native-tls 0.3`, `:63` forked `tokio-tungstenite` with `native-tls`+`rustls` features ⇒ **OpenSSL on Linux**; `hbb_common/src/tls.rs` `TlsType{Plain,NativeTls,Rustls}`; `hbb_common/src/verifier.rs` has a `NoVerifier` (accepts any cert) |
| **Relay server (hbbr)** | `rustdesk/rustdesk-server` | `hbbr` — **bin-only** | No | `rustdesk-server/src/hbbr.rs:5-7` `mod common; mod relay_server; mod version;` — `relay_server` is **not** in `lib.rs`; `rustdesk-server/src/relay_server.rs` |
| **Rendezvous server (hbbs)** | `rustdesk/rustdesk-server` | `hbbs` — has a **lib** target | No | `rustdesk-server/src/lib.rs:1-6` `mod rendezvous_server; pub use rendezvous_server::*; pub mod common; mod database; pub mod peer; pub mod version;` ⇒ linkable in principle |
| `nat_type` config storage (not classification) | `rustdesk/hbb_common` | `hbb_common` | No | `hbb_common/src/config.rs:244` field, `:994-1004` get/set |

**Confirmation of the prior audit's structural claim: CONFIRMED.** The punch logic is **not** in a publishable library crate. `hbb_common` contains the *vocabulary* (protobuf, `NatType` enum, frame codec, socket helper) but **not the algorithm**.

**Can the server side be linked into another binary?** Partially:
- `hbbs` (rendezvous server): **yes**, `src/lib.rs` exposes `pub use rendezvous_server::*` — but the crate is named `hbbs` and would have to be pulled as an unpinned git dep with its own `hbb_common` path submodule, plus it pulls `axum 0.5`, `sqlx 0.6`, `bcrypt`, `jsonwebtoken`, `tokio-tungstenite 0.17`, `deadpool` — a second, *older* tokio-tungstenite major version alongside `hbb_common`'s 0.29 fork.
- `hbbr` (relay server): **no** — `relay_server.rs` lives only in the `hbbr` binary's module tree (`src/hbbr.rs:6`), not in `lib.rs`. Reusing it requires copying the module or patching upstream's `lib.rs`.

---

## 4. Evidence table

| Claim | Source | Verified? |
|---|---|---|
| `rustdesk/rustdesk` is AGPL-3.0 | `https://github.com/rustdesk/rustdesk/blob/master/LICENCE` (34,523 B, "GNU AFFERO GENERAL PUBLIC LICENSE / Version 3, 19 November 2007") | **Verified** (read in full) |
| `rustdesk/rustdesk-server` is AGPL-3.0 | `https://github.com/rustdesk/rustdesk-server/blob/master/LICENSE` (34,523 B, identical header) | **Verified** |
| `hbb_common` has **no** LICENSE file, ever | `git log --all --diff-filter=AD --name-only -- 'LICENSE*' 'LICENCE*'` over unshallowed 465-commit history → empty; root listing confirms | **Verified** |
| `hbb_common` Cargo.toml has no `license` key | `hbb_common/Cargo.toml:1-7` (`name/version/authors/edition` only) | **Verified** |
| `hbb_common` not published on crates.io | `https://crates.io/api/v1/crates/hbb_common` → `{"errors":[{"detail":"crate `hbb_common` does not exist"}]}` | **Verified** |
| `hbbs` not published on crates.io | `https://crates.io/api/v1/crates/hbbs` → "crate `hbbs` does not exist" | **Verified** |
| crates.io `rustdesk` is an unrelated MIT project | `https://crates.io/api/v1/crates/rustdesk` → "minimum tiling window manager based on penrose", license MIT, 46 Rust LOC, 2022-03-27, author "Broken Pipe" | **Verified** |
| `rustdesk` app pulls `hbb_common` by **path**, not version | `rustdesk/Cargo.toml:55` `hbb_common = { path = "libs/hbb_common", features = ["webrtc"] }`; `:270` build-dep same | **Verified** |
| `libs/hbb_common` is a git submodule | `rustdesk/.gitmodules` → `url = https://github.com/rustdesk/hbb_common` | **Verified** |
| `rustdesk` app is unpublishable (path deps) | `rustdesk/Cargo.toml:54,56,102,103,151,152` + `[workspace] members = ["libs/scrap","libs/hbb_common","libs/base","libs/enigo","libs/clipboard","libs/virtual_display","libs/virtual_display/dylib","libs/portable","libs/remote_printer"]` | **Verified** |
| `rustdesk` app declares a lib target | `rustdesk/Cargo.toml:11-13` `[lib] name = "librustdesk"  crate-type = ["cdylib","staticlib","rlib"]` | **Verified** (but unusable in practice — drags sciter-rs, flutter_rust_bridge, scrap) |
| `hbb_common` version has never been released | `hbb_common/Cargo.toml:3` `version = "0.1.0"`, `edition = "2018"` | **Verified** |
| `hbb_common` only optional dep is `webrtc` | `hbb_common/Cargo.toml:9-11` `[features] default = [] ; webrtc = ["dep:webrtc"]`; `:74` `webrtc = { version = "0.13.0", optional = true }` | **Verified** |
| `hbb_common` pulls OpenSSL on Linux | `hbb_common/Cargo.toml:59` `tokio-native-tls = "0.3"`; `:63` tokio-tungstenite features include `"native-tls"` | **Verified** (manifest) |
| `hbb_common` has 6 unconditional git deps | `hbb_common/Cargo.toml:33` confy, `:37` tokio-socks, `:44` sysinfo (**branch**, not rev), `:63` tokio-tungstenite (**rev**), `:79` default_net, `:80` machine-uid | **Verified** |
| `hbb_common`'s `[patch.crates-io]` is ignored for consumers | `hbb_common/Cargo.toml:95-96` comment: "Only takes effect when this crate is built on its own …; a consumer's own `[patch]` governs its build" | **Verified** (comment) — *but the underlying cargo behaviour when the patched crates are absent from the graph was not empirically tested* |
| `protobuf-codegen` needs no `protoc` | `hbb_common/build.rs:6` `.pure()` → pure-Rust parser | **Verified** (code) |
| `hbb_common` MSRV | `hbb_common/Cargo.toml` — **no `rust-version` key**; `rustdesk/Cargo.toml:9` `rust-version = "1.75"` | **Verified** |
| rdog MSRV / license | `rdog/Cargo.toml:6` `license = "GPL-3.0-only"`, `:9` `rust-version = "1.89"` | **Verified** |
| rdog has **no** tokio dependency | `grep -n "tokio" rdog/Cargo.toml` → no match; `zenoh = "1.8.0"` only | **Verified** |
| iroh current MSRV | `https://crates.io/api/v1/crates/iroh` → 1.4.1 `rust_version: "1.91"`, license `MIT OR Apache-2.0`; 0.98.2 `rust_version: "1.89"` | **Verified** |
| `direct_failures` sticky degradation | `rustdesk/src/client.rs:400-410` — update guarded by `(direct_failures == 0) != direct`; a **relay** success (`direct == false`) with `direct_failures == 1` leaves it at 1; `:1453` `let n = if direct_failures > 0 { 3 } else { 6 };` | **Verified** |
| 1000 ms direct timeout floor | `rustdesk/src/client.rs:1431` `const MIN: u64 = 1000;`, `:1457-1459` `if connect_timeout < MIN { connect_timeout = MIN; }` | **Verified** |
| Symmetric NAT ⇒ force relay | `rustdesk-server/src/rendezvous_server.rs:776`; `rustdesk/src/rendezvous_mediator.rs:1091-1097` | **Verified** |
| NAT classification is binary, no port prediction | `rustdesk/protos`→`hbb_common/protos/rendezvous.proto:96-101` `NatType{UNKNOWN_NAT,ASYMMETRIC,SYMMETRIC}`; `rustdesk/src/common.rs:747-751` | **Verified** |
| 1 GiB frame surface, unmitigated on the relay path | `hbb_common/src/bytes_codec.rs:8` `MAX_FRAME_LENGTH = 0x3FFF_FFFF`; `grep -rn "set_max_packet_length" rustdesk-server/src` → **no matches** (only `rustdesk/src/port_forward_mux.rs:46`, `rustdesk/src/server/connection.rs:1895`, `rustdesk/src/server.rs:230`) | **Verified** |
| `bytes_codec` preallocation is already capped upstream | `hbb_common/src/bytes_codec.rs:6, 63-68` | **Verified** |
| `client.rs` commit cadence ≈ 2–7/month | `git log --oneline --since=2025-10-01 -- src/client.rs \| wc -l` → **34 commits over ~13 months ≈ 2.6/month** | **Verified** |
| webrtc fork is bumped repeatedly | `git log --format="%ad %s" --date=short -- src/client.rs` → `2026-09-19 bump webrtc fork: one IPv6 host candidate per interface and prefix (#16263)`; pin at `hbb_common/Cargo.toml:97-99` (rev `49c89bd8`), comment `:70-73` explains the 0.13 pin and the rustc-1.75 CI constraint | **Verified** |
| RustDesk ID space is flat, one ID per peer | `hbb_common/protos/rendezvous.proto:15-18` `RegisterPeer{ id, serial }`; `:113-135` `RegisterPk`/`RegisterPkResponse` | **Verified** (proto shape) — *the exact ID string format/normalization rules were not traced* |
| GPL-3.0 has no §4(d) and no "remotely through a computer network" | `rdog/LICENSE:195-207` (§4 = 2 paragraphs); `grep -n "remotely through a computer network" rdog/LICENSE` → no match; `rdog/LICENSE:552-562` (§13) | **Verified** |
| AGPL §13 ¶2 link/combine permission | `rustdesk/LICENCE:540-560` | **Verified** (verbatim read) |
| AGPL §5(c) license-whole-work | `rustdesk/LICENCE:210` | **Verified** |

---

## 5. Dependency footprint & feature gating (§3 of the brief)

**Can feature gates isolate `bytes_codec` + crypto and avoid the webrtc/image stacks?** **Partially — and the part that matters is not the one that matters.**

| Concern | Finding |
|---|---|
| webrtc | **Avoidable.** `optional = true` behind `feature = "webrtc"` (`hbb_common/Cargo.toml:11, 74`). rdog would not enable it. But `hbb_common/Cargo.toml:96-99` still declares `[patch.crates-io]` for webrtc/webrtc-util/webrtc-sctp — and hbb_common's own comment states the patch "only takes effect when this crate is built on its own"; a consumer must supply its own. If cargo eagerly resolves patch entries even for absent dependencies, rdog inherits a mandatory git fetch of `rustdesk-org/webrtc` (rev `49c89bd8`). **Unverified empirically.** |
| image stack | **Not in `hbb_common`.** `image = "0.24"` is in `rustdesk/Cargo.toml` under the macOS/Linux/Windows target block, not in `hbb_common/Cargo.toml`. The brief's premise here is wrong — the image stack is an *app-crate* cost, and rdog already depends on `image 0.25` itself. |
| tokio | **Not avoidable.** `tokio = { version = "1.44", features = ["full"] }` (`hbb_common/Cargo.toml:17`) — unconditional, `full`. |
| crypto | **Not avoidable.** `sodiumoxide = "0.2"` (`:35`) unconditional; `tokio-rustls 0.26` (`:54`) + `tokio-native-tls 0.3` (`:59`) + `rustls-platform-verifier`, `rustls-native-certs`, `webpki-roots` unconditional. |
| WS | **Not avoidable.** Forked `tokio-tungstenite` rev `c241b11b` (`:63`) + upstream `tungstenite 0.29` with `native-tls`+`rustls` features. rdog already uses `tungstenite 0.27` ⇒ **two incompatible tungstenite majors in the tree**, both compiled. |
| misc unconditional | `flexi_logger`, `env_logger`, `zstd`, `confy`(git), `dirs-next`, `sysinfo`(git, **branch** `rlim_max`), `machine-uid`(git), `default_net`(git), `tokio-socks`(git), `uuid`, `sha2`, `httparse`, `regex`, `chrono`, `libc`, `dlopen`, `libloading`, `whoami`, `async-recursion`, `directories-next`, `serde_json`, `toml`, `percent-encoding`, `url`, `base64`, `rand`, `lazy_static`, `thiserror`, `anyhow`, `bytes`, `futures*`, `log`, `socket2` ⇒ **~45 crates + 6 git repos** for a non-webrtc build. |
| MSRV | `hbb_common` declares **none**; `rustdesk` app declares `1.75`. rdog is `1.89` ⇒ RustDesk is *not* an MSRV problem. **iroh is**: current iroh 1.4.1 needs `1.91` > rdog's `1.89` (iroh 0.98.2 is `1.89`). |
| TLS backend | **Both** rustls (ring) and native-tls ⇒ **OpenSSL on Linux** via `native-tls`. rdog currently has no OpenSSL dependency and builds fully static-friendly binaries; adding it is a real supply-chain/packaging regression on Linux. |

**Verdict on §3: feature gating does *not* isolate the useful parts.** The only optional dependency is `webrtc`. `bytes_codec` and the crypto are bundled with a mandatory `tokio`+`full`, mandatory OpenSSL-capable TLS, a forked WebSocket stack, and six git dependencies — of which `sysinfo` is pinned to a **branch**, not a rev (`hbb_common/Cargo.toml:44`), meaning it can move under rdog's lockfile.

---

## 6. Protocol coupling (§4 of the brief)

**Are `hbb_common`'s rendezvous messages hard-bound to RustDesk's wire protocol? Yes, completely.**

- `RendezvousMessage` is a `oneof` of **24 variants** (`hbb_common/protos/rendezvous.proto:283-310`): `register_peer`, `register_peer_response`, `punch_hole_request`, `punch_hole`, `punch_hole_sent`, `punch_hole_response`, `fetch_local_addr`, `local_addr`, `configure_update`, `register_pk`, `register_pk_response`, `software_update`, `request_relay`, `relay_response`, `test_nat_request`, `test_nat_response`, `peer_discovery`, `online_request`, `online_response`, `key_exchange`, `hc`, `http_proxy_request`, `http_proxy_response`, `ice_candidate`.
- The NAT test rides the **same** message channel: `TestNatRequest{serial}` / `TestNatResponse{port, cu}` (`:86-94`), with `serial` as the per-process identity correlating the two probes (`rustdesk/src/common.rs:711-715`).
- The punch flow is `PunchHoleRequest` (`:31-48`, carries `socket_addr`, `udp_port`, `relay_server`, `nat_type`, `webrtc_sdp_offer`, `uuid`) → `PunchHole` (`:73-85`) → `PunchHoleSent` (`:102-112`, carries `socket_addr`, `id`, `relay_server`, `nat_type`, `version`, `upnp_port`, `socket_addr_v6`, `webrtc_sdp_answer`) → `PunchHoleResponse` (`:136-158`).
- Identity is `RegisterPeer{ id: string, serial: int32 }` (`:15-18`) plus a separate public-key registration `RegisterPk`/`RegisterPkResponse` (`:113-135`).

**Would rdog have to implement an hbbs-compatible server?** **Yes** — and this is where the force-relay decision actually lives: `rustdesk-server/src/rendezvous_server.rs:776` is the server-side code that overrides `ph.nat_type` to SYMMETRIC to force the relay. Reusing RustDesk's *mechanism* (not just its client-side punch code) means reimplementing the hbbs TCP framing (`hbb_common::tcp::FramedStream` + `BytesCodec`), the protobuf message set, the ID→(socket_addr, nat_type) registry, and the public-key registration handshake. That is a re-implementation of `rustdesk-server/src/rendezvous_server.rs` — i.e. **route (c) is "write hbbs", and "write hbbs" is strictly more work than "write rdog's own rendezvous"**, because rdog's would be built on rdog's own protocol and identity model.

**Can the algorithm be taken without the message format? Yes — for the two parts worth having.** Both are protocol-agnostic in their *idea* and require only a few hundred lines of independent code:

1. **NAT classification** (`rustdesk/src/common.rs:705-756`): open two TCP connections from the *same* local socket to two different destination ports; compare the mapped source ports the server observes. Same port ⇒ endpoint-independent mapping (asymmetric). Different ports ⇒ address-and-port-dependent (symmetric). *Reimplementable in ~40 lines* with rdog's own probe message.
2. **Punch probe/acknowledgement** (`rustdesk/src/common.rs:2849-2910`): 12-byte packet = 4-byte magic (`RDP?`/`RDP!`) + 8-byte little-endian transaction id; exponential backoff 20 ms → 200 ms; 3 s absolute deadline; both ends probe *and* answer. The *hard* part is not the wire format but the concurrency discipline documented in the comments (`:2867-2884`): absolute instants rather than relative sleeps because `select!` rebuilds every arm each iteration; the listener must stop on the peer's first real packet rather than on its own ack, because after the connector stops, KCP's io loop drops anything shorter than its header.
3. **Relay frame codec** (`hbb_common/src/bytes_codec.rs`): 4-byte variable-length header, length `<< 2` with a 2-bit width selector. Trivial length-prefix; write rdog's own.

**Critical caveat — copying ≠ borrowing.** Literally copying these fragments (even 120 lines of `bytes_codec.rs`) makes them part of rdog's covered work ⇒ AGPL-3.0 under §5(c). Reimplementing the *algorithm* from a specification of the idea is not creating a derivative work. **The line must be drawn at "reimplement algorithms, never copy source text."** That is stricter than the brief's "read the code, don't copy the code" and is the only line that leaves rdog on GPL-3.0.

---

## 7. Route comparison (a)/(b)/(c)

| | **(a) crate dependency** | **(b) whole-process integration** | **(c) protocol interop** |
|---|---|---|---|
| **What** | `hbb_common = { git = "https://github.com/rustdesk/hbb_common" }` in rdog's `Cargo.toml` | Run `hbbs`+`hbbr` on the cloud VPS; rdog is only a client | rdog's daemon registers with hbbs as a peer; hbbs coordinates the punch |
| **Cost (build)** | 6 git repos + ~45 crates;2 tungstenite majors; OpenSSL on Linux; `tokio`+`full` in a repo with **zero async runtime** today; unversioned (`version = "0.1.0"`, never released) ⇒ pin by rev on a moving `main` | Separate build of `rustdesk-server`; second tokio-tungstenite major (0.17 vs hbb_common's 0.29); `axum 0.5`, `sqlx 0.6`, `bcrypt`, `jsonwebtoken` | rdog must implement the RustDesk rendezvous protobuf + hbbs framing + ID registry ⇒ re-implement `rendezvous_server.rs` |
| **Cost (runtime)** | — | **3+ processes on the cloud** (hbbs, hbbr, rdog relay) | 3+ processes again, plus rdog must speak RustDesk's session stack |
| **Cost (semantic)** | — | rdog's line-control/reverse-shell protocol must ride inside RustDesk's **remote-desktop session** (KCP + secretbox + their message framing + their client GUI) ⇒ rdog becomes a RustDesk client ⇒ collapses into (a) | hbbs only *signals*; the data path is RustDesk's. To carry rdog's protocol you still need RustDesk's session stack ⇒ collapses into (a) |
| **Conflict with "one binary, rdog only"** | None formally, but drags a GUI app crate (sciter-rs, flutter_rust_bridge, `scrap`) | **Direct, total** — 2 extra daemons, different language ecosystem, different release cadence, different config surface (`.env`, `ini`), different admin model | Same as (b) for the server side |
| **Conflict with "P2P is a HARD requirement for v1"** | n/a | RustDesk's symmetric-NAT give-up means P2P is *not* guaranteed ⇒ same problem rdog is trying to solve | Same |
| **Conflict with "GPL-3.0-only"** | **Yes** — forces AGPL-3.0 (§5(c)) | No (separate process, no linking) — but AGPL obligations still attach to the RustDesk side and to rdog if rdog links hbb_common | No linking ⇒ no AGPL contagion; but rdog's own code must not copy RustDesk source |
| **Verdict** | **Dead** — 4 independent blockers | **Dead** — violates the one-binary constraint outright | **Dead as a shortcut** — it is "implement hbbs", plus RustDesk's session stack |

**Semantic value of (c):** low but not zero. hbbs is a competent rendezvous/ID-registry server with ID→(socket_addr, nat_type) mapping, a relay-allocation table, and public-key registration. If rdog's protocol were **binary-compatible with RustDesk's**, (c) would give rdog a free server. It is not, and making it so would mean re-implementing RustDesk's session semantics (their4-byte frame header, their secretbox handshake, their KCP transport) — at which point rdog has its own server anyway.

**Mapping rdog's `namespace/daemon_name` onto RustDesk's ID space:** RustDesk's rendezvous identity is a **flat, one-ID-per-device** string plus a per-process `serial` (`rendezvous.proto:15-18`), with a separate public-key registration step (`:113-135`) that binds the ID to a key. rdog's `namespace/daemon_name` is **hierarchical**: one namespace fans out to many named daemons, and rdog's daemon identity is not a single opaque string. A namespace could be *encoded* into the flat ID string, but (i) it collides with RustDesk's ID normalization/validation rules, (ii) RustDesk's registry is keyed by ID only, with no hierarchical lookup, and (iii) rdog's own `namespace/daemon_name` lookup semantics (which the brief treats as fixed) would have to be flattened into a single opaque key. **The mapping is lossy and would degrade rdog's addressing model.** *(The exact ID format/normalization rules were not traced — see Unverified.)*

---

## 8. Inheritance-of-traps table

| Reusable part | Trap inherited | Mitigation |
|---|---|---|
| **NAT classification algorithm** (two-destination-port probe) | **Binary-only verdict.** RustDesk has3 `NatType` states; "port-prediction" is impossible with this scheme. A NAT that is *endpoint-independent but port-restricted* is misclassified as ASYMMETRIC; a NAT that is *address-dependent but port-independent* is misclassified as SYMMETRIC. Both misclassifications cause the wrong transport choice. | Adopt the *probe technique* only. Classify into rdog's own richer set (e.g. cone types via a STUN-style server that also reports the observed mapped address), and treat RustDesk's binary verdict as a **lower bound** hint, never as the transport decision. |
| **Symmetric ⇒ force relay** | RustDesk abandons v4 UDP punching entirely when either side is SYMMETRIC (`rendezvous_server.rs:776`, `rendezvous_mediator.rs:1091-1097`). Under rdog's constraint that **P2P is a HARD requirement for v1**, inheriting this is unacceptable — it is exactly the "counterexample" the prior audit flagged. | **Do not inherit.** rdog's design must keep attempting direct (peer-reflexive + port-prediction + longer parallel attempts) and treat relay as the policy-controlled fallback the brief mandates. RustDesk here is a counterexample, not a template. |
| **Lane state machine** (`direct_failures`) | **Sticky degradation.** `rustdesk/src/client.rs:400-410`: the counter is only updated in `connect()`; a *relay* success never resets it to 0, so once `direct_failures == 1` it stays 1 and permanently shortens the direct timeout to `3 × punch_time_used` (`:1453`) instead of `6 ×`. RustDesk also only *raises* the counter on a state transition — there is no decay/TTL. | In rdog, make the direct-failure counter (i) reset on **any** successful session including relay, and (ii) decay with time (half-life), and (iii) never *lower* the direct attempt budget below the baseline. Per the prior audit, **copy this state machine in reverse.** |
| **Direct-connect timeout floor** | `rustdesk/src/client.rs:1431` `const MIN: u64 = 1000;` and the clamp at `:1457-1459`. 1000 ms is at or below the initial TCP SYN RTO (Linux `tcp_syn_retries=6` ⇒ ~127 s total, but the **first** retransmission lands at ~1 s), so a single lost SYN consumes the entire budget and the connection is declared failed before the first retransmit arrives. | Set rdog's floor **above** the SYN RTO — e.g. ≥ 3–5 s for the first attempt, or make the floor a function of the measured RTT. Never reuse RustDesk's 1000 ms. |
| **Relay frame codec** | **1 GiB frame DoS.** `MAX_FRAME_LENGTH = 0x3FFF_FFFF` (~1 GiB) is representable in the 4-byte header (`hbb_common/src/bytes_codec.rs:8`). Upstream *did* fix the speculative-allocation half (`:6` 256 KiB cap, `:63-68` capped reserve) — but `max_packet_length` **defaults to `usize::MAX`** (`:29`) and **`rustdesk-server/src` never calls `set_max_packet_length`** (verified: zero matches). So the hbbr relay path will accept and buffer ~1 GiB of attacker-chosen bytes per connection. | Do not inherit the default. rdog must set an explicit per-lane frame cap at the point of decode (the API is already there: `set_max_packet_length`), *and* cap in-flight buffered bytes per connection, *and* cap total concurrent bytes. Note: the mitigation is available in the library but unused on the relay path — a good illustration of why copying a codec is not the same as inheriting its deployment. |
| **UDP punch / NAT traversal surface** | **Reflection/amplification.** The punch probe (`RDP?`/`RDP!`, 12 bytes, `common.rs:2854`) is sent to a *third party's* observed address; any UDP amplification vector here is a liability for the operator of the punching node. RustDesk's `PUNCH_PROBE` is sent to a peer address learned from the rendezvous server, so a malicious/compromised peer can aim probes at third-party addresses. | Rate-limit and authenticate the probe (the tid + ack pattern already gives a round-trip proof — use it to also bind the session), cap probe rate per peer, and — if rdog's cloud node ever *relays* punch probes — never originate probes toward addresses the peer supplied without an independent address-family/consistency check. |
| **`hbb_common` dependency surface** | 6 git deps (one a **branch**, `sysinfo` `rlim_max`); forked `tokio-tungstenite` rev-pinned; `webrtc` patch that rdog would have to re-declare; OpenSSL via `native-tls`; `tokio`+`full`. | If ever adopted: pin **every** git dep by rev, drop `native-tls`/OpenSSL, and vendor-fork `hbb_common` under rdog's own licence decision. (But the correct answer is not to adopt.) |
| **webrtc fork churn** | `hbb_common/Cargo.toml:70-73` pins webrtc to0.13 with a hand-written rationale, and `:97-99` pins a **hand-maintained fork rev**; `rustdesk/src/client.rs` history shows `bump webrtc fork … (#16263)` on 2026-09-19. The fork is bumped repeatedly and each bump is a coordinated multi-crate change across `rustdesk`, `hbb_common`, and `rustdesk-server`. | Not applicable if webrtc is never enabled. If it were: rdog inherits a3-repo coordinated-bump obligation. This is the concrete instance of the general risk in §9. |

**Where the line should be drawn:** *read the code, don't copy the code* is the right instinct but the wrong threshold — it still leaves AGPL contagion, because §5(c) attaches to the **work as a whole** once any AGPL source is copied in. The correct line is:

> **Reimplement algorithms and wire *designs*; never copy source text.**
>
> Take from RustDesk: the *idea* of a two-destination-port NAT probe; the *shape* of a probe/ack punch handshake with a transaction id and exponential backoff; the *shape* of a length-prefixed frame header. Write rdog's own code for all three. Everything else (the state machine, the timeout policy, the symmetric-NAT give-up, the relay server) is either a counterexample or an operational liability — use it as a cautionary reference, not a template.

---

## 9. Maintenance burden

- **`rustdesk/src/client.rs` churn:** measured **34 commits in the ~13 months since 2025-10-01 ≈ 2.6/month** — consistent with the prior audit's "2–7 commits/month" claim. This file contains the lane state machine, the timeout policy, and the punch orchestration — i.e. **exactly the code rdog would want to track**.
- **Is rdog forced to follow `hbb_common`'s fast-moving interfaces if it depends on it?** **Yes, and worse than the brief suggests:**
  - `hbb_common` is **never versioned** — `version = "0.1.0"` since the beginning, `edition = "2018"`. There is no semver contract, no release cadence, and **no crates.io artifact to pin**. A git dependency must pin a **rev** on `main`, and every bump is a manual rdog-side action.
  - `hbb_common` is a **submodule of two other repos** (`rustdesk`, `rustdesk-server`), both of which pin their own submodule SHA. rdog would be pinning a third independent SHA, with no upstream release to reconcile against.
  - **Six git dependencies** come along, one (`sysinfo`, branch `rlim_max`) pinned to a **branch**, not a rev — i.e. it can move under rdog's lockfile.
  - The **`[patch.crates-io]` trap**: hbb_common's own patch block is inert for consumers (its own comment says so). If rdog enables `webrtc`, rdog must re-declare the webrtc fork patch itself — three coordinated entries (`hbb_common/Cargo.toml:97-99`) — and track every upstream fork bump.
- **Does the webrtc-pin/fork risk exist on `hbb_common`?** **Yes — directly, and it has already materialised.** `hbb_common/Cargo.toml:70-73` pins webrtc to 0.13 with the explicit reason that "webrtc >=0.14 pulls sdp 0.10 / webrtc-util 0.12, which use `usize::is_multiple_of` (needs rustc >=1.87), while rustdesk CI builds with Rust 1.75 … Bump only after CI's Rust moves past 1.87, and work through the upgrade checklist at the top of `src/webrtc.rs` first." That is a **hand-maintained upgrade checklist living inside the dependency** — the clearest possible signal of ongoing fork-drag. The `bump webrtc fork … (#16263)` commit on 2026-09-19 confirms the bump cadence is real, not hypothetical.
  - The mitigations rdog would want are all upstream's problem to make: the patch block, the rev pins, the checklist.
  - **Mitigating nuance:** webrtc is the *optional* part. If rdog only ever wanted `bytes_codec` + crypto, the webrtc risk is avoidable. But then rdog would be depending on `hbb_common` purely to get ~250 lines of codec and a secretbox handshake — while inheriting `tokio`+`full`, OpenSSL, a forked WebSocket stack, and six git repos. **The cost/benefit is decisively negative; write rdog's own 120-line codec.**

---

## 10. Verdict by layer

| Layer | Verdict | Cost | Preconditions |
|---|---|---|---|
| **Protocol reference / precedent** | ✅ **Correct layer.** RustDesk is a strong precedent for topology, relay framing, and the trust model, and a **counterexample** for NAT traversal and lane state management — exactly as the prior audit concluded. | Reading only. | None. |
| **Borrow specific code fragments (verbatim)** | ⚠️ **Available but legally costly.** Verbatim copying of `bytes_codec.rs` etc. ⇒ AGPL-3.0 for rdog via §5(c). | One-line licence change + downstream AGPL obligations. | Switch to `AGPL-3.0-only`; accept the AGPL network clause for every patched self-hosted daemon. |
| **Borrow specific code fragments (clean-room reimplementation)** | ✅ **Recommended.** The two algorithms worth having are small and protocol-agnostic. | ~200–300 lines of new rdog code. | Document the independent derivation; do **not** copy source text. |
| **Crate dependency** | ❌ **Dead.** | — | Would need: `hbb_common` published & licensed (it is neither), punch logic relocated to a lib (it is not), feature-gated to `bytes_codec`+crypto only (it is not), AGPL accepted (optional but moot). |
| **Whole-process integration** | ❌ **Dead.** | 3+ processes, different ecosystem. | Would need to abandon the one-binary constraint. |

**Does the license gate alone end the discussion?** **No — but it ends one option and confirms the others are unnecessary.** The license gate is *survivable* (switch to AGPL-3.0-only). What actually ends the "depend on RustDesk" family is the conjunction of **(i) the NAT-traversal code living in an application crate that is not a publishable library, (ii) nothing being published to crates.io, (iii) `hbb_common` carrying no license at all, and (iv) the dependency tree being unfeature-gateable to the useful subset.** The license gate is the *second* independent reason, and the one that most affects rdog's downstream users.

---

## 11. Unverified / open questions

1. **Cargo's behaviour with an inert `[patch.crates-io]`.** `hbb_common/Cargo.toml:96-99` declares webrtc/webrtc-util/webrtc-sctp patches. Whether Cargo resolves (and therefore git-fetches) those entries when the patched crates are **absent** from rdog's dependency graph — i.e. when rdog does not enable the `webrtc` feature — was **not empirically tested**. This determines whether a non-webrtc `hbb_common` dep still forces a fetch of `rustdesk-org/webrtc`.
2. **RustDesk's exact ID string format and normalization.** `rendezvous.proto:15-18` shows `RegisterPeer{ id: string, serial: int32 }`, but the **ID format rules** (numeric length, custom-ID handling, normalization on the server) were not traced into `hbb_common/src/config.rs` / `rustdesk-server/src/rendezvous_server.rs`. The namespace→ID mapping conclusion in §7 is therefore directional, not exact.
3. **Whether `rustdesk-server`'s `hbbs` lib is practically linkable.** `src/lib.rs` exposes `rendezvous_server`, but the full dependency cost (axum 0.5, sqlx 0.6, bcrypt, jsonwebtoken, a *second* tokio-tungstenite major) and the `hbb_common` submodule path requirement were not build-tested.
4. **`hbb_common`'s effective MSRV.** No `rust-version` key is declared. Its own `Cargo.toml` comment mentions rustc 1.75 and `usize::is_multiple_of` needing ≥1.87 — but the **actual** minimum compiler that builds `hbb_common` standalone was not established by compilation.
5. **Whether `hbb_common`'s git deps still resolve and build today.** The six git dependencies (one branch-pinned) were not fetched or compiled; `rustdesk-org/confy`, `tokio-socks`, `sysinfo@rlim_max`, `default_net`, `machine-uid`, and the `tokio-tungstenite` fork rev `c241b11b` could have moved or broken.
6. **The "1 GiB frame DoS" on the relay path.** The *absence* of `set_max_packet_length` in `rustdesk-server/src` is verified by exhaustive grep. Whether hbbr is in fact reachable with a ~1 GiB frame in practice (e.g. whether an outer layer caps it) was **not** confirmed by reading the full relay read loop.
7. **The `direct_failures` "never reset by relay sessions" claim.** Verified as a *code-path* property at `rustdesk/src/client.rs:400-410`. Whether some other code path resets it (e.g. a config write at `:3171`) was not exhaustively traced.
8. **The "1000 ms is below a 1 s SYN RTO" claim.** The 1000 ms floor is verified in RustDesk's source. The comparison to the platform SYN RTO is an **inference from default TCP behaviour**, not measured on any target platform, and the applicable default differs by OS.
9. **`rustdesk`'s `[lib] librustdesk` rlib** — technically a linkable artifact. Its true dependency closure (sciter-rs, flutter_rust_bridge, `scrap`, enigo, clipboard, virtual_display, remote_printer) was read from the manifest but **not built or measured**.
10. **Legal opinion.** This is an engineering analysis of license *text* and *facts*, not legal advice. In particular: whether a rdog author who merely *read* RustDesk source and wrote independent code creates a derivative work, and whether AGPL §13's compatibility clause would be applied by a court as read here, are matters for counsel. The `hbb_common`-has-no-license finding is a **fact about the repository**, not a legal conclusion that use is impossible.

---

### Suggested next step

Close route (a) in the ticket with the four blockers above and pivot the design work to **(c′) "rdog's own rendezvous, designed from RustDesk's *lessons*"**: keep RustDesk in the repository as a **read-only reference document** (a `docs/solutions/` entry, not a dependency) that records, with file:line citations, (i) the two-destination-port NAT probe as a *technique to reimplement*, (ii) the probe/ack punch handshake as a *design to reimplement*, (iii) the symmetric-NAT give-up and sticky `direct_failures` and 1000 ms floor as **counterexamples to invert**, and (iv) the 1 GiB frame cap as a **requirement to impose explicitly**. Then design rdog's own lane state machine and frame codec with the traps designed out from the start, and evaluate `iroh` (MIT OR Apache-2.0) separately as the actual P2P candidate — noting that iroh 1.4.1 requires rustc **1.91**, above rdog's declared MSRV of **1.89** (iroh 0.98.2 is `1.89`), so an MSRV decision is needed before that comparison can be run.
