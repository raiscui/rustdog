//! bulk 数据面在 iroh **准入 lane** 上的回环用例 (RAI-476 步骤 1 / 步骤 2)。
//!
//! 步骤 1: 同一连接承载 `@put-file` → `@file-chunk` → `@file-end` 与 `@get-file` →
//! `@file-read`, 以及 `@file-list` / `@file-cancel`。
//! 步骤 2: open 与每个 chunk 边界读路径, 只放行 `direct`。**这是逐 chunk 检查, 不是
//! 传输层 pin** —— 边界之间的排队与重传它看不到 (`specs/rdog-bulk-direct-only-spec.md` §3.5)。
//!
//! 回环拓扑只会给出 `direct`, 所以 relay / 读不出 / 先 direct 后掉 relay 三种读数由
//! `PathSampler::Script` 钉住喂进门禁 —— 判定表与拒绝码仍是生产那一份, 测试不另抄一份。

use std::{
    collections::BTreeSet,
    fs,
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    sync::{Arc, Mutex},
    thread::{self, JoinHandle},
    time::Duration,
};

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use iroh::{EndpointId, SecretKey};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::{admission, BulkPathGate, BulkStep, PathSampler, RelayReading};
use crate::{
    bulk_path::{
        BulkDecision, PathClass, IROH_DIRECT_PATH_LOST, IROH_DIRECT_PATH_UNAVAILABLE,
        IROH_PATH_UNKNOWN,
    },
    control_transport::ControlTransport,
};

struct TempRoot(PathBuf);

impl TempRoot {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "rdog-iroh-bulk-{tag}-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir(&path).expect("create isolated test dir");
        Self(path)
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// 一条 lane 的客户端那一端, 外加必须活着的测试目录与服务器线程。
struct Lane {
    stream: TcpStream,
    reader: BufReader<TcpStream>,
    server: Option<JoinHandle<std::io::Result<()>>>,
    roots: Vec<TempRoot>,
}

impl Lane {
    fn new(stream: TcpStream, server: JoinHandle<std::io::Result<()>>) -> Self {
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .expect("set lane read timeout");
        let reader = BufReader::new(stream.try_clone().expect("clone lane stream"));
        Self {
            stream,
            reader,
            server: Some(server),
            roots: Vec::new(),
        }
    }

    fn send(&mut self, line: &str) {
        writeln!(self.stream, "{line}").expect("write lane line");
        self.stream.flush().expect("flush lane line");
    }

    /// 读一条响应 (握手回执或 `@response ...`), 解析成 JSON。
    fn reply(&mut self) -> Value {
        let mut line = String::new();
        let read = self.reader.read_line(&mut line).expect("read lane line");
        assert!(read > 0, "lane closed before replying");
        let body = line.trim_end();
        let body = body.strip_prefix("@response ").unwrap_or(body);
        serde_json::from_str(body).unwrap_or_else(|err| panic!("non-JSON reply {body:?}: {err}"))
    }
}

impl Drop for Lane {
    fn drop(&mut self) {
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
        if let Some(server) = self.server.take() {
            let outcome = server.join().expect("lane server thread panicked");
            // 客户端先断之后, 服务端收尾的 `shutdown` 在 macOS/Windows 上会回 ENOTCONN
            // ("Socket is not connected")。这是 TCP 替身的收尾产物, 不是 lane 的结论 ——
            // 真正的 iroh 流不会走到这条路径。除它以外的错误照旧算失败。
            let teardown_only = matches!(&outcome, Err(err) if err.kind() == std::io::ErrorKind::NotConnected);
            assert!(
                outcome.is_ok() || teardown_only,
                "lane server failed: {outcome:?}"
            );
        }
    }
}

fn pair() -> (TcpStream, TcpStream) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind lane fixture");
    let client =
        TcpStream::connect(listener.local_addr().expect("lane addr")).expect("connect lane fixture");
    let (server, _) = listener.accept().expect("accept lane fixture");
    (server, client)
}

