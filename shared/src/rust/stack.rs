/// The deepest nesting a deploy term or a stored value may have. Code outside
/// the interpreter (derived `Clone`, `Drop`, `Hash` and `PartialEq`, serde and
/// prost) recurses over values with no stack checks, so the depth itself has
/// to be bounded.
pub const MAX_NESTING_DEPTH: usize = 128;

/// The deepest synchronous expression evaluation may recurse, and so the
/// deepest a value can nest while an expression is being evaluated.
pub const MAX_EVALUATION_DEPTH: usize = 2 * MAX_NESTING_DEPTH;

/// Stack per nesting level of the costliest walk the generated code makes over
/// a value: derived `Clone`, measured at about 3.7 KiB per level.
const LIBRARY_WALK_BYTES_PER_LEVEL: usize = 4 * 1024;

/// Stack one level of the interpreter's own recursion uses before the next
/// level checks again.
const LEVEL_BYTES: usize = 256 * 1024;

/// Stack every level of a recursive walk over a term or value starts with: enough
/// for its own work plus a library walk over the deepest value it can hold.
const RED_ZONE: usize = MAX_EVALUATION_DEPTH * LIBRARY_WALK_BYTES_PER_LEVEL + LEVEL_BYTES;

/// Stack added when less than [`RED_ZONE`] remains.
const SEGMENT_SIZE: usize = 4 * RED_ZONE;

/// Runs one level of a recursive walk over a term or value, moving to a fresh
/// stack segment first when less than [`RED_ZONE`] remains, so the depth of the
/// walk is bounded by memory rather than by the thread's stack.
pub fn with_stack_headroom<R>(level: impl FnOnce() -> R) -> R {
    stacker::maybe_grow(RED_ZONE, SEGMENT_SIZE, level)
}
