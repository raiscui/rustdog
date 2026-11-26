//! 声明传播自检 (RAI-580)。
//!
//! `zenoh 1.8` 的 `declare_subscriber` 只把 `Declare` 消息发出去, 不等 router 确认;
//! `DeclareSubscriber::wait()` 只保证**本地注册**完成。于是「声明之后立刻 pub」在跨进程 e2e 里
//! 可能丢首条消息 —— 传播没完成时, pub 在 router 上找不到匹配订阅者, 直接丢弃 (无缓存无重试)。
//!
//! 这里的辅助把「传播完成」变成**可观测条件**: 订阅声明之后, 于**同一个 session** 上再声明一个
//! queryable (同一 session 的 Declare 按序到达 router), 然后用**另一个 session** 去 get 它。
//! 回复必须经 router 路由到声明方再回来, 所以「答得出」⇒ router 已经处理过这个 session 之前的
//! Declare ⇒ 订阅声明对 router 可见。
//!
//! 为什么不用 liveliness token: 实测在 CI runner 上它不可靠 (一条从没答出来, 另一条在 router
//! 被 SIGSTOP 之后反而答了 —— 说明客户端 session 会就本地 liveliness 状态直接作答)。queryable
//! 的回复只能由声明方经 router 交付, 没有这个歧义。
//!
//! 为什么不用固定 sleep: 传播时间取决于 router 负载与连接建立, 猜时长在 CI 负载下必然偶发失败;
//! 而 query 是可观测的, 拿到了就立刻返回 (本机 1-3 ms), 拿不到就在预算内重试。

use std::time::{Duration, Instant};

use zenoh::{key_expr::KeyExpr, Session, Wait};

/// queryable 的 key 后缀, 只在测量期间存在。
const PROBE_SUFFIX: &str = "rdog-visibility-probe";

pub const DEFAULT_BUDGET: Duration = Duration::from_secs(5);

fn probe_key_expr(subscribed: &str, nonce: u64) -> Result<KeyExpr<'static>, String> {
    let raw = format!("{subscribed}/{PROBE_SUFFIX}-{}-{nonce}", std::process::id());
    KeyExpr::try_from(raw).map_err(|err| format!("probe keyexpr: {err}"))
}

/// 等到 `declaring` 上已经声明过的订阅对 router 可见。
///
/// `declaring` 是声明订阅的那个 session, `witness` 必须是**另一个**连到同一 router 的 session
/// (用它 query 才能证明结果经 router 转过)。返回 `Ok` 时, router 已经处理过 `declaring` 在此
/// 之前的全部 Declare 消息, 因此现在 pub 到 `subscribed` 不会被丢。
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
    let nonce = nonce();
    let probe = probe_key_expr(subscribed, nonce)?;
    // queryable 的声明晚于订阅声明, 且两者的 Declare 在同一 session 上按序发出。
    // 回复里带 nonce: 只判 `result().is_ok()` 的话, 任何第三方在本 keyexpr 下挂的 queryable 都会让
    // 探针假绿 (实测 3.6ms 就 Ok) —— 那等于在承重断言上开了口子。
    let answer_key = probe.clone();
    let answer = format!("visible-{nonce}");
    let reply_body = answer.clone();
    let _queryable = declaring
        .declare_queryable(probe.clone())
        .callback(move |query| {
            let _ = query.reply(answer_key.clone(), reply_body.clone()).wait();
        })
        .wait()
        .map_err(|err| format!("declare visibility queryable on {probe}: {err}"))?;

    let deadline = Instant::now() + budget;
    let mut attempts = 0u32;
    loop {
        attempts += 1;
        let replies = witness
            .get(probe.clone())
            .wait()
            .map_err(|err| format!("visibility query for {probe}: {err}"))?;
        // 用 recv_timeout 而不是 for: 没有答复时 query 要等到超时才收尾, 那会把每次重试拖到
        // 会话的默认 query 超时 (秒级); 我们只需要「这一次有没有答出来」。
        while let Ok(Some(reply)) = replies.recv_timeout(Duration::from_millis(200)) {
            let Ok(sample) = reply.result() else { continue };
            if is_probe_reply(
                sample.key_expr(),
                sample
                    .payload()
                    .try_to_string()
                    .ok()
                    .map(|body| body.into_owned()),
                &probe,
                &answer,
            ) {
                return Ok(());
            }
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "router still does not deliver a probe reply for `{subscribed}` \
                 after {attempts} queries within {budget:?} (declaration has not propagated)"
            ));
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// 一条回复算不算本探针的答复: keyexpr 必须是探针自己那条, payload 必须带着本次的 nonce。
///
/// 抽成纯函数是为了能直接测: 只判 `result().is_ok()` 的版本会把任何一个第三方在 `rdog/lab/**`
/// 下挂的 queryable 当成「可见」—— 断言还在, 但它已经不证明任何事了。
pub fn is_probe_reply(keyexpr: &KeyExpr, payload: Option<String>, probe: &KeyExpr, expected: &str) -> bool {
    keyexpr == probe && payload.as_deref() == Some(expected)
}

fn nonce() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    NEXT.fetch_add(1, Ordering::Relaxed)
}
