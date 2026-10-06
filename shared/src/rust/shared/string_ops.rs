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
    fn integer_literals_pass_through_unwrapped() {
        assert_eq!(wrap_with_braces("42".to_string()), "42");
        assert_eq!(wrap_with_braces("-7".to_string()), "-7");
    }

    #[test]
    fn already_parenthesized_expressions_pass_through() {
        assert_eq!(wrap_with_braces("(a + b)".to_string()), "(a + b)");
    }

    #[test]
    fn other_expressions_are_wrapped() {
        assert_eq!(wrap_with_braces("a + b".to_string()), "(a + b)");
        assert_eq!(wrap_with_braces("(a) + (b".to_string()), "((a) + (b)");
        assert_eq!(wrap_with_braces("".to_string()), "()");
    }
}
