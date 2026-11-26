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

#[test]
#[should_panic(expected = "Cannot start a runtime from within a runtime")]
fn blocking_on_a_handle_inside_an_async_task_is_the_panic_we_hit_on_the_lane() {
    // 真机现场: `with_bulk_lane` 的握手留在 async 块里, 而 `IrohLineTransport` 每次读写都是
    // `handle.block_on` —— 在 async 上下文里再 block_on 自己必然 panic, 且这个 panic 与 handle
    // 是不是同一个运行时无关 (这里就是同一个)。
    let rt = super::runtime().unwrap();
    rt.block_on(async {
        let handle = super::Handle::current();
        let _ = handle.block_on(async { 1 });
    });
}

#[test]
fn the_same_block_on_on_a_blocking_thread_is_fine() {
    // 修法的形状与 PTY 客户端一致: 行 I/O 整段放进 `spawn_blocking`, 在那里 block_on 是合法的。
    let rt = super::runtime().unwrap();
    let value = rt.block_on(async {
        let handle = super::Handle::current();
        tokio::task::spawn_blocking(move || handle.block_on(async { 7u64 }))
            .await
            .expect("blocking task joined")
    });
    assert_eq!(value, 7, "阻塞线程上 block_on 同一个 runtime 是合法的");
}
