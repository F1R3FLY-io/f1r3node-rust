//! The three blocks of the P9 suites over native space state (P9 design v2, sections 6 and 9).
//!
//! Block 1 creates a durable space of each of the 26 families, the 24 outer and inner families
//! and the two VectorDB families, and an agent space with the reference agent. Each family draws
//! the names `a` to `d`, stores records on `a`, parks a waiter on `b`, and stores its handle and
//! names on a public channel. Block 2 touches every space of block 1, and each of its family
//! deploys also creates and ends a `temp` space and a `seq` space. Block 3 runs joins, PathMap
//! prefix deliveries and agent calls. The replay across blocks (`reified_cross_process.rs`) plays
//! the three blocks, and the LFS suite (`reified_lfs.rs`) exports the root of block 1 and plays
//! block 2 on the imported store.
//!
//! Each family forwards its deliveries to a channel of its own, and each agent deploy makes one
//! call of the agent. Native replay charges loads that grow with the values on a channel, and
//! play does not (decision 79, finding M-47-1). With one shared channel, the 48th deploy, a deploy
//! of block 2, needed more than the v6 limits in replay (verification.md, section 34.4.3).

use super::family_programs::{reference_agent_creation, with_kit_names, Inner, Outer, KIT_NAMES};
use super::native::AMPLE_LIMIT;

/// The phlo limit of the deploys that create a HashSet space (`reified_hashset.rs`).
pub const HASHSET_LIMIT: u64 = 20 * AMPLE_LIMIT;

/// The constructor of the VectorDB spaces.
pub const VECTORDB_CTOR: &str = r#""default"; capacity=16, dimensions=2"#;

/// A deploy of a set: its label, which seeds its random state, its term and its phlo limit.
pub struct Deploy {
    pub label: String,
    pub term: String,
    pub limit: u64,
}

impl Deploy {
    pub fn new(label: impl Into<String>, term: impl Into<String>, limit: u64) -> Self {
        Self {
            label: label.into(),
            term: term.into(),
            limit,
        }
    }
}

/// A family of the blocks: its label, its factory, its constructor, and the parts of its three
/// blocks over the names `a` to `d` of its space.
pub struct BlockFamily {
    pub label: String,
    pub urn: String,
    pub ctor: String,
    /// Block 1: records on `a` and a waiter on `b`.
    pub state: String,
    /// Block 2: a take of a record of `a`, a store, and a send that fires the waiter on `b`.
    pub touch: String,
    /// Block 3: a join of `c` and `d`, and a prefix delivery for a PathMap space.
    pub join: String,
}

/// The families of the blocks: the 24 outer and inner families and the two VectorDB families.
pub fn block_families() -> Vec<BlockFamily> {
    let mut families = Vec::with_capacity(26);
    for outer in Outer::ALL {
        for inner in Inner::ALL {
            let records = match inner {
                Inner::Cell => "a!(1)",
                Inner::Bag | Inner::Queue | Inner::Stack | Inner::Set | Inner::PriorityQueue => {
                    "a!(1) | a!(2)"
                }
            };
            let prefix = match outer {
                Outer::PathMap => {
                    r#" | use space { @[*c, 9]!(7) } | for (@v <- c) { out!(["prefix", v]) }"#
                }
                Outer::HashMap | Outer::Array | Outer::Vector => "",
            };
            families.push(BlockFamily {
                label: format!("{}:{}", outer.name(), inner.name()),
                urn: format!("rho:space:{}:{}", outer.name(), inner.name()),
                ctor: outer.ctor("default"),
                state: format!(r#"{records} | for (@v <- b) {{ out!(["fired", v]) }}"#),
                touch: r#"for (@x <- a) { out!(["take", x]) | a!(x + 10) } | b!(3)"#.to_string(),
                join: format!(
                    r#"for (@p <- d & @q <- a) {{ out!(["join", p, q]) }} | d!(5){prefix}"#
                ),
            });
        }
    }
    for outer in ["hashmap", "vector"] {
        families.push(BlockFamily {
            label: format!("{outer}:vectordb"),
            urn: format!("rho:space:{outer}:vectordb"),
            ctor: VECTORDB_CTOR.to_string(),
            state: r#"a!([1.0, 0.0]) | a!([0.6, 0.8]) |
for (@w <- b ~ ([0.0, 1.0]; threshold=0.9)) { out!(["fired", w]) }"#
                .to_string(),
            touch: r#"for (@x <- a ~ ([1.0, 0.0])) { out!(["take", x]) | a!([0.0, 2.0]) } | b!([0.0, 1.0])"#
                .to_string(),
            join: r#"for (@p <- d & @q <- a) { out!(["join", p, q]) } | d!([1.0, 1.0])"#
                .to_string(),
        });
    }
    families
}

/// The public channel that holds the handle and the names of the space of `family`.
pub fn stash(family: &BlockFamily) -> String { format!("rrs-p9-replay-{}", family.label) }

/// The public channel of the deliveries of the deploys of `label`. Each family has its own
/// channel, so no channel grows with the length of the chain (the module docs).
pub fn deliveries(label: &str) -> String { format!("rrs-p9-replay-out-{label}") }

/// A deploy that runs `body` over the stored handle `space` and the names `a` to `d` of
/// `family`, with the values on `out` forwarded to the channel of the family ([`deliveries`]).
pub fn with_stash(family: &BlockFamily, body: &str) -> String {
    format!(
        r#"new out in {{
  for (space, a, b, c, d <<- @"{stash}") {{
    {body}
  }} |
  for (@value <= out) {{ @"{channel}"!(value) }}
}}"#,
        stash = stash(family),
        channel = deliveries(&family.label),
    )
}

