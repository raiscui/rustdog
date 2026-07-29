use std::fs;

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};

use super::{
    bundle::Bundle,
    delivery::{DeliveryError, DeliveryFailureReason, DeliveryManager},
    session::ConnectionId,
};

fn bundle() -> Bundle {
    let path =
        std::env::temp_dir().join(format!("rdog-delivery-{}.rdogrec.tar", std::process::id()));
    fs::write(&path, b"bundle").unwrap();
    Bundle {
        path,
        size_bytes: 6,
        sha256: "unused".to_owned(),
    }
}

#[test]
fn only_owner_receives_single_base64_savefile_frame() {
    let manager = DeliveryManager::default();
    let bundle = bundle();
    assert!(matches!(
        manager.frame_for_owner(ConnectionId(1), ConnectionId(2), None, &bundle),
        Err(DeliveryError::NotOwner)
    ));

    let frame = manager
        .frame_for_owner(ConnectionId(1), ConnectionId(1), Some(9), &bundle)
        .unwrap();
    assert_eq!(frame.request_id, Some(9));
    assert_eq!(frame.mime, "application/vnd.rdog.recording-bundle");
    assert_eq!(BASE64_STANDARD.decode(frame.data).unwrap(), b"bundle");
}

#[test]
fn sixth_stop_within_one_second_is_rate_limited() {
    let mut manager = DeliveryManager::default();
    let connection = ConnectionId(1);
    for _ in 0..5 {
        manager.check_record_stop(connection).unwrap();
    }
    let err = manager.check_record_stop(connection).unwrap_err();
    assert!(matches!(
        err,
        DeliveryError::Rejected(DeliveryFailureReason::RateLimited)
    ));
}

#[test]
fn delivery_reason_codes_are_stable() {
    assert_eq!(
        DeliveryFailureReason::ChecksumMismatch.as_str(),
        "checksum_mismatch"
    );
    assert_eq!(
        DeliveryFailureReason::BundleTooLarge.as_str(),
        "bundle_too_large"
    );
    assert_eq!(DeliveryFailureReason::Cancelled.as_str(), "cancelled");
    assert_eq!(DeliveryFailureReason::RateLimited.as_str(), "rate_limited");
}
