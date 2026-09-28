use std::fs;
use std::path::Path;

fn without_comments(source: &str) -> String {
    let mut code = String::with_capacity(source.len());
    let mut rest = source;
    while let Some(start) = rest.find("/*") {
        code.push_str(&rest[..start]);
        rest = rest[start..]
            .find("*/")
            .map_or("", |end| &rest[start + end + 2..]);
    }
    code.push_str(rest);
    code.lines()
        .map(|line| line.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

/// A `_deposit` send reaches a vault only after `findOrCreate` installed its
/// contract, so a production contract that uses `_deposit` must also call
/// `findOrCreate`. Comments are ignored on both sides.
#[test]
fn production_contracts_with_deposit_also_call_find_or_create() {
    let resources = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/main/resources");
    let mut checked = 0;
    for entry in fs::read_dir(&resources).unwrap() {
        let path = entry.unwrap().path();
        let is_rholang = path
            .extension()
            .is_some_and(|ext| ext == "rho" || ext == "rhox");
        if !is_rholang {
            continue;
        }
        let code = without_comments(&fs::read_to_string(&path).unwrap());
        if code.contains("_deposit") {
            checked += 1;
            assert!(
                code.contains("findOrCreate"),
                "{} uses _deposit without findOrCreate",
                path.display()
            );
        }
    }
    assert!(checked > 0, "no production contract uses _deposit");
}

#[test]
fn comments_do_not_count_as_code() {
    let code = without_comments("a /* _deposit */ b // findOrCreate\nc");
    assert_eq!(code, "a  b \nc");
}
