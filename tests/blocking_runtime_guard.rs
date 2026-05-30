#![allow(clippy::unwrap_used)]
#[cfg(feature = "blocking")]
#[test]
fn honcho_new_does_not_panic() {
    let honcho = honcho_ai::blocking::Honcho::new("http://localhost:9999", "ws");
    assert!(honcho.is_ok());
    assert_eq!(honcho.unwrap().workspace_id(), "ws");
}

#[cfg(feature = "blocking")]
#[tokio::test]
async fn blocking_force_ensure_inside_async_returns_error() {
    let honcho = honcho_ai::blocking::Honcho::new("http://localhost:9999", "ws").unwrap();
    let err = honcho.force_ensure().unwrap_err();
    assert!(
        matches!(err, honcho_ai::error::HonchoError::Configuration(_)),
        "expected Configuration error, got {err:?}"
    );
    assert!(
        err.to_string()
            .contains("cannot be called from within an async runtime")
    );
}
