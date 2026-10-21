use std::io;
use std::process::Output;

pub fn resolve_build_id(
    explicit: Option<&str>,
    git: impl FnOnce() -> io::Result<Output>,
) -> Result<String, &'static str> {
    if let Some(value) = explicit {
        let identity = value.trim();
        if !identity.is_empty() {
            if value.chars().any(char::is_control) {
                return Err("F1R3NODE_BUILD_ID must not contain control characters");
            }
            return Ok(identity.to_owned());
        }
    }

    let identity = git()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty() && !value.chars().any(char::is_control));

    Ok(identity.unwrap_or_else(|| "unknown".to_owned()))
}
