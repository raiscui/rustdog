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
