//! The family programs of the native suites (decision 47 design v2, section 6; P7).
//!
//! Each program creates a space of one family, draws the names `a` to `e` from it, and runs the
//! parts of its discipline, a same-space join and the part of its outer store. The values on
//! `out` go to the public channel [`OUT`]. `reified_native_spaces.rs` plays them with the
//! `default` qualifier, and the P7 suites play them with `temp`.

use super::native::OUT;

/// The outer stores of the families.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outer {
    HashMap,
    PathMap,
    Array,
    Vector,
}

impl Outer {
    pub fn name(self) -> &'static str {
        match self {
            Outer::HashMap => "hashmap",
            Outer::PathMap => "pathmap",
            Outer::Array => "array",
            Outer::Vector => "vector",
        }
    }

    /// Every outer store with a family program.
    pub const ALL: [Outer; 4] = [Outer::HashMap, Outer::PathMap, Outer::Array, Outer::Vector];

    /// The constructor of the factory call with the qualifier `qualifier`. An array needs its
    /// size (F7).
    pub fn ctor(self, qualifier: &str) -> String {
        match self {
            Outer::Array => format!("\"{qualifier}\"; size=8"),
            Outer::HashMap | Outer::PathMap | Outer::Vector => format!("\"{qualifier}\""),
        }
    }

    /// The part of the program that exercises the outer store, over the name `e`: a persistent
    /// waiter (HashMap), a prefix delivery (PathMap, decision 9.3), or an allocation by `new` in
    /// `use` and a persistent waiter (Array, Vector, F11).
    pub fn part(self) -> &'static str {
        match self {
            Outer::HashMap => r#"for (@v <= e) { out!(["each", v]) } | e!(7)"#,
            Outer::PathMap => {
                r#"use space { @[*e, 1]!(5) | @[*e, 2]!(6) } | for (@v <- e) { out!(["prefix", v]) }"#
            }
            Outer::Array | Outer::Vector => {
                r#"use space { new n in { n!(1) | for (@v <- n) { out!(["alloc", v]) } } } |
for (@v <= e) { out!(["each", v]) } | e!(7)"#
            }
        }
    }

    /// The label of the result that [`Outer::part`] delivers.
    pub fn part_label(self) -> &'static str {
        match self {
            Outer::HashMap | Outer::Array | Outer::Vector => "each",
            Outer::PathMap => "prefix",
        }
    }
}

/// The inner collections of the families.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Inner {
    Bag,
    Queue,
    Stack,
    Set,
    Cell,
    PriorityQueue,
}

impl Inner {
    pub const ALL: [Inner; 6] = [
        Inner::Bag,
        Inner::Queue,
        Inner::Stack,
        Inner::Set,
        Inner::Cell,
        Inner::PriorityQueue,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Inner::Bag => "bag",
            Inner::Queue => "queue",
            Inner::Stack => "stack",
            Inner::Set => "set",
            Inner::Cell => "cell",
            Inner::PriorityQueue => "priorityqueue",
        }
    }

    /// The part of the program that exercises the discipline, over the names `a` to `d`: the
    /// runtime programs of the determinism suite (N9), and a same-space join of `c` and `d`.
    pub fn part(self) -> &'static str {
        match self {
            Inner::Bag => {
                r#"a!(3) | a!(1) | a!(2) | a!(1) | b!!(7) |
for (@x <- a) { out!(["take", x]) } |
for (@1 <- a) { out!(["one", 1]) } |
for (@y <<- a) { out!(["peek", y]) } |
for (@z <- b) { out!(["persistent", z]) } |
for (@w <<- b) { out!(["peek persistent", w]) }"#
            }
            Inner::Queue | Inner::Stack => {
                r#"a!(1) | a!(2) | a!(3) |
for (@x <- a) { out!(["take", x]) } |
for (@y <<- a) { out!(["peek", y]) } |
for (@2 <- b) { out!(["two", 2]) } | b!(1) | b!(2)"#
            }
            Inner::Set => {
                r#"a!(3) | a!(1) | a!(2) | a!(1) | b!!(7) |
for (@1 <- a) { out!(["class", 1]) } |
for (@x <- a) { out!(["take", x]) } |
for (@y <<- a) { out!(["peek", y]) } |
for (@z <- b) { out!(["persistent", z]) }"#
            }
            Inner::Cell => {
                r#"a!(1) | for (@x <- a) { out!(["take", x]) | a!(x + 1) } |
for (@y <<- b) { out!(["peek", y]) } | b!(5)"#
            }
            Inner::PriorityQueue => {
                r#"a!(1; priority=1) | a!(2; priority=0) | a!(3) |
for (@x <- a) { out!(["take", x]) } |
for (@y <<- a) { out!(["peek", y]) } |
for (@z <- a ~ (; priority=1)) { out!(["lane", z]) } |
for (@2 <- b ~ (; priority=0)) { out!(["two", 2]) } | b!(1) | b!(2; priority=0)"#
            }
        }
    }
}

