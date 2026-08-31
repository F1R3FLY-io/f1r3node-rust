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

    // All env-var mutation lives in this single test to avoid races with
    // parallel test threads.
    #[test]
    fn env_var_helpers_parse_filter_and_default() {
        assert_eq!(var_parsed::<i32>("SHARED_ENV_TEST_UNSET"), None);
        assert_eq!(var_or("SHARED_ENV_TEST_UNSET", 7i32), 7);
        assert!(var_bool("SHARED_ENV_TEST_UNSET", true));
        assert!(!var_bool("SHARED_ENV_TEST_UNSET", false));

        std::env::set_var("SHARED_ENV_TEST_INT", "42");
        assert_eq!(var_parsed::<i32>("SHARED_ENV_TEST_INT"), Some(42));
        assert_eq!(var_or("SHARED_ENV_TEST_INT", 7i32), 42);

        std::env::set_var("SHARED_ENV_TEST_BAD", "not-a-number");
        assert_eq!(var_parsed::<i32>("SHARED_ENV_TEST_BAD"), None);
        assert_eq!(var_or("SHARED_ENV_TEST_BAD", 7i32), 7);

        std::env::set_var("SHARED_ENV_TEST_NEG", "-3");
        assert_eq!(var_or_filtered("SHARED_ENV_TEST_NEG", 1i64, |v| *v > 0), 1);
        assert_eq!(var_or_filtered("SHARED_ENV_TEST_INT", 1i64, |v| *v > 0), 42);
        assert_eq!(
            var_or_filtered("SHARED_ENV_TEST_UNSET", 1i64, |v| *v > 0),
            1
        );

        for truthy in ["1", "true", "YES", " on "] {
            std::env::set_var("SHARED_ENV_TEST_BOOL", truthy);
            assert!(var_bool("SHARED_ENV_TEST_BOOL", false), "{truthy:?}");
        }
        for falsy in ["0", "false", "No", "OFF"] {
            std::env::set_var("SHARED_ENV_TEST_BOOL", falsy);
            assert!(!var_bool("SHARED_ENV_TEST_BOOL", true), "{falsy:?}");
        }
        std::env::set_var("SHARED_ENV_TEST_BOOL", "maybe");
        assert!(var_bool("SHARED_ENV_TEST_BOOL", true));
        assert!(!var_bool("SHARED_ENV_TEST_BOOL", false));

        for name in [
            "SHARED_ENV_TEST_INT",
            "SHARED_ENV_TEST_BAD",
            "SHARED_ENV_TEST_NEG",
            "SHARED_ENV_TEST_BOOL",
        ] {
            std::env::remove_var(name);
        }
    }
}
