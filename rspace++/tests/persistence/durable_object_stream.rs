use crate::rspace::persistence::canonical::CanonicalSink;
use crate::rspace::persistence::durable::{
    DurableObjectCopyErrorV1, DurableObjectReadErrorV1, copy_verified_canonical_with,
    copy_verified_canonical_with_scratch,
};
use crate::rspace::persistence::{ObjectDigest, ObjectRefV1};

fn source_bytes(length: usize) -> Vec<u8> { (0..length).map(|index| (index % 251) as u8).collect() }

fn reference(bytes: &[u8]) -> ObjectRefV1 {
    ObjectRefV1::new(ObjectDigest::of_canonical_bytes(bytes), bytes.len() as u64).unwrap()
}

#[test]
fn large_objects_stream_through_fixed_heap_scratch_on_a_small_stack() {
    let source = source_bytes(1_048_613);
    let expected = reference(&source);
    std::thread::Builder::new()
        .stack_size(128 * 1024)
        .spawn(move || {
            let mut output = Vec::new();
            copy_verified_canonical_with(expected, &mut output, |offset, destination| {
                let start = offset as usize;
                destination.copy_from_slice(&source[start..start + destination.len()]);
                Ok(())
            })
            .unwrap();
            assert_eq!(output, source);
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn caller_owned_copy_scratch_reuses_one_bounded_allocation_across_objects() {
    let first = source_bytes(131_101);
    let second = source_bytes(98_321);
    let mut scratch = Vec::new();
    let mut output = Vec::new();

    let first_reference = reference(&first);
    copy_verified_canonical_with_scratch(
        first_reference,
        &mut output,
        &mut scratch,
        |offset, destination| {
            let start = offset as usize;
            destination.copy_from_slice(&first[start..start + destination.len()]);
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(output, first);
    let allocation = scratch.as_ptr();
    let capacity = scratch.capacity();
    assert_eq!(scratch.len(), 64 * 1024);

    output.clear();
    let second_reference = reference(&second);
    copy_verified_canonical_with_scratch(
        second_reference,
        &mut output,
        &mut scratch,
        |offset, destination| {
            let start = offset as usize;
            destination.copy_from_slice(&second[start..start + destination.len()]);
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(output, second);
    assert_eq!(scratch.as_ptr(), allocation);
    assert_eq!(scratch.capacity(), capacity);
    assert_eq!(scratch.len(), 64 * 1024);
}

#[test]
fn post_admission_corruption_is_detected_before_copy_success() {
    let source = source_bytes(131_101);
    let expected = reference(&source);
    let mut corrupt = source;
    corrupt[100_000] ^= 1;
    let mut output = Vec::new();
    assert!(matches!(
        copy_verified_canonical_with(expected, &mut output, |offset, destination| {
            let start = offset as usize;
            destination.copy_from_slice(&corrupt[start..start + destination.len()]);
            Ok(())
        }),
        Err(DurableObjectCopyErrorV1::Object(
            DurableObjectReadErrorV1::CopiedDigestMismatch { .. }
        ))
    ));
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SinkFailure;

impl std::fmt::Display for SinkFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("injected sink failure")
    }
}

struct FailingSink;

impl CanonicalSink for FailingSink {
    type Error = SinkFailure;

    fn write_all(&mut self, _bytes: &[u8]) -> Result<(), Self::Error> { Err(SinkFailure) }
}

#[test]
fn sink_failure_cannot_be_misreported_as_a_complete_copy() {
    let source = source_bytes(4096);
    let expected = reference(&source);
    assert!(matches!(
        copy_verified_canonical_with(expected, &mut FailingSink, |offset, destination| {
            let start = offset as usize;
            destination.copy_from_slice(&source[start..start + destination.len()]);
            Ok(())
        }),
        Err(DurableObjectCopyErrorV1::Sink(SinkFailure))
    ));
}