/// The join of `c` and `d` that every family program runs.
pub const JOIN: &str = r#"for (@p <- c & @q <- d) { out!(["join", p, q]) } | c!(5) | d!(6)"#;

/// A deploy that creates a space of the family `urn` with the constructor `ctor`, draws the
/// names `a` to `e` from it, and runs `body`. The values on `out` go to the public channel
/// [`OUT`].
pub fn space_program(urn: &str, ctor: &str, body: &str) -> String {
    format!(
        r#"new factory(`{urn}`), out in {{
  for (space <- factory!?({ctor})) {{
    for (a <- space!?("gensym")) {{
      for (b <- space!?("gensym")) {{
        for (c <- space!?("gensym")) {{
          for (d <- space!?("gensym")) {{
            for (e <- space!?("gensym")) {{
              {body}
            }}
          }}
        }}
      }}
    }}
  }} |
  for (@value <= out) {{ @"{OUT}"!(value) }}
}}"#
    )
}

/// The program of the family `outer:inner` with the `default` qualifier.
pub fn program(outer: Outer, inner: Inner) -> String { program_with(outer, inner, "default") }

/// The program of the family `outer:inner` with the qualifier `qualifier` (P7: `temp`).
pub fn program_with(outer: Outer, inner: Inner, qualifier: &str) -> String {
    space_program(
        &format!("rho:space:{}:{}", outer.name(), inner.name()),
        &outer.ctor(qualifier),
        &format!("{} |\n{} |\n{}", inner.part(), JOIN, outer.part()),
    )
}

/// A family whose state outlives its deploy (P7, design v3 W6). Block 1 creates a space of the
/// family, leaves records on `a` and a waiter on `b`, and stores the handle and both names on a
/// public channel. Block 2 calls the handle method `config` and touches the stored state: it
/// takes a record, fires the waiter, and stores a record. The values on `out` go to the public
/// channel [`OUT`] as pairs `[label, value]`.
pub struct StoredFamily {
    /// The label of the family in the outputs and the stash channel.
    pub label: &'static str,
    /// The factory URN.
    pub urn: &'static str,
    /// The constructor of the factory call.
    pub ctor: &'static str,
    /// The records and the waiter of block 1, over `space`, `a` and `b`.
    pub state: &'static str,
    /// The operations of block 2, over `space`, `a` and `b`. They deliver `take` and `fired`.
    pub touch: &'static str,
}

impl StoredFamily {
    /// The public channel that holds the handle and the names `a` and `b`.
    pub fn stash(&self) -> String { format!("rrs-p7-stored-{}", self.label) }

    /// Block 1: the creation of the space, its state, and the stash.
    pub fn creation(&self) -> String {
        format!(
            r#"new factory(`{urn}`), out in {{
  for (space <- factory!?({ctor})) {{
    for (a <- space!?("gensym")) {{
      for (b <- space!?("gensym")) {{
        @"{stash}"!(*space, *a, *b) | {state}
      }}
    }}
  }} |
  for (@value <= out) {{ @"{OUT}"!(["{label}", value]) }}
}}"#,
            urn = self.urn,
            ctor = self.ctor,
            stash = self.stash(),
            state = self.state,
            label = self.label,
        )
    }

    /// Block 2: the handle call `config` and the operations of [`StoredFamily::touch`] over the
    /// stored handle and names.
    pub fn touching(&self) -> String {
        self.with_stash(&format!("{} | {}", CONFIG_CALL, self.touch))
    }

    /// A deploy that runs `body` over the stored handle `space` and the names `a` and `b`, with
    /// the values on `out` forwarded to [`OUT`] under the label of the family.
    pub fn with_stash(&self, body: &str) -> String {
        format!(
            r#"new out in {{
  for (space, a, b <<- @"{stash}") {{
    {body}
  }} |
  for (@value <= out) {{ @"{OUT}"!(["{label}", value]) }}
}}"#,
            stash = self.stash(),
            label = self.label,
        )
    }
}

/// The handle call `config` of block 2, which delivers `config` with the config map.
pub const CONFIG_CALL: &str = r#"for (@config <- space!?("config")) { out!(["config", config]) }"#;

