use std::str::FromStr;

pub fn var_parsed<T: FromStr>(name: &str) -> Option<T> {
    std::env::var(name).ok()?.parse::<T>().ok()
}

pub fn var_or<T: FromStr + Copy>(name: &str, default: T) -> T {
    var_parsed(name).unwrap_or(default)
}

pub fn var_or_filtered<T: FromStr + Copy>(
    name: &str,
    default: T,
    predicate: impl Fn(&T) -> bool,
) -> T {
    var_parsed(name).filter(predicate).unwrap_or(default)
}

pub fn var_bool(name: &str, default: bool) -> bool {
    match std::env::var(name) {
        Ok(value) => match value.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => true,
            "0" | "false" | "no" | "off" => false,
            _ => default,
        },
        Err(_) => default,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(suffix: &str) -> String {
        format!("F1R3NODE_SHARED_ENV_TEST_{}_{}", std::process::id(), suffix)
    }

    #[test]
    fn parses_values_and_uses_defaults() {
        let number = name("NUMBER");
        let missing = name("MISSING");
        std::env::set_var(&number, "42");
        std::env::remove_var(&missing);

        assert_eq!(var_parsed::<u32>(&number), Some(42));
        assert_eq!(var_or(&number, 7), 42);
        assert_eq!(var_or(&missing, 7), 7);
        assert_eq!(var_or_filtered(&number, 7, |value| *value > 40), 42);
        assert_eq!(var_or_filtered(&number, 7, |value| *value < 40), 7);

        std::env::set_var(&number, "invalid");
        assert_eq!(var_parsed::<u32>(&number), None);
        std::env::remove_var(number);
    }

    #[test]
    fn parses_boolean_spellings() {
        let variable = name("BOOL");
        for value in ["1", "true", "YES", " on "] {
            std::env::set_var(&variable, value);
            assert!(var_bool(&variable, false));
        }
        for value in ["0", "false", "NO", " off "] {
            std::env::set_var(&variable, value);
            assert!(!var_bool(&variable, true));
        }

        std::env::set_var(&variable, "invalid");
        assert!(var_bool(&variable, true));
        assert!(!var_bool(&variable, false));
        std::env::remove_var(&variable);
        assert!(var_bool(&variable, true));
    }
}
