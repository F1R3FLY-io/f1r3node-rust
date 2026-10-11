use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use rholang::rust::interpreter::io::bulk::hex_root;
use rholang::rust::interpreter::io::snapshot_chunk::{
    chunk_snapshot, snapshot_merkle_root, CHUNK_SIZE,
};

use crate::error::BulkError;

fn read_full(f: &mut File, buf: &mut [u8], path: &Path) -> Result<usize, BulkError> {
    let mut filled = 0;
    while filled < buf.len() {
        let n = f
            .read(&mut buf[filled..])
            .map_err(|e| BulkError::io(path, e))?;
        if n == 0 {
            break;
        }
        filled += n;
    }
    Ok(filled)
}

pub fn chunk_hashes(path: &Path) -> Result<Vec<[u8; 32]>, BulkError> {
    let mut f = File::open(path).map_err(|e| BulkError::io(path, e))?;
    let mut buf = vec![0u8; CHUNK_SIZE];
    let mut hashes = Vec::new();
    loop {
        let n = read_full(&mut f, &mut buf, path)?;
        if n == 0 {
            break;
        }
        hashes.push(chunk_snapshot(&buf[..n])[0].hash);
        if n < CHUNK_SIZE {
            break;
        }
    }
    Ok(hashes)
}

pub fn source_root(path: &Path) -> Result<[u8; 32], BulkError> {
    Ok(snapshot_merkle_root(&chunk_hashes(path)?))
}

fn download(url: &str, dest: &Path) -> Result<(), BulkError> {
    let mut resp =
        reqwest::blocking::get(url).map_err(|e| BulkError::Source(format!("{url}: {e}")))?;
    if !resp.status().is_success() {
        return Err(BulkError::Source(format!("{url}: HTTP {}", resp.status())));
    }
    let tmp = dest.with_extension("part");
    let mut out = File::create(&tmp).map_err(|e| BulkError::io(&tmp, e))?;
    resp.copy_to(&mut out)
        .map_err(|e| BulkError::Source(format!("{url}: {e}")))?;
    out.flush().map_err(|e| BulkError::io(&tmp, e))?;
    out.sync_all().map_err(|e| BulkError::io(&tmp, e))?;
    fs::rename(&tmp, dest).map_err(|e| BulkError::io(dest, e))
}

pub fn fetch_verified(
    locations: &[String],
    expected: &[u8; 32],
    cache: &Path,
) -> Result<PathBuf, BulkError> {
    fs::create_dir_all(cache).map_err(|e| BulkError::io(cache, e))?;
    let cached = cache.join(format!("{}.src", hex_root(expected)));
    if cached.exists() && source_root(&cached)? == *expected {
        return Ok(cached);
    }
    let mut failures = Vec::new();
    for loc in locations {
        let attempt = if let Some(p) = loc.strip_prefix("file://") {
            let p = Path::new(p);
            fs::copy(p, &cached)
                .map(|_| ())
                .map_err(|e| BulkError::io(p, e))
        } else if loc.starts_with("https://") {
            download(loc, &cached)
        } else {
            Err(BulkError::Source(format!("unsupported location {loc}")))
        };
        match attempt {
            Ok(()) => {
                let got = source_root(&cached)?;
                if got == *expected {
                    return Ok(cached);
                }
                let _ = fs::remove_file(&cached);
                failures.push(format!("{loc}: root {} does not match", hex_root(&got)));
            }
            Err(e) => failures.push(e.to_string()),
        }
    }
    Err(BulkError::Source(format!(
        "no location produced the expected source root: {}",
        failures.join("; ")
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_matches_in_memory_chunking() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("s");
        let data: Vec<u8> = (0..(CHUNK_SIZE * 2 + 17))
            .map(|i| (i % 251) as u8)
            .collect();
        fs::write(&p, &data).unwrap();
        let expected: Vec<[u8; 32]> = chunk_snapshot(&data).iter().map(|c| c.hash).collect();
        assert_eq!(chunk_hashes(&p).unwrap(), expected);
        assert_eq!(source_root(&p).unwrap(), snapshot_merkle_root(&expected));
    }

    #[test]
    fn empty_source_has_sentinel_root() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("s");
        fs::write(&p, b"").unwrap();
        assert_eq!(source_root(&p).unwrap(), [0u8; 32]);
    }

    #[test]
    fn fetch_verifies_and_rejects_wrong_root() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("data.csv");
        fs::write(&p, b"id\n1\n").unwrap();
        let root = source_root(&p).unwrap();
        let loc = vec![format!("file://{}", p.display())];
        let got = fetch_verified(&loc, &root, &d.path().join("cache")).unwrap();
        assert_eq!(fs::read(got).unwrap(), b"id\n1\n");
        assert!(fetch_verified(&loc, &[7u8; 32], &d.path().join("cache2")).is_err());
    }
}
