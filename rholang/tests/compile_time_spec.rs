use std::time::{Duration, Instant};

use rholang::rust::interpreter::compiler::compiler::Compiler;

const WIDTH: usize = 50_000;

/// Far above the linear time these shapes take, and far below the minutes
/// they took when compiling a wide term was superlinear.
const LIMIT: Duration = Duration::from_secs(5);

fn joined(f: impl Fn(usize) -> String, separator: &str) -> String {
    (0..WIDTH).map(f).collect::<Vec<_>>().join(separator)
}

#[test]
fn wide_terms_compile_in_time_linear_in_their_size() {
    let numbers = joined(|i| i.to_string(), " | ");
    let names = joined(|i| format!("x{i}"), ", ");
    let wildcards = joined(|_| "_".to_string(), ", ");
    let entries = joined(|i| format!("{i}: x{i}"), ", ");
    let shapes = [
        ("parallel composition", format!("@0!(0) | {numbers}")),
        ("parallel sends", joined(|i| format!("@0!({i})"), " | ")),
        ("new", format!("new {names} in {{ Nil }}")),
        (
            "match target",
            format!("match {{ {numbers} }} {{ {{ Int | Bool | rest }} => Nil }}"),
        ),
        ("list pattern", format!("match 0 {{ [{names}] => Nil }}")),
        (
            "wildcard pattern",
            format!("match 0 {{ [{wildcards}] => Nil }}"),
        ),
        ("map pattern", format!("match 0 {{ {{{entries}}} => Nil }}")),
        (
            "parallel composition in scope",
            format!("new {names} in {{ {numbers} }}"),
        ),
    ];
    for (shape, source) in shapes {
        let start = Instant::now();
        Compiler::source_to_adt(&source).unwrap();
        let elapsed = start.elapsed();
        assert!(elapsed < LIMIT, "{shape} of {WIDTH} took {elapsed:?}");
    }
}
