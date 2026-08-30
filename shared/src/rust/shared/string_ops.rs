pub fn wrap_with_braces(expr: String) -> String {
    match expr.parse::<i32>() {
        Ok(_) => expr.clone(),

        Err(_) => {
            if expr.starts_with('(') && expr.ends_with(')') {
                expr.clone()
            } else {
                format!("({})", expr)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_only_unwrapped_non_integer_expressions() {
        assert_eq!(wrap_with_braces("42".into()), "42");
        assert_eq!(wrap_with_braces("(name)".into()), "(name)");
        assert_eq!(wrap_with_braces("name".into()), "(name)");
    }
}
