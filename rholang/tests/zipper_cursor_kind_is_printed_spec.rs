//! # Two zippers that are `!=` and hash differently must not print the same
//!
//! `EZipper.cursor_kind` is **half the cursor**. The canonical path codec has
//! two top-level arms, so a segment vector alone does not name an entry: the
//! bare element `5` (key `03 0a`) and the singleton list `[5]` (key
//! `03 0a 00`) share the segment `03 0a`, are DIFFERENT entries, and one map
//! may hold both at once.
//!
//! `models/src/lib.rs` already treats the discriminator as load-bearing —
//! `cursor_kind` participates in `EZipper`'s `PartialEq` and `Hash`, whose own
//! comment says *"two zippers agreeing on the segments but differing on the arm
//! are focused on DIFFERENT entries … and must not compare equal"* — and
//! `models/src/rust/spliced_event_bytes.rs` emits it into the event hash.
//!
//! The pretty printer did not read it. The consequence is stated as a
//! three-way conjunction below, and the third conjunct is the one that was
//! false: **`!=`, hashes differ, prints differ**.
//!
//! ## Why this matters beyond tidiness
//!
//! `build_channel_string`'s output is block-resident and replay-compared
//! (`casper/src/rust/rholang/replay_runtime.rs`), and it is reachable from
//! untrusted input through `rho:io:stdout`. A rendering that collapses two
//! distinct entries into one string is a rendering that cannot be used to tell
//! two states apart in a diagnostic — in the one place where telling them apart
//! is the whole job.
//!
//! ## The control
//!
//! `CursorKind::Split` is proto value 0, so it is what every `EZipper`
//! serialized before `cursor_kind` existed decodes to. Its rendering is
//! asserted BYTE-FOR-BYTE against the string the printer produced before the
//! discriminator was read at all. A fix that moves it is over-reaching.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EZipper, Expr, Par};
use models::rust::pathmap_integration::{render_cursor_position, CursorKind};
use models::rust::rhoapi_ext::EPathMap;
use rholang::rust::interpreter::pretty_printer::PrettyPrinter;

/// `encode_trie_segment(5)` — the segment the bare entry `5` and the singleton
/// list `[5]` have in common, and therefore the only place the discriminator
/// can be doing any work.
const SEGMENT_FIVE: [u8; 2] = [0x03, 0x0a];

fn gint(i: i64) -> Par {
    Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GInt(i)),
        }],
        ..Default::default()
    }
}

fn zipper_at_five(cursor_kind: u32) -> Par {
    Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::EZipperBody(EZipper {
                pathmap: Some(EPathMap::new(vec![gint(5)], vec![], false, None)),
                current_path: vec![SEGMENT_FIVE.to_vec()],
                is_write_zipper: false,
                locally_free: vec![],
                connective_used: false,
                cursor_kind,
            })),
        }],
        ..Default::default()
    }
}

fn hash_of(par: &Par) -> u64 {
    let mut hasher = DefaultHasher::new();
    par.hash(&mut hasher);
    hasher.finish()
}

fn print(par: &Par) -> String {
    PrettyPrinter::new().build_string_from_message(par)
}

/// ★ THE RED. The three-way conjunction, on a pair that differs in
/// `cursor_kind` and in nothing else.
#[test]
fn a_bare_cursor_and_a_split_cursor_over_the_same_segments_print_differently() {
    let split = zipper_at_five(CursorKind::Split.to_wire());
    let bare = zipper_at_five(CursorKind::Bare.to_wire());

    // The mutation is exactly one field.
    match (
        &split.exprs[0].expr_instance,
        &bare.exprs[0].expr_instance,
    ) {
        (
            Some(ExprInstance::EZipperBody(l)),
            Some(ExprInstance::EZipperBody(r)),
        ) => {
            assert_eq!(
                l.current_path, r.current_path,
                "the two zippers must agree on the segments, or the test is \
                 measuring the segments and not the arm"
            );
            assert_ne!(l.cursor_kind, r.cursor_kind, "…and differ on the arm");
        }
        other => panic!("fixture is not a pair of zippers: {other:?}"),
    }

    assert_ne!(split, bare, "conjunct 1: they are not equal");
    assert_ne!(
        hash_of(&split),
        hash_of(&bare),
        "conjunct 2: they hash differently"
    );
    assert_ne!(
        print(&split),
        print(&bare),
        "★ conjunct 3, the one that was FALSE: two values that are `!=` and \
         hash differently printed the SAME string, because the printer rendered \
         the segments and dropped the arm. split = {:?}, bare = {:?}",
        print(&split),
        print(&bare)
    );
}