/// The families of the restart and rehydration suites: a queue, a set, a priority queue, a
/// VectorDB space and a PathMap space (design v3 section 10, `reified_restart_rehydrate.rs`).
pub const STORED_FAMILIES: [StoredFamily; 5] = [
    StoredFamily {
        label: "queue",
        urn: "rho:space:hashmap:queue",
        ctor: r#""default""#,
        state: r#"a!(1) | a!(2) | a!(3) | for (@2 <- b) { out!(["fired", 2]) }"#,
        touch: r#"for (@x <- a) { out!(["take", x]) | a!(x + 10) } | b!(2)"#,
    },
    StoredFamily {
        label: "set",
        urn: "rho:space:hashmap:set",
        ctor: r#""default""#,
        state: r#"a!(1) | a!(1) | a!(2) | for (@3 <- b) { out!(["fired", 3]) }"#,
        touch: r#"for (@1 <- a) { out!(["take", 1]) | a!(11) } | b!(3)"#,
    },
    StoredFamily {
        label: "priorityqueue",
        urn: "rho:space:hashmap:priorityqueue",
        ctor: r#""default""#,
        state: r#"a!(1; priority=1) | a!(2; priority=0) | a!(3) |
for (@z <- b ~ (; priority=0)) { out!(["fired", z]) }"#,
        touch: r#"for (@x <- a) { out!(["take", x]) | a!(x + 10; priority=1) } | b!(9; priority=0)"#,
    },
    StoredFamily {
        label: "vectordb",
        urn: "rho:space:hashmap:vectordb",
        ctor: r#""default"; capacity=16, dimensions=2"#,
        state: r#"a!([1.0, 0.0]) | a!([0.6, 0.8]) |
for (@w <- b ~ ([0.0, 1.0]; threshold=0.9)) { out!(["fired", w]) }"#,
        touch: r#"for (@x <- a ~ ([1.0, 0.0])) { out!(["take", x]) | a!([0.0, 2.0]) } | b!([0.0, 1.0])"#,
    },
    StoredFamily {
        label: "pathmap",
        urn: "rho:space:pathmap:bag",
        ctor: r#""default""#,
        state: r#"use space { @[*a, 1]!(5) | @[*a, 2]!(6) } | for (@v <- b) { out!(["fired", v]) }"#,
        touch: r#"for (@v <- a) { out!(["take", v]) } | use space { @[*b, 3]!(7) }"#,
    },
];

// ─── The HashSet families and the reference agent (P9) ─────────────────────────────────────────

/// The inner collections of the six HashSet families, in family-index order.
pub const HASHSET_INNERS: [&str; 6] = ["bag", "queue", "stack", "set", "cell", "priorityqueue"];

/// The body of [`hashset_program`] that adds one unit to `x` through the handle and takes it.
pub const HASHSET_UNIT_TAKE: &str = r#"S!?("produce", *x, []); for ( <- x) { out!(["took", 1]) }"#;

/// A deploy that creates a `seq` space `S` of the family `hashset:inner`, draws the names `x`
/// and `y` of `S`, and runs `body` (`reified_hashset.rs`). The values on `out` go to the public
/// channel [`OUT`].
pub fn hashset_program(inner: &str, body: &str) -> String {
    format!(
        r#"new factory(`rho:space:hashset:{inner}`), out in {{
  for (S <- factory!?("seq")) {{
    for (x <- S!?("gensym"); y <- S!?("gensym")) {{
      {body}
    }}
  }} |
  for (@value <= out) {{ @"{OUT}"!(value) }}
}}"#
    )
}

/// The reference agent: the example program of the repository.
pub const REFERENCE_AGENT: &str =
    include_str!("../../../examples/reified_rspaces/agent_hashmap_bag.rho");

/// The comment that starts the demo of the reference agent. The kit runs the definitions before
/// it, without the demo.
pub const DEMO_MARKER: &str = "  // Demo:";

/// The deploy that creates one instance of the reference agent and publishes it on
/// `@"kit-agent"`, as the conformance kit does (`reified_agent_conformance.rs`).
pub fn reference_agent_creation() -> String {
    let (definitions, _) = REFERENCE_AGENT
        .split_once(DEMO_MARKER)
        .expect("the reference agent marks its demo");
    format!(
        "{definitions}  for (space <- agentHashmapBag!?(\"default\")) {{ @\"kit-agent\"!(*space) }}\n}}\n"
    )
}

/// The deploy that draws the names `x` and `y` from the agent space and publishes them on
/// `@"kit-names"`, as the conformance kit does.
pub const KIT_NAMES: &str = r#"new names in {
  for (space <<- @"kit-agent") { use space { new x, y in { names!([*x, *y]) } } } |
  for (@ns <- names) { @"kit-names"!(ns) }
}"#;

/// `body` with the names of the agent space bound to `x` and `y`.
pub fn with_kit_names(body: &str) -> String {
    format!("for (@[x, y] <<- @\"kit-names\") {{\n{body}\n}}")
}

/// The six deploys of a run of the conformance kit: the creation of the agent, its names, a send,
/// a take, a parked peek and a send that wakes the peek. The values go to [`OUT`].
pub fn kit_deploys() -> Vec<String> {
    vec![
        reference_agent_creation(),
        KIT_NAMES.to_string(),
        with_kit_names("@x!(1, 2)"),
        with_kit_names(&format!("for (@a, @b <- @x) {{ @\"{OUT}\"!([a, b]) }}")),
        with_kit_names(&format!("for (@a <<- @y) {{ @\"{OUT}\"!(a) }}")),
        with_kit_names("@y!(3)"),
    ]
}
