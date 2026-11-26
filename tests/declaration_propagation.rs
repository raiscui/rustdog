//! 声明传播的确定性自检 (RAI-580)。
//!
//! `zenoh 1.8` 的 `declare_subscriber` 只把 `Declare` 发出去, `.wait()` 只保证本地注册; 声明是否已经
//! 对 router 可见是**异步**的。本测试做两件事:
//!
//! 1. **guarded 臂**: 订阅声明后先 `await_subscriber_visible` 再投递, 20 轮 0 丢失;
//! 2. **负对照**: 把 router 进程 SIGSTOP 之后, 同一个自检必须**答不出来** (返回 Err) —— 这一条证明
//!    自检看的是 router 的可见性, 而不是本地注册状态; SIGCONT 之后必须重新成功。
//!
//! 另外把 naive 臂 (声明后立刻投递, 不自检) 的丢包数打到 stderr。**在本机 loopback 上它是 0**
//! (实测 750+ 次投递无一丢失), 原因见下面的注释 —— 所以这条不能当「自检修掉了本地可复现的丢失」
//! 来读, 见 `docs/solutions/conventions/zenoh-pub-sub-declaration-propagation.md` 的边界说明。

#![cfg(unix)]

#[path = "support/propagation.rs"]
mod propagation;

use std::{
    fs,
    path::PathBuf,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use zenoh::{config::Config, Session, Wait};

/// 这两个测试各自起一个 daemon + 若干 client session; 同一个 runner 上并行跑会互相抢端口与
/// CPU, 也会让「谁连了哪个 router」变得难以判读 —— 按 `parallel-test-global-state-single-lock`
/// 的纪律用模块级唯一锁串起来。
static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

const ROUNDS_DEFAULT: usize = 20;
/// 单轮里等首条 sample 的预算: 足够覆盖正常投递 (本机 ~1ms), 又不至于让一次丢包拖很久。
const FIRST_SAMPLE_BUDGET: Duration = Duration::from_millis(400);

fn rounds() -> usize {
    std::env::var("RDOG_PROPAGATION_ROUNDS")
        .ok()
        .and_then(|raw| raw.parse().ok())
        .filter(|n| *n > 0)
        .unwrap_or(ROUNDS_DEFAULT)
}

fn rdog_binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_rdog"))
}

fn isolated_home(tag: &str) -> PathBuf {
    // 短根目录: macOS runner 上 `temp_dir()` 是 /var/folders/... 的长路径, daemon 的 unixpipe
    // socket 名会超过 sun_path 上限而起不来 (同 `docs/solutions/conventions/e2e-isolated-home-credentials.md`
    // 里 a45f1fa 修过的那条)。
    let dir = PathBuf::from("/tmp").join(format!("rdog-prop-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join(".rdog")).expect("isolated home");
    fs::write(
        dir.join(".rdog/auth.toml"),
        "user = \"propagation\"\npassword = \"propagation-pw\"\n",
    )
    .expect("auth.toml");
    dir
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind ephemeral")
        .local_addr()
        .expect("local addr")
        .port()
}

struct Daemon {
    child: Child,
    home: PathBuf,
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_dir_all(&self.home);
    }
}

impl Daemon {
    fn pid(&self) -> i32 {
        self.child.id() as i32
    }

    fn signal(&self, signal: i32) {
        // SAFETY: 只是对自己 spawn 的子进程发信号。
        let sent = unsafe { libc::kill(self.pid(), signal) };
        assert_eq!(sent, 0, "failed to signal daemon {}", self.pid());
    }
}

fn start_daemon() -> (Daemon, String) {
    let home = isolated_home("daemon");
    let port = free_port();
    let name = format!("propagation-{}.lab", std::process::id());
    let config = home.join("daemon.toml");
    fs::write(
        &config,
        format!(
            "[observation]\ndurable_enabled = false\n\n[zenoh]\nenabled = true\nmode = \"router\"\n\
             namespace = \"lab\"\ndaemon_name = \"{name}\"\n\
             listen_endpoints = [\"tcp/127.0.0.1:{port}\"]\n"
        ),
    )
    .expect("daemon config");
    let log = home.join("daemon.log");
    let out = fs::File::create(&log).expect("daemon log");
    let child = Command::new(rdog_binary())
        .args(["daemon", "-c", &config.display().to_string()])
        .env("HOME", &home)
        .stdin(Stdio::null())
        .stdout(out.try_clone().expect("log clone"))
        .stderr(out)
        .spawn()
        .expect("daemon should start");
    let daemon = Daemon { child, home };
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if fs::read_to_string(&log)
            .unwrap_or_default()
            .contains("zenoh router daemon ready")
        {
            return (daemon, format!("tcp/127.0.0.1:{port}"));
        }
        thread::sleep(Duration::from_millis(20));
    }
    let log_text = fs::read_to_string(&log).unwrap_or_default();
    panic!(
        "daemon never reported ready; last log lines:\n{}",
        log_text.lines().rev().take(15).collect::<Vec<_>>().join("\n")
    );
}

fn open_session(entrypoint: &str) -> Session {
    let mut config = Config::default();
    config
        .insert_json5("connect/endpoints", &format!("[\"{entrypoint}\"]"))
        .expect("connect endpoints");
    config
        .insert_json5("transport/auth/usrpwd/user", "\"propagation\"")
        .expect("auth user");
    config
        .insert_json5("transport/auth/usrpwd/password", "\"propagation-pw\"")
        .expect("auth password");
    config
        .insert_json5("scouting/multicast/enabled", "false")
        .expect("disable multicast");
    zenoh::open(config).wait().expect("session open")
}

