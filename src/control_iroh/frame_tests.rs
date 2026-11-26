use super::*;

#[test]
fn a_timed_out_frame_makes_the_stream_unusable() {
    let guard = FrameGuard::new(Duration::from_millis(50));
    assert!(guard
        .write("first", |_frame| Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "iroh line write timed out"
        )))
        .is_err());

    let mut written = 0;
    let second = guard.write("second", |_frame| {
        written += 1;
        Ok(())
    });
    assert!(
        second.is_err(),
        "a frame written onto a stream that holds a partial frame must be refused"
    );
    assert_eq!(written, 0, "the refused frame must not reach the stream");
    assert!(
        second.unwrap_err().to_string().contains(FRAME_BROKEN),
        "the refusal has to name the broken frame stream"
    );
}

#[test]
fn healthy_writes_keep_the_stream_usable() {
    let guard = FrameGuard::new(Duration::from_millis(50));
    let mut frames: Vec<String> = Vec::new();
    for message in ["one", "two", "three"] {
        guard
            .write(message, |frame| {
                frames.push(String::from_utf8_lossy(frame).into_owned());
                Ok(())
            })
            .expect("a healthy write is accepted");
    }
    assert_eq!(frames, vec!["one\n", "two\n", "three\n"]);
}

#[test]
fn a_large_frame_gets_a_budget_above_the_profile_default() {
    // 6.7 MB 的 bulk 帧在 96 KiB/s 的目标链路上要 ~71 s: 固定 3 s 预算会把慢但可用的链路
    // 判成坏帧。预算必须随帧长走。
    let configured = Duration::from_secs(3);
    let small = super::frame_write_budget(1024, configured);
    assert_eq!(small, configured, "小帧不受影响");
    let large = super::frame_write_budget(6_700_000, configured);
    assert!(
        large > configured,
        "大帧的预算必须高于 profile 默认值: {large:?}"
    );
    assert!(
        large >= Duration::from_secs(60),
        "按 96 KiB/s 算 6.7 MB 应给到一分钟量级: {large:?}"
    );
}

#[test]
fn the_frame_budget_never_undercuts_the_profile() {
    // profile 显式抬到 30 s 时, 即便是小帧也不许被压回去。
    let configured = Duration::from_secs(30);
    assert_eq!(
        super::frame_write_budget(64, configured),
        configured,
        "预算只抬不降"
    );
    assert_eq!(
        super::frame_write_budget(0, configured),
        configured,
        "空帧同样按 profile"
    );
}
