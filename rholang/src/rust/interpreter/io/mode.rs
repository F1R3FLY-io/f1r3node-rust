//! Fopen-style file-mode parsing.
//!
//! Accepts the nine strings the FIP §"With the config file" table
//! enumerates. Returns a `tokio::fs::OpenOptions` configured to
//! match the semantics. Unknown strings produce `None`; the caller
//! translates to `FSERR_BAD_ARG` at the response layer.

use tokio::fs::OpenOptions;

/// Translate an fopen-style mode string to a `tokio::fs::OpenOptions`.
///
/// Modes per FIP:
///
/// - `"r"`   — read-only; fail if missing.
/// - `"w"`   — write-only; create; truncate.
/// - `"a"`   — write-only; create; append.
/// - `"r+"`  — read+write; fail if missing.
/// - `"w+"`  — read+write; create; truncate.
/// - `"a+"`  — read+write (writes append).
/// - `"wx"`  — write-only; create-exclusive.
/// - `"w+x"` — read+write; create-exclusive.
/// - `"wbx"` — synonym for `"wx"` per §"With the config file" table.
///
/// Any other string returns `None`.
pub fn open_options_for(mode: &str) -> Option<OpenOptions> {
    let mut opts = OpenOptions::new();
    match mode {
        "r" => {
            opts.read(true);
        }
        "w" => {
            opts.write(true).create(true).truncate(true);
        }
        "a" => {
            opts.write(true).create(true).append(true);
        }
        "r+" => {
            opts.read(true).write(true);
        }
        "w+" => {
            opts.read(true).write(true).create(true).truncate(true);
        }
        "a+" => {
            opts.read(true).write(true).create(true).append(true);
        }
        "wx" | "wbx" => {
            opts.write(true).create_new(true);
        }
        "w+x" => {
            opts.read(true).write(true).create_new(true);
        }
        _ => return None,
    }
    Some(opts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn r_opens_existing_and_fails_on_missing() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        assert!(open_options_for("r")
            .unwrap()
            .open(tmp.path())
            .await
            .is_ok());
        let missing = tmp.path().with_extension("missing");
        assert!(open_options_for("r").unwrap().open(&missing).await.is_err());
    }

    #[tokio::test]
    async fn w_creates_and_truncates() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("new.txt");
        // create
        assert!(open_options_for("w").unwrap().open(&path).await.is_ok());
        std::fs::write(&path, b"seed").unwrap();
        // reopen with "w" truncates
        {
            let _f = open_options_for("w").unwrap().open(&path).await.unwrap();
        }
        assert_eq!(std::fs::read(&path).unwrap().len(), 0);
    }

    #[tokio::test]
    async fn wx_fails_when_existing() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        assert!(open_options_for("wx")
            .unwrap()
            .open(tmp.path())
            .await
            .is_err());
    }

    #[test]
    fn unknown_mode_returns_none() {
        assert!(open_options_for("").is_none());
        assert!(open_options_for("rb").is_none());
        assert!(open_options_for("u+x").is_none());
        assert!(open_options_for("0644").is_none());
    }
}