/// The channel of the deliveries of the agent deploys. Each agent deploy makes one call of the
/// agent, as each step of the conformance kit does: native replay charges more host work than
/// play for a call of a stored contract (decision 79), and two calls of the reference agent in
/// one deploy need more than the v6 limits in replay.
pub const AGENT_OUT: &str = "rrs-p9-replay-out-agent";

/// The `temp` space and the `seq` space that each family deploy of block 2 creates and ends.
pub const EPHEMERAL: &str = r#"new tf(`rho:space:hashmap:queue`), sf(`rho:space:hashset:bag`) in {
  for (t <- tf!?("temp")) { for (n <- t!?("gensym")) { n!(1) | for (@z <- n) { out!(["temp", z]) } } } |
  for (S <- sf!?("seq")) { for (u <- S!?("gensym")) { S!?("produce", *u, []); for ( <- u) { out!(["seq", 1]) } } }
}"#;

/// The three blocks, in order: 30, 28 and 28 deploys.
pub fn blocks() -> [Vec<Deploy>; 3] {
    let families = block_families();
    let mut first = Vec::with_capacity(families.len() + 4);
    for family in &families {
        first.push(Deploy::new(
            format!("block 1 {}", family.label),
            format!(
                r#"new factory(`{urn}`), out in {{
  for (space <- factory!?({ctor})) {{
    for (a <- space!?("gensym")) {{ for (b <- space!?("gensym")) {{
      for (c <- space!?("gensym")) {{ for (d <- space!?("gensym")) {{
        @"{stash}"!(*space, *a, *b, *c, *d) | {state}
      }} }}
    }} }}
  }} |
  for (@value <= out) {{ @"{channel}"!(value) }}
}}"#,
                urn = family.urn,
                ctor = family.ctor,
                stash = stash(family),
                state = family.state,
                channel = deliveries(&family.label),
            ),
            AMPLE_LIMIT,
        ));
    }
    first.push(Deploy::new(
        "block 1 agent",
        reference_agent_creation(),
        AMPLE_LIMIT,
    ));
    first.push(Deploy::new("block 1 agent names", KIT_NAMES, AMPLE_LIMIT));
    first.push(Deploy::new(
        "block 1 agent message",
        with_kit_names("@x!(1, 2)"),
        AMPLE_LIMIT,
    ));
    first.push(Deploy::new(
        "block 1 agent waiter",
        with_kit_names(&format!(
            r#"for (@a, @b <- @y) {{ @"{AGENT_OUT}"!(["agent fired", a, b]) }}"#
        )),
        AMPLE_LIMIT,
    ));
    let mut second = Vec::with_capacity(families.len() + 2);
    for family in &families {
        second.push(Deploy::new(
            format!("block 2 {}", family.label),
            with_stash(family, &format!("{} | {EPHEMERAL}", family.touch)),
            HASHSET_LIMIT,
        ));
    }
    second.push(Deploy::new(
        "block 2 agent wake",
        with_kit_names("@y!(3, 4)"),
        AMPLE_LIMIT,
    ));
    second.push(Deploy::new(
        "block 2 agent take",
        with_kit_names(&format!(
            r#"for (@a, @b <- @x) {{ @"{AGENT_OUT}"!(["agent take", a, b]) }}"#
        )),
        AMPLE_LIMIT,
    ));
    let mut third = Vec::with_capacity(families.len() + 2);
    for family in &families {
        third.push(Deploy::new(
            format!("block 3 {}", family.label),
            with_stash(family, &family.join),
            AMPLE_LIMIT,
        ));
    }
    third.push(Deploy::new(
        "block 3 agent call",
        with_kit_names("@x!(5, 6)"),
        AMPLE_LIMIT,
    ));
    third.push(Deploy::new(
        "block 3 agent take",
        with_kit_names(&format!(
            r#"for (@a, @b <- @x) {{ @"{AGENT_OUT}"!(["agent call", a, b]) }}"#
        )),
        AMPLE_LIMIT,
    ));
    [first, second, third]
}
