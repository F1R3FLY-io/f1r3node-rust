use super::*;

#[test]
fn missing_public_guard_is_detected_before_canonical_fast_import() {
    let outcome = std::panic::catch_unwind(|| {
        crate::protobuf_decoder_oracle::legacy(|| {
            decode_par_with_epathmap_validation(&[][..], true)
        })
    });
    let panic = outcome.expect_err("unguarded canonical fast-import entry must be refused");
    assert_eq!(
        panic.downcast_ref::<&str>().copied(),
        Some("successor body bypassed decoder routing")
    );
}

#[test]
fn missing_public_guard_is_detected_before_machine_execution() {
    let outcome = std::panic::catch_unwind(|| {
        crate::protobuf_decoder_oracle::legacy(|| {
            Machine::new(
                &[][..],
                Node::Par {
                    value: Par::default(),
                    limit: 0,
                },
                true,
            )
            .run()
        })
    });
    let panic = outcome.expect_err("unguarded machine entry must be refused");
    assert_eq!(
        panic.downcast_ref::<&str>().copied(),
        Some("successor body bypassed decoder routing")
    );
}