/// The CONTROL, pinned byte-for-byte: the `Split` rendering — proto value 0,
/// hence every zipper representable before `cursor_kind` existed — is exactly
/// what the segment-only printer produced.
///
/// ⚠ This is the assertion that must NOT discriminate. These bytes reach a
/// block through `build_channel_string` -> `cap` -> `error_message`.
#[test]
fn the_split_rendering_is_byte_identical_to_the_segment_only_printer() {
    assert_eq!(
        CursorKind::Split.to_wire(),
        0,
        "the control rests on `Split` being the proto default"
    );
    assert_eq!(
        print(&zipper_at_five(CursorKind::Split.to_wire())),
        "ReadZipper(at: [@{5}], {|5|})",
        "the split arm must render exactly as it did before the discriminator \
         was read; a fix that moves this row is over-reaching"
    );

    // The empty cursor at the root, same obligation.
    let root = Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::EZipperBody(EZipper {
                pathmap: Some(EPathMap::new(vec![gint(5)], vec![], false, None)),
                current_path: vec![],
                is_write_zipper: false,
                locally_free: vec![],
                connective_used: false,
                cursor_kind: CursorKind::Split.to_wire(),
            })),
        }],
        ..Default::default()
    };
    assert_eq!(print(&root), "ReadZipper(at: [], {|5|})");
}

/// Every wire value the printer can be handed renders, and the five rows of the
/// rendering table are pairwise distinct at the same segments.
///
/// `cursor_kind` is a peer-controlled field, so "every wire value" includes the
/// ones `CursorKind::from_wire` refuses. The printer is in the error path and
/// must therefore be total: this asserts on the returned strings, never on a
/// panic.
#[test]
fn every_cursor_kind_on_the_wire_renders_and_the_rows_are_distinct() {
    let kinds: [u32; 4] = [
        CursorKind::Split.to_wire(),
        CursorKind::Bare.to_wire(),
        CursorKind::Prefix.to_wire(),
        99, // decodes to no `CursorKind`
    ];
    let rendered: Vec<String> = kinds.iter().map(|k| print(&zipper_at_five(*k))).collect();

    assert_eq!(
        rendered,
        vec![
            "ReadZipper(at: [@{5}], {|5|})".to_string(),
            "ReadZipper(at: @{5}, {|5|})".to_string(),
            "ReadZipper(at: [@{5}]?, {|5|})".to_string(),
            "ReadZipper(at: [@{5}]<unknown cursor kind 99>, {|5|})".to_string(),
        ],
        "the five rendering rows are pinned; `Bare` is the entry `5` itself and \
         `Split` is the singleton list `[5]`"
    );

    for (i, left) in rendered.iter().enumerate() {
        for right in rendered.iter().skip(i + 1) {
            assert_ne!(left, right, "the rows must be pairwise distinct");
        }
    }
}

/// The leaf itself, exercised where no `EZipper` can reach it: a `Bare` cursor
/// at a depth no bare entry inhabits.
///
/// A bare entry's key is exactly ONE segment, so `Bare` is inhabited at exactly
/// one cursor depth — 1. Every other depth is representable on the wire and
/// names no entry. The renderer marks it instead of asserting, because a
/// printer that can fail cannot be used in an error path.
#[test]
fn a_bare_cursor_at_an_uninhabited_depth_is_marked_rather_than_rejected() {
    let two = vec!["5".to_string(), "6".to_string()];
    assert_eq!(
        render_cursor_position(CursorKind::Bare.to_wire(), &two),
        "bare([5, 6])"
    );
    assert_eq!(
        render_cursor_position(CursorKind::Bare.to_wire(), &[]),
        "bare([])"
    );
    // …and the inhabited depth is the unbracketed element.
    assert_eq!(
        render_cursor_position(CursorKind::Bare.to_wire(), &["5".to_string()]),
        "5"
    );
    // CONTROL: the split arm is the bracketed list at EVERY depth, including
    // the two the bare arm cannot inhabit.
    assert_eq!(
        render_cursor_position(CursorKind::Split.to_wire(), &two),
        "[5, 6]"
    );
    assert_eq!(render_cursor_position(CursorKind::Split.to_wire(), &[]), "[]");
    assert_eq!(
        render_cursor_position(CursorKind::Split.to_wire(), &["5".to_string()]),
        "[5]"
    );
}
