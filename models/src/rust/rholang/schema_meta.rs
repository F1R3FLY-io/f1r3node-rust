//! # `schema_meta` — what the schema IS, as data the program can read
//!
//! The hand-written half of the schema-meta table. Its generated twin —
//! `SCHEMA_CHILDREN`, `SCHEMA_SCC`, `RECURSIVE_TYPES`,
//! `DERIVE_DISPOSITION_REGISTRY`, `HAND_WRITTEN_TRAVERSALS` and
//! `DISPOSITIONED_DERIVES` — is emitted by `models/build/wire_schema.rs` into
//! `OUT_DIR/rhoapi_schema_meta.rs` and included by
//! [`crate::rust::rholang::schema_meta_tables`].
//!
//! ---
//!
//! ## Why the schema's own shape is a first-class artifact
//!
//! Every driver this campaign writes replaces a recursive walk. Two questions
//! decide whether such a driver is correct and whether it is complete:
//!
//! 1. **Which types can contain themselves?** That is the set a walk can be
//!    unbounded over, and it is [`SCHEMA_SCC`] / `RECURSIVE_TYPES` — Tarjan's
//!    decomposition of the child relation, computed from the SAME resolved
//!    fields the wire tables are generated from. Recomputing it from the
//!    `.proto` by hand would be a second reading, and a second reading of one
//!    truth does not stay equal to the first: the Θ(depth) audit's converted and
//!    tripwired sets existed twice, and both prose copies went stale *within the
//!    hour* of being reconciled, twice.
//!
//! 2. **Which walks are there at all?** That is
//!    [`DERIVE_DISPOSITION_REGISTRY`], and it is the question this module exists
//!    to stop anybody answering from memory.
//!
//! ## ★★ The derive-disposition registry, and the defect it replaces
//!
//! The campaign's driver list was, at one point, hand-picked: four traits
//! somebody named. **It missed `Hash` entirely**, and the enumeration that
//! replaced it additionally found `Ord`/`PartialOrd`, which nobody had named.
//! Neither `hash` nor `ord` appeared anywhere in `rholang/tests/
//! stack_depth_gate.rs` — not in `CONVERTED_DEPTH`, not in `TRIPWIRE_DEPTH`, and
//! in no `assert_slope_below` call. Their absence meant UNMEASURED, and absence
//! reads exactly like flatness from outside.
//!
//! So the list is **derived**: `models/build/wire_schema.rs` holds a closed
//! table mapping every `#[derive]` token that reaches `OUT_DIR/rhoapi.rs` to the
//! run-time surfaces it expands to, each with a [`Disposition`]; the generator
//! emits the cross product with the descriptor's items; and `models/build.rs`
//! **cross-checks the closed table against a textual scan of the generated
//! file**, exactly as it already cross-checks the `locally_free` rewrite. A
//! seventh trait fails the build, naming itself.
//!
//! ## ⚠ The registry is a LOWER BOUND, and says so
//!
//! `models/build.rs` STRIPS `PartialEq`, `Eq` and `Hash` from prost's output and
//! `models/src/lib.rs` writes them **by hand** — `<Par as PartialEq>::eq`
//! deliberately ignores `locally_free`, which no derive would do. They are
//! recursive walks all the same, and no `#[derive]` scan can see them.
//! `HAND_WRITTEN_TRAVERSALS` carries them, and the implicit `Drop` glue with
//! them.
//!
//! ★ That distinction is the whole method: the registry is complete *for what it
//! covers*, and the boundary of what it covers is stated rather than left for a
//! reader to discover by being wrong.

/// What the four-quadrant campaign has decided about one run-time surface of one
/// `#[derive]`.
///
/// ⚠ There is no "unknown" variant. A trait token that reaches
/// `OUT_DIR/rhoapi.rs` without a row in the generator's closed
/// `DERIVE_DISPOSITIONS` table **fails the build**; it cannot be carried as an
/// unclassified entry that nobody notices.
///
/// The payload string is never decorative — it is the part a reader needs and a
/// bare enum would not carry:
///
/// * [`Disposition::NotATraversal`] says **why** it does not recurse, because
///   "it does not recurse" is a claim about code;
/// * [`Disposition::Converted`] names the **driver** that replaced it, so the
///   claim is checkable;
/// * [`Disposition::Remaining`] names the **stage** that owes it one;
/// * [`Disposition::FollowsFrom`] names the driver that subsumes it, so
///   "no driver of its own" is an argument rather than an omission.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Disposition {
    /// Not a walk over term structure at all — a marker trait, a type-level
    /// artifact, or a constructor that fills defaults without descending.
    NotATraversal(&'static str),
    /// A recursive walk that an explicit-worklist driver has already replaced.
    Converted(&'static str),
    /// A recursive walk with no driver yet.
    Remaining(&'static str),
    /// A recursive walk that needs no driver of its own because a driver listed
    /// elsewhere subsumes it.
    FollowsFrom(&'static str),
}

impl Disposition {
    /// Whether this surface is a recursive walk over the term — i.e. whether it
    /// is in scope for the campaign at all.
    #[inline]
    pub const fn is_traversal(self) -> bool {
        !matches!(self, Disposition::NotATraversal(_))
    }

    /// Whether this surface still needs a driver of its own.
    #[inline]
    pub const fn needs_driver(self) -> bool {
        matches!(self, Disposition::Remaining(_))
    }

    /// The payload — the driver, the stage, or the reason.
    #[inline]
    pub const fn detail(self) -> &'static str {
        match self {
            Disposition::NotATraversal(s)
            | Disposition::Converted(s)
            | Disposition::Remaining(s)
            | Disposition::FollowsFrom(s) => s,
        }
    }
}