/// 生产形态: daemon profile 给了 `policy_file`, 走 `serve_admitted`。bulk 只在这条 lane 上。
///
/// `relay` 钉住括号两端各取一次的 relay 读数 (空 = 一次都读不到, 门禁之外的证据一律 `null`)。
fn start_admitted_with_relay(
    readings: Vec<PathClass>,
    relay: Vec<Option<RelayReading>>,
) -> Lane {
    let root = TempRoot::new("admitted");
    let target = SecretKey::generate();
    let client = SecretKey::generate();
    let policy = root.0.join("policy.json");
    // 用 `save_policy_with` 传入空目录同步: `save_policy` 会 fsync 父目录, 而 Windows 打不开
    // 目录句柄 (`DurabilityUnconfirmed(PermissionDenied)`)。夹具只需要"策略已落盘"这一件事。
    admission::save_policy_with(
        &policy,
        &admission::Policy {
            allow: BTreeSet::from([client.public()]),
            ..admission::Policy::default()
        },
        |_| Ok(()),
    )
    .expect("write admission policy");

    let state = admission::Admission::new(policy, target.public());
    let session = uuid::Uuid::new_v4().to_string();
    // 客户端自签、daemon 校验: `Admission::validate` 要求 `peer == claims.issuer`。
    let grant = admission::SignedGrant::issue(
        &client,
        target.public(),
        &session,
        super::utc_ms(),
        Duration::from_secs(60),
    )
    .expect("issue grant");
    // 不让 fixture 先 admit: `serve_admitted` 收到握手后才落租约, 预置会让它按"租约已存在"拒。
    let state = Arc::new(Mutex::new(state));

    let handshake = serde_json::to_string(&super::Handshake::Admit { grant }).expect("handshake");
    let peer: EndpointId = client.public();
    let (server, client_stream) = pair();
    let handle = thread::spawn(move || {
        let transport = ControlTransport::from_tcp_stream(server)?;
        let sampler = PathSampler::script_with_relay(readings, relay);
        let mut gate = BulkPathGate::new(super::ADMITTED_LANE_PATH_CAPABILITY);
        super::serve_admitted(transport, peer, "test", state, &sampler, &mut gate)
    });
    let mut lane = Lane::new(client_stream, handle);
    lane.roots.push(root);
    lane.send(&handshake);
    assert_eq!(lane.reply()["status"], "admitted");
    lane
}

fn start_admitted(readings: Vec<PathClass>) -> Lane {
    start_admitted_with_relay(readings, Vec::new())
}