/// 一种投递: `guard` 为真时先做传播自检, 否则声明后立刻投递 (naive 臂)。
fn deliver_one_round(entrypoint: &str, key: &str, guard: bool) -> bool {
    let subscriber_session = open_session(entrypoint);
    let publisher_session = open_session(entrypoint);
    let subscriber = subscriber_session
        .declare_subscriber(key)
        .wait()
        .expect("declare subscriber");
    if guard {
        propagation::await_subscriber_visible(&subscriber_session, &publisher_session, key)
            .expect("declaration should become visible");
    }
    publisher_session.put(key, "first").wait().expect("put");
    let deadline = Instant::now() + FIRST_SAMPLE_BUDGET;
    while Instant::now() < deadline {
        if subscriber.recv_timeout(Duration::from_millis(20)).is_ok() {
            return true;
        }
    }
    false
}

#[test]
fn declaration_self_check_should_make_first_message_deterministic() {
    let _serial = TEST_LOCK.lock().unwrap_or_else(|err| err.into_inner());
    // `_daemon` 只用来保活: drop 时 kill。
    let (_daemon, entrypoint) = start_daemon();
    let total = rounds();

    let mut guarded_lost = Vec::new();
    let mut naive_lost = Vec::new();
    for round in 0..total {
        if !deliver_one_round(&entrypoint, &format!("rdog/lab/propagation/guarded/{round}"), true) {
            guarded_lost.push(round);
        }
        if !deliver_one_round(&entrypoint, &format!("rdog/lab/propagation/naive/{round}"), false) {
            naive_lost.push(round);
        }
    }
    eprintln!(
        "declaration propagation: {total} rounds — guarded lost {guarded_lost:?}, \
         naive lost {} {naive_lost:?} (naive 在上游能丢, 但本机 loopback 上实测为 0, 见文件头注释)",
        naive_lost.len()
    );
    assert!(
        guarded_lost.is_empty(),
        "self-checked declaration still lost the first message in rounds {guarded_lost:?}"
    );
}

/// 承重断言的口子: 只有「keyexpr 是探针那条 **且** payload 带着本次 nonce」的回复才算可见。
///
/// 这一条直接测谓词, 不去造路由: 第三方在 `rdog/lab/**` 下挂 queryable 时, 旧版本 (只判
/// `result().is_ok()`) 会被它 3.6ms 骗过; 现在它的回复既没有探针 keyexpr 也没有 nonce, 不算数。
#[test]
fn probe_should_only_accept_its_own_nonce_carrying_reply() {
    use zenoh::key_expr::KeyExpr;
    let probe = KeyExpr::try_from("rdog/lab/propagation/x/rdog-visibility-probe-1-7").unwrap();
    let other = KeyExpr::try_from("rdog/lab/propagation/y/rdog-visibility-probe-1-7").unwrap();
    let expected = "visible-7";

    assert!(propagation::is_probe_reply(&probe, Some(expected.to_owned()), &probe, expected));
    assert!(
        !propagation::is_probe_reply(&other, Some(expected.to_owned()), &probe, expected),
        "a reply from another keyexpr must not count"
    );
    assert!(
        !propagation::is_probe_reply(&probe, Some("visible".to_owned()), &probe, expected),
        "a reply without the per-call nonce must not count (this is the foreign-queryable hole)"
    );
    assert!(
        !propagation::is_probe_reply(&probe, None, &probe, expected),
        "an empty payload must not count"
    );
}

/// 负对照: 自检必须依赖 router 的可见性, 而不是本地注册状态。
///
/// 把 daemon 进程 SIGSTOP 之后, 声明仍然会成功 (本地注册 + 写进 socket), 但 router 不再处理任何
/// 消息 —— 所以自检必须**答不出来**。SIGCONT 之后必须重新答得出来。
#[test]
fn self_check_should_fail_when_the_router_is_not_processing() {
    let _serial = TEST_LOCK.lock().unwrap_or_else(|err| err.into_inner());
    let (daemon, entrypoint) = start_daemon();
    let subscriber_session = open_session(&entrypoint);
    let witness = open_session(&entrypoint);
    let key = "rdog/lab/propagation/paused";

    // 先证明正常路径下自检是成功的 (排除「自检永远失败」这种假红)。
    subscriber_session
        .declare_subscriber(key)
        .wait()
        .expect("subscriber");
    propagation::await_subscriber_visible_within(
        &subscriber_session,
        &witness,
        key,
        Duration::from_secs(5),
    )
    .expect("router is running, declaration should be visible");

    daemon.signal(libc::SIGSTOP);
    let key_paused = "rdog/lab/propagation/paused-2";
    subscriber_session
        .declare_subscriber(key_paused)
        .wait()
        .expect("subscriber while router is paused");
    let paused = propagation::await_subscriber_visible_within(
        &subscriber_session,
        &witness,
        key_paused,
        Duration::from_secs(2),
    );
    daemon.signal(libc::SIGCONT);

    let error = paused.expect_err(
        "self-check must not report visibility while the router is stopped: it would mean the check \
         is reading local state instead of the router's",
    );
    eprintln!("self-check while router paused: {error}");
    assert!(
        error.contains("has not propagated"),
        "unexpected failure text from the self-check: {error}"
    );

    // router 恢复后, 同一个 keyexpr 必须重新可见 —— 证明刚才的 Err 来自「暂停」而不是别的坏状态。
    propagation::await_subscriber_visible_within(
        &subscriber_session,
        &witness,
        key_paused,
        Duration::from_secs(10),
    )
    .expect("after SIGCONT the declaration should become visible");
}
