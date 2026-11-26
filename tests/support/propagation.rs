//! 声明传播自检 (RAI-580)。
//!
//! `zenoh 1.8` 的 `declare_subscriber` 只把 `Declare` 消息发出去, 不等 router 确认;
//! `DeclareSubscriber::wait()` 只保证**本地注册**完成。于是「声明之后立刻 pub」在跨进程 e2e 里
//! 可能丢首条消息 —— 传播没完成时, pub 在 router 上找不到匹配订阅者, 直接丢弃 (无缓存无重试)。
//!
//! 这里的辅助把「传播完成」变成**可观测条件**: 在订阅声明之后, 于同一个 session 上声明一个
//! liveliness token, 再用**另一个 session** 去 query 这个 token。第二个 session 只能通过 router
//! 看到它, 所以「它答得出」⇒ router 已经处理过这个 session 之前的 Declare 消息 (同一 session 的
//! Declare 按序到达) ⇒ 订阅声明已经对 router 可见。
//!
//! 为什么不用固定 sleep: 传播时间取决于 router 负载与连接建立, 猜时长在 CI 负载下必然偶发失败;
//! 而 query 是可观测的, 拿到了就立刻返回 (本机通常 1-3 ms), 拿不到就在预算内继续重试。

use std::time::{Duration, Instant};

use zenoh::{key_expr::KeyExpr, Session, Wait};

/// liveliness token 的 key 后缀, 只在测量期间存在。
const PROBE_SUFFIX: &str = "rdog-visibility-probe";

pub const DEFAULT_BUDGET: Duration = Duration::from_secs(5);

fn probe_key_expr(subscribed: &str, nonce: u64) -> Result<KeyExpr<'static>, String> {
    let raw = format!("{subscribed}/{PROBE_SUFFIX}-{}-{nonce}", std::process::id());
    KeyExpr::try_from(raw.clone()).map_err(|err| format!("probe keyexpr {raw}: {err}"))
}

/// 等到 `declaring` 上已经声明过的订阅对 router 可见。
///
/// `declaring` 是声明订阅的那个 session, `witness` 必须是**另一个**连到同一 router 的 session
/// (用它 query 才能证明结果来自 router, 而不是本地状态)。返回 `Ok` 时, router 已经处理过
/// `declaring` 在此之前的全部 Declare 消息, 因此现在 pub 到 `subscribed` 不会被丢。
pub fn await_subscriber_visible(
    declaring: &Session,
    witness: &Session,
    subscribed: &str,
) -> Result<(), String> {
    await_subscriber_visible_within(declaring, witness, subscribed, DEFAULT_BUDGET)
}

pub fn await_subscriber_visible_within(
    declaring: &Session,
    witness: &Session,
    subscribed: &str,
    budget: Duration,
) -> Result<(), String> {
    let probe = probe_key_expr(subscribed, nonce())?;
    // token 的声明晚于订阅声明, 且两者的 Declare 在同一 session 上按序发出。
    let _token = declaring
        .liveliness()
        .declare_token(probe.clone())
        .wait()
        .map_err(|err| format!("declare visibility token on {probe}: {err}"))?;

    let deadline = Instant::now() + budget;
    let mut attempts = 0u32;
    loop {
        attempts += 1;
        let replies = witness
            .liveliness()
            .get(probe.clone())
            .wait()
            .map_err(|err| format!("liveliness query for {probe}: {err}"))?;
        // 用 recv_timeout 而不是 for: 没匹配时 query 要等到超时才收尾, 那会把每次重试拖到
        // 会话的默认 query 超时 (秒级); 我们只需要「这一次有没有答出来」。
        while let Ok(Some(reply)) = replies.recv_timeout(Duration::from_millis(200)) {
            let Ok(sample) = reply.result() else { continue };
            if sample.key_expr() == &probe {
                return Ok(());
            }
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "router still does not report the declaration probe for `{subscribed}` \
                 after {attempts} queries within {budget:?} (declaration has not propagated)"
            ));
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn nonce() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    NEXT.fetch_add(1, Ordering::Relaxed)
}