fn escape_json(input: &str) -> String {
    input.replace('\\', "\\\\").replace('"', "\\\"")
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn transfer_id(tag: &str) -> String {
    format!("{tag}-{}", uuid::Uuid::new_v4())
}

fn put_file(lane: &mut Lane, id: &str, target: &PathBuf, size: usize, sha: &str) {
    lane.send(&format!(
        r#"@put-file:{{"transfer_id":"{id}","path":"{}","size":{size},"sha256":"{sha}","resume":false}}"#,
        escape_json(&target.display().to_string())
    ));
    assert_eq!(lane.reply()["state"], "open");
}

fn put_chunk(lane: &mut Lane, id: &str, offset: usize, bytes: &[u8]) {
    let data = BASE64_STANDARD.encode(bytes);
    lane.send(&format!(
        r#"@file-chunk:{{"transfer_id":"{id}","offset":{offset},"data":"{data}"}}"#
    ));
    let reply = lane.reply();
    assert_eq!(reply["written"], (offset + bytes.len()) as u64);
}

fn read_chunk(lane: &mut Lane, id: &str, offset: usize, length: usize) -> (Vec<u8>, bool) {
    lane.send(&format!(
        r#"@file-read:{{"transfer_id":"{id}","offset":{offset},"length":{length}}}"#
    ));
    let reply = lane.reply();
    let bytes = BASE64_STANDARD
        .decode(reply["data"].as_str().expect("file-read data"))
        .expect("file-read data is base64");
    (bytes, reply["eof"].as_bool().expect("file-read eof"))
}

#[test]
fn upload_commits_over_one_lane_connection() {
    // 三个 chunk 串在一条连接上: 每个 chunk 是一条独立请求行, 不需要新流。
    let dir = TempRoot::new("upload");
    let target = dir.0.join("landed.bin");
    let payload: Vec<u8> = (0..=200u8).collect();
    let sha = sha256_hex(&payload);
    let id = transfer_id("up");
    let mut lane = start_admitted(vec![PathClass::Direct]);

    put_file(&mut lane, &id, &target, payload.len(), &sha);
    put_chunk(&mut lane, &id, 0, &payload[..64]);
    put_chunk(&mut lane, &id, 64, &payload[64..160]);
    put_chunk(&mut lane, &id, 160, &payload[160..]);

    lane.send(&format!(
        r#"@file-end:{{"transfer_id":"{id}","sha256":"{sha}"}}"#
    ));
    let end = lane.reply();
    assert_eq!(end["state"], "committed");
    assert_eq!(end["sha256"], sha);
    assert_eq!(fs::read(&target).expect("read committed file"), payload);
}

#[test]
fn download_reads_the_same_bytes_back() {
    let dir = TempRoot::new("download");
    let source = dir.0.join("source.bin");
    let payload: Vec<u8> = (0..=180u8).map(|byte| byte ^ 0x5a).collect();
    fs::write(&source, &payload).expect("write source file");
    let sha = sha256_hex(&payload);
    let id = transfer_id("down");
    let mut lane = start_admitted(vec![PathClass::Direct]);

    lane.send(&format!(
        r#"@get-file:{{"transfer_id":"{id}","path":"{}"}}"#,
        escape_json(&source.display().to_string())
    ));
    let open = lane.reply();
    assert_eq!(open["state"], "reading");
    assert_eq!(open["size"], payload.len() as u64);
    assert_eq!(open["sha256"], sha);

    // 两次读取, 第二次跨到 EOF: 每个 `@file-read` 也是一次独立的 chunk 边界。
    let (first, eof) = read_chunk(&mut lane, &id, 0, 100);
    assert!(!eof);
    let (second, eof) = read_chunk(&mut lane, &id, 100, 200);
    assert!(eof, "读到 EOF 才算读完");

    let mut reassembled = first;
    reassembled.extend_from_slice(&second);
    assert_eq!(reassembled, payload, "读回的字节必须与源文件一致");
}

#[test]
fn file_list_shows_the_open_transfer_and_cancel_is_idempotent() {
    let dir = TempRoot::new("list");
    let target = dir.0.join("pending.bin");
    let id = transfer_id("list");
    let mut lane = start_admitted(vec![PathClass::Direct]);

    put_file(&mut lane, &id, &target, 16, &sha256_hex(&[0u8; 16]));
    put_chunk(&mut lane, &id, 0, &[7u8; 16]);

    lane.send("@file-list");
    let list = lane.reply();
    assert_eq!(list["state"], "listed");
    let listed = list["transfers"]
        .as_array()
        .expect("transfers array")
        .iter()
        .any(|entry| entry["transfer_id"] == id.as_str());
    assert!(listed, "活跃传输必须能被 @file-list 直接读到");

    lane.send(&format!(r#"@file-cancel:{{"transfer_id":"{id}"}}"#));
    let cancelled = lane.reply();
    assert_eq!(cancelled["state"], "cancelled");
    assert_eq!(cancelled["partial_removed"], true);

    // 重复取消是正常调用顺序, 不是错误。
    lane.send(&format!(r#"@file-cancel:{{"transfer_id":"{id}"}}"#));
    assert_eq!(lane.reply()["state"], "absent");

    lane.send("@file-list");
    let list = lane.reply();
    let still_listed = list["transfers"]
        .as_array()
        .expect("transfers array")
        .iter()
        .any(|entry| entry["transfer_id"] == id.as_str());
    assert!(!still_listed, "取消后注册表不得留残留");
    assert!(!target.exists(), "取消不得碰目标路径上不存在的文件");
}

#[test]
fn the_gate_reads_the_declared_capability_table() {
    use PathClass::{Direct, Relay, Unknown};

    let gate = || BulkPathGate::new(super::ADMITTED_LANE_PATH_CAPABILITY);
    // direct 放行
    assert_eq!(
        gate().admit(BulkStep::Open, Some("a"), Direct),
        BulkDecision::Allowed
    );
    // 从来没有直连: relay 拒, 码说"直连不可得"
    assert_eq!(
        gate().admit(BulkStep::Open, Some("b"), Relay),
        BulkDecision::Refused(IROH_DIRECT_PATH_UNAVAILABLE)
    );
    // 读不出路径: fail-closed
    assert_eq!(
        gate().admit(BulkStep::Open, Some("c"), Unknown),
        BulkDecision::Refused(IROH_PATH_UNKNOWN)
    );
    // 先 direct 后掉 relay: "曾直连" 置位, 码说"直连丢了"
    let mut stepwise = gate();
    assert_eq!(
        stepwise.admit(BulkStep::Open, Some("d"), Direct),
        BulkDecision::Allowed
    );
    assert_eq!(
        stepwise.admit(BulkStep::Continue, Some("d"), Relay),
        BulkDecision::Refused(IROH_DIRECT_PATH_LOST)
    );
}

#[test]
fn this_lane_checks_every_chunk_but_does_not_pin_the_transport() {
    // 裁定 A 的核心: 逐 chunk 检查 ≠ 传输层 pin。把两者写成同一个值, 就等于用"每个边界
    // 都直连"顶替"边界之间不会改路" —— §3.5 明说应用层采样不是充分条件。
    let declared = super::ADMITTED_LANE_PATH_CAPABILITY;
    assert!(declared.path_aware);
    assert!(declared.bulk_path_checked_per_chunk);
    assert!(!declared.bulk_path_pin, "采样不是 pin, 本版没做传输层 pin");
    // 既不钉也不查的传输仍按能力拒绝, 而不是按路径类别放行
    for class in [PathClass::Direct, PathClass::Relay, PathClass::Unknown] {
        assert_eq!(
            crate::bulk_path::decide(
                crate::bulk_path::PathCapability::PATH_AWARE_UNPINNED,
                class,
                false
            ),
            BulkDecision::Refused(crate::bulk_path::IROH_BULK_PATH_PIN_UNAVAILABLE)
        );
    }
}

#[test]
fn an_idempotent_open_starts_a_fresh_attempt() {
    // C: open 无条件清零。同 id 的幂等重试是**新的一次尝试** —— 继承上一次的"曾直连"
    // 会把 `unavailable` 误判成 `lost`, 而两个码对调用方的意思完全不同。
    let mut gate = BulkPathGate::new(super::ADMITTED_LANE_PATH_CAPABILITY);
    assert_eq!(gate.admit(BulkStep::Open, Some("t"), PathClass::Direct), BulkDecision::Allowed);
    assert_eq!(
        gate.admit(BulkStep::Continue, Some("t"), PathClass::Direct),
        BulkDecision::Allowed
    );
    // 同 id 重开、这次一开始就是 relay: 必须是"直连不可得", 不是"直连丢了"
    assert_eq!(
        gate.admit(BulkStep::Open, Some("t"), PathClass::Relay),
        BulkDecision::Refused(IROH_DIRECT_PATH_UNAVAILABLE)
    );
}

#[test]
fn interleaved_transfers_keep_their_own_direct_history() {
    // D: 同一条连接上交错两条传输。单槽会把 A 的"曾直连"被 B 的 open 清掉, 于是 A 中途
    // 掉 relay 会被误判成 `unavailable`; 按键记录才不会串。
    let mut gate = BulkPathGate::new(super::ADMITTED_LANE_PATH_CAPABILITY);
    assert_eq!(gate.admit(BulkStep::Open, Some("a"), PathClass::Direct), BulkDecision::Allowed);
    assert_eq!(gate.admit(BulkStep::Open, Some("b"), PathClass::Direct), BulkDecision::Allowed);
    assert_eq!(
        gate.admit(BulkStep::Continue, Some("a"), PathClass::Relay),
        BulkDecision::Refused(IROH_DIRECT_PATH_LOST),
        "A 的历史属于 A"
    );
    assert_eq!(
        gate.admit(BulkStep::Continue, Some("b"), PathClass::Direct),
        BulkDecision::Allowed,
        "A 的失败不得牵连 B"
    );
}

#[test]
fn a_relay_lane_refuses_bulk_before_any_disk_byte() {
    // B: 名字刻意不写 "before any byte" —— 上传 chunk 的载荷内联在请求行里, daemon 读到
    // `transfer_id` 之前那一 chunk 的字节已经过线。这里能证明的只是**盘上没有字节、也没有
    // 会话**: 目标文件与半成品都不存在。
    let dir = TempRoot::new("gate-relay");
    let target = dir.0.join("never.bin");
    let id = transfer_id("relay");
    let mut lane = start_admitted(vec![PathClass::Relay]);

    lane.send(&format!(
        r#"@put-file:{{"transfer_id":"{id}","path":"{}","size":4,"resume":false}}"#,
        escape_json(&target.display().to_string())
    ));
    let refused = lane.reply();
    assert_eq!(refused["error"], IROH_DIRECT_PATH_UNAVAILABLE);
    assert_eq!(refused["code"], super::LANE_REFUSED_CODE);
    assert!(!target.exists(), "被拒的 open 不得留下目标文件");

    // 会话也没建: `@file-list` 里看不到这条 id。
    lane.send("@file-list");
    let list = lane.reply();
    let listed = list["transfers"]
        .as_array()
        .expect("transfers array")
        .iter()
        .any(|entry| entry["transfer_id"] == id.as_str());
    assert!(!listed, "被门禁拒掉的尝试不得建会话");
}

#[test]
fn an_unreadable_path_is_refused_not_treated_as_direct() {
    let dir = TempRoot::new("gate-unknown");
    let target = dir.0.join("never.bin");
    let id = transfer_id("unknown");
    let mut lane = start_admitted(vec![PathClass::Unknown]);

    lane.send(&format!(
        r#"@put-file:{{"transfer_id":"{id}","path":"{}","size":4,"resume":false}}"#,
        escape_json(&target.display().to_string())
    ));
    assert_eq!(lane.reply()["error"], IROH_PATH_UNKNOWN);
    assert!(!target.exists());
}

#[test]
fn losing_the_direct_path_mid_transfer_refuses_the_next_chunk() {
    let dir = TempRoot::new("gate-lost");
    let target = dir.0.join("partial.bin");
    let payload = b"half the bytes";
    let id = transfer_id("lost");
    // open 与第一个 chunk 边界都是 direct, 第二个 chunk 边界掉成 relay。
    let mut lane = start_admitted(vec![PathClass::Direct, PathClass::Direct, PathClass::Relay]);

    put_file(&mut lane, &id, &target, payload.len(), &sha256_hex(payload));
    put_chunk(&mut lane, &id, 0, &payload[..4]);

    lane.send(&format!(
        r#"@file-chunk:{{"transfer_id":"{id}","offset":4,"data":"{}"}}"#,
        BASE64_STANDARD.encode(&payload[4..])
    ));
    let refused = lane.reply();
    assert_eq!(refused["error"], IROH_DIRECT_PATH_LOST);
    assert!(
        !target.exists(),
        "被拒的 chunk 之后目标文件不得出现 (半成品保留以便续传)"
    );
}

#[test]
fn cancel_and_list_stay_out_of_the_path_gate() {
    // 这两个动词不搬字节: relay 路径上也要能取消/查表, 否则半成品就没人能收尾。
    let dir = TempRoot::new("gate-list");
    let target = dir.0.join("pending.bin");
    let id = transfer_id("gate-list");
    let mut lane = start_admitted(vec![PathClass::Direct, PathClass::Direct, PathClass::Relay]);

    put_file(&mut lane, &id, &target, 8, &sha256_hex(&[0u8; 8]));
    put_chunk(&mut lane, &id, 0, &[1u8; 8]);

    lane.send("@file-list");
    assert_eq!(lane.reply()["state"], "listed");
    lane.send(&format!(r#"@file-cancel:{{"transfer_id":"{id}"}}"#));
    assert_eq!(lane.reply()["state"], "cancelled");
}

// ---------------------------------------------------------------------------
// 步骤 3: daemon 侧 relay 字节括号采样, 以及半快照落在哪条响应里 (§4.2/§4.3)
// ---------------------------------------------------------------------------

/// `@file-end` 的收据是上传方向的终态: daemon 半快照必须挂在这里, 字段形状与
/// `BulkPathSnapshot::to_json()` 同 schema, `endpoint:"daemon"`。
#[test]
fn upload_delivers_the_daemon_half_on_the_file_end_receipt() {
    let dir = TempRoot::new("evidence-put");
    let target = dir.0.join("landed.bin");
    let payload = b"bracket me";
    let sha = sha256_hex(payload);
    let id = transfer_id("ev-put");
    // 门槛: open、chunk、end 三次采样都是 direct; 括号两端各一次 relay 读数。
    let mut lane = start_admitted_with_relay(
        vec![PathClass::Direct],
        vec![Some(RelayReading::at(100, 50)), Some(RelayReading::at(100, 50))],
    );

    put_file(&mut lane, &id, &target, payload.len(), &sha);
    put_chunk(&mut lane, &id, 0, payload);
    lane.send(&format!(r#"@file-end:{{"transfer_id":"{id}","sha256":"{sha}"}}"#));
    let end = lane.reply();
    assert_eq!(end["state"], "committed");

    let evidence = &end["evidence"];
    assert_eq!(evidence["schema"], "rdog.bulk-path.v1");
    assert_eq!(evidence["endpoint"], "daemon");
    assert_eq!(evidence["transfer_id"], id.as_str());
    // 括号: before 与 after 都是 (100, 50), 所以两个 delta 都是 0 —— 那是**测到**的零。
    assert_eq!(evidence["relay_tx_before"], 100);
    assert_eq!(evidence["relay_tx_after"], 100);
    assert_eq!(evidence["relay_sample_available"], true);
    assert_eq!(evidence["verdict"], "relay_bytes_zero");
    // 载荷字节与压上控制行的 base64 字节是两件事, 两个都要有。
    assert_eq!(evidence["bulk_bytes"], payload.len() as u64);
    assert!(
        evidence["wire_bytes"].as_u64().expect("wire_bytes") > payload.len() as u64,
        "base64 编码后的字节数必须大于载荷字节数"
    );
    assert_eq!(evidence["chunks"], 1);
    assert_eq!(fs::read(&target).expect("read committed file"), payload);
}

#[test]
fn bracket_sampling_reports_the_delta_of_relay_bytes() {
    // 括号两端不同: delta 必须反映这段区间里 relay path 上真的走过的字节。
    let dir = TempRoot::new("evidence-delta");
    let target = dir.0.join("landed.bin");
    let payload = b"delta";
    let sha = sha256_hex(payload);
    let id = transfer_id("ev-delta");
    let mut lane = start_admitted_with_relay(
        vec![PathClass::Direct],
        vec![Some(RelayReading::at(100, 50)), Some(RelayReading::at(160, 90))],
    );

    put_file(&mut lane, &id, &target, payload.len(), &sha);
    put_chunk(&mut lane, &id, 0, payload);
    lane.send(&format!(r#"@file-end:{{"transfer_id":"{id}","sha256":"{sha}"}}"#));
    let evidence = lane.reply()["evidence"].clone();

    assert_eq!(evidence["relay_tx_before"], 100);
    assert_eq!(evidence["relay_tx_after"], 160);
    assert_eq!(evidence["relay_rx_after"], 90);
    assert_eq!(evidence["relay_sample_available"], true);
    // 60 / 40 的增量就是"本次尝试归属的 relay 字节": 它不为零, 判定就不能是 relay_bytes_zero。
    assert_eq!(evidence["verdict"], "relay_bytes_nonzero");
    assert_eq!(evidence["path_class"], "direct");
}

#[test]
fn unsampled_relay_counters_land_as_null_not_zero() {
    // #158 的语义在 lane 上也要成立: 读不到 = 四个 null + relay_sample_available:false,
    // 且 verdict 是 relay_sample_unavailable —— 不是 0。
    let dir = TempRoot::new("evidence-unsampled");
    let target = dir.0.join("landed.bin");
    let payload = b"nobody watched";
    let sha = sha256_hex(payload);
    let id = transfer_id("ev-none");
    let mut lane = start_admitted_with_relay(vec![PathClass::Direct], vec![None]);

    put_file(&mut lane, &id, &target, payload.len(), &sha);
    put_chunk(&mut lane, &id, 0, payload);
    lane.send(&format!(r#"@file-end:{{"transfer_id":"{id}","sha256":"{sha}"}}"#));
    let receipt = lane.reply();
    assert_eq!(receipt["state"], "committed");
    let evidence = &receipt["evidence"];

    for key in [
        "relay_tx_before",
        "relay_tx_after",
        "relay_rx_before",
        "relay_rx_after",
    ] {
        assert_eq!(evidence[key], Value::Null, "{key} 未采样必须是 null");
    }
    assert_eq!(evidence["relay_sample_available"], false);
    assert_eq!(evidence["verdict"], "relay_sample_unavailable");
    assert_eq!(evidence["bulk_bytes"], payload.len() as u64, "字节数照记");
}

#[test]
fn a_relay_path_that_closed_inside_the_bracket_is_not_a_zero() {
    // `before` 时在场的 relay path 在 `after` 没了: 它的关闭字节已经不在快照里, 相减会得到
    // 一个假的零。这种情况整份读数作废 (null), 不是拿剩下的 path 凑一个数。
    let dir = TempRoot::new("evidence-closed");
    let target = dir.0.join("landed.bin");
    let payload = b"closed mid-flight";
    let sha = sha256_hex(payload);
    let id = transfer_id("ev-closed");
    let before = RelayReading {
        tx: 10,
        rx: 0,
        relay_paths: vec!["PathId(7)@relay:https://relay.invalid".to_owned()],
    };
    let mut lane = start_admitted_with_relay(
        vec![PathClass::Direct],
        vec![Some(before), Some(RelayReading::at(10, 0))],
    );

    put_file(&mut lane, &id, &target, payload.len(), &sha);
    put_chunk(&mut lane, &id, 0, payload);
    lane.send(&format!(r#"@file-end:{{"transfer_id":"{id}","sha256":"{sha}"}}"#));
    let evidence = lane.reply()["evidence"].clone();
    assert_eq!(evidence["relay_tx_before"], Value::Null);
    assert_eq!(evidence["relay_sample_available"], false);
    assert_eq!(evidence["verdict"], "relay_sample_unavailable");
}

#[test]
fn download_delivers_the_daemon_half_on_the_eof_frame() {
    // 下载方向的终态是 `eof:true` 的那一帧 `@file-read` (§4.3 写清的那一条)。
    let dir = TempRoot::new("evidence-get");
    let source = dir.0.join("source.bin");
    let payload: Vec<u8> = (0..=120u8).collect();
    fs::write(&source, &payload).expect("write source file");
    let id = transfer_id("ev-get");
    let mut lane = start_admitted_with_relay(
        vec![PathClass::Direct],
        vec![Some(RelayReading::at(0, 0)), Some(RelayReading::at(0, 0))],
    );

    lane.send(&format!(
        r#"@get-file:{{"transfer_id":"{id}","path":"{}"}}"#,
        escape_json(&source.display().to_string())
    ));
    assert_eq!(lane.reply()["state"], "reading");

    lane.send(&format!(
        r#"@file-read:{{"transfer_id":"{id}","offset":0,"length":4096}}"#
    ));
    let frame = lane.reply();
    assert_eq!(frame["eof"], true);
    let evidence = &frame["evidence"];
    assert_eq!(evidence["endpoint"], "daemon");
    assert_eq!(evidence["direction"], "get");
    assert_eq!(evidence["bulk_bytes"], payload.len() as u64);
    assert_eq!(evidence["verdict"], "relay_bytes_zero");
}

#[test]
fn a_gate_refusal_still_carries_its_own_evidence() {
    // 被门禁拒掉的尝试也要有证据: 它不是结论 (aborted), 但"那一刻读到的路径"正是要留下的东西。
    let dir = TempRoot::new("evidence-refused");
    let target = dir.0.join("never.bin");
    let id = transfer_id("ev-refused");
    let mut lane = start_admitted_with_relay(
        vec![PathClass::Relay],
        vec![Some(RelayReading::at(7, 0))],
    );

    lane.send(&format!(
        r#"@put-file:{{"transfer_id":"{id}","path":"{}","size":4,"resume":false}}"#,
        escape_json(&target.display().to_string())
    ));
    let refused = lane.reply();
    assert_eq!(refused["error"], IROH_DIRECT_PATH_UNAVAILABLE);
    let evidence = &refused["evidence"];
    assert_eq!(evidence["aborted"], true);
    assert_eq!(evidence["verdict"], "aborted");
    assert_eq!(evidence["path_class"], "relay");
    assert_eq!(evidence["chunks"], 0);
    assert!(!target.exists());
}

#[test]
fn an_empty_transfer_is_not_evidence_of_a_clean_path() {
    // 0 字节的下载: 读数再干净也不构成"bulk 没走 relay"的证据 (§4.3 第 2 条)。
    let dir = TempRoot::new("evidence-empty");
    let source = dir.0.join("empty.bin");
    fs::write(&source, []).expect("write empty source file");
    let id = transfer_id("ev-empty");
    let mut lane = start_admitted_with_relay(
        vec![PathClass::Direct],
        vec![Some(RelayReading::at(0, 0)), Some(RelayReading::at(0, 0))],
    );

    lane.send(&format!(
        r#"@get-file:{{"transfer_id":"{id}","path":"{}"}}"#,
        escape_json(&source.display().to_string())
    ));
    assert_eq!(lane.reply()["state"], "reading");
    lane.send(&format!(
        r#"@file-read:{{"transfer_id":"{id}","offset":0,"length":4096}}"#
    ));
    let frame = lane.reply();
    assert_eq!(frame["eof"], true);
    let evidence = &frame["evidence"];
    assert_eq!(evidence["chunks"], 0);
    assert_eq!(evidence["bulk_bytes"], 0);
    assert_eq!(evidence["verdict"], "no_bulk_bytes");
}

#[test]
fn a_relay_path_rebuilt_inside_the_bracket_is_not_a_zero() {
    // relay path 关闭后以同一 RelayUrl 重建, 新 PathId、计数从 0 起 —— 这是 iroh 的正常事件。
    // 只按地址比对的在场集合会认为"同一条 path 还在", 于是 1000 → 100 相减得到一个漂亮的零,
    // verdict 就成了"测得零"。键必须带 PathId, 而且相减必须 checked。
    let dir = TempRoot::new("evidence-rebuilt");
    let target = dir.0.join("landed.bin");
    let payload = b"rebuilt path";
    let sha = sha256_hex(payload);
    let id = transfer_id("ev-rebuilt");
    let before = RelayReading {
        tx: 1000,
        rx: 0,
        relay_paths: vec!["PathId(1)@relay:https://relay.example/".to_owned()],
    };
    let after = RelayReading {
        tx: 100,
        rx: 0,
        relay_paths: vec!["PathId(2)@relay:https://relay.example/".to_owned()],
    };
    let mut lane = start_admitted_with_relay(vec![PathClass::Direct], vec![Some(before), Some(after)]);

    put_file(&mut lane, &id, &target, payload.len(), &sha);
    put_chunk(&mut lane, &id, 0, payload);
    lane.send(&format!(r#"@file-end:{{"transfer_id":"{id}","sha256":"{sha}"}}"#));
    let evidence = lane.reply()["evidence"].clone();
    assert_eq!(evidence["relay_tx_before"], Value::Null);
    assert_eq!(evidence["relay_tx_after"], Value::Null);
    assert_eq!(evidence["relay_sample_available"], false);
    assert_eq!(evidence["verdict"], "relay_sample_unavailable");
}

#[test]
fn a_counter_that_went_backwards_on_the_same_path_is_not_a_zero() {
    // 同一条 path 的计数倒退 (不该发生, 但真发生了) 也必须是"没结论", 不是"零"。
    let dir = TempRoot::new("evidence-backwards");
    let target = dir.0.join("landed.bin");
    let payload = b"backwards";
    let sha = sha256_hex(payload);
    let id = transfer_id("ev-backwards");
    let key = "PathId(3)@relay:https://relay.example/".to_owned();
    let before = RelayReading { tx: 500, rx: 0, relay_paths: vec![key.clone()] };
    let after = RelayReading { tx: 20, rx: 0, relay_paths: vec![key] };
    let mut lane = start_admitted_with_relay(vec![PathClass::Direct], vec![Some(before), Some(after)]);

    put_file(&mut lane, &id, &target, payload.len(), &sha);
    put_chunk(&mut lane, &id, 0, payload);
    lane.send(&format!(r#"@file-end:{{"transfer_id":"{id}","sha256":"{sha}"}}"#));
    let evidence = lane.reply()["evidence"].clone();
    assert_eq!(evidence["relay_tx_before"], 500);
    assert_eq!(evidence["relay_tx_after"], 20);
    assert_eq!(evidence["relay_sample_available"], false, "倒退不是零");
    assert_eq!(evidence["verdict"], "relay_sample_unavailable");
}

#[test]
fn a_relay_path_that_only_appeared_at_a_boundary_voids_the_bracket() {
    // 评审 #2: `before` 只有直连时它的 relay 集合是空的, 空集合的 `all()` 真空成立 —— 一次
    // "边界上短暂 fallback 到 relay、终态前又只剩直连"的尝试会被读成四个 0 并判
    // `relay_bytes_zero`。对账必须用"本次见过的 relay path 并集"。
    let dir = TempRoot::new("evidence-union");
    let target = dir.0.join("landed.bin");
    let payload = b"boundary detour";
    let sha = sha256_hex(payload);
    let id = transfer_id("ev-union");
    let detour = RelayReading {
        tx: 120,
        rx: 60,
        relay_paths: vec!["PathId(9)@relay:https://relay.invalid:443/".to_owned()],
    };
    // 五个读数: open 前的基线、三次门禁采样 (@put-file / @file-chunk / @file-end)、终态前一次。
    // 第二个读数里出现了 relay path, 而基线跟终态都只有直连。
    let mut lane = start_admitted_with_relay(
        vec![PathClass::Direct],
        vec![
            Some(RelayReading::at(100, 50)),
            Some(detour),
            Some(RelayReading::at(120, 60)),
            Some(RelayReading::at(120, 60)),
            Some(RelayReading::at(120, 60)),
        ],
    );

    put_file(&mut lane, &id, &target, payload.len(), &sha);
    put_chunk(&mut lane, &id, 0, payload);
    lane.send(&format!(r#"@file-end:{{"transfer_id":"{id}","sha256":"{sha}"}}"#));
    let evidence = lane.reply()["evidence"].clone();

    for key in [
        "relay_tx_before",
        "relay_tx_after",
        "relay_rx_before",
        "relay_rx_after",
    ] {
        assert_eq!(evidence[key], Value::Null, "{key} 必须作废, 不是 0");
    }
    assert_eq!(evidence["relay_sample_available"], false);
    assert_eq!(evidence["verdict"], "relay_sample_unavailable");
}
