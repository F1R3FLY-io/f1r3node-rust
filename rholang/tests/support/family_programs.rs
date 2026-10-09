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
