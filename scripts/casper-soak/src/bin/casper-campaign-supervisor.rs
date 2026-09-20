#[path = "../campaign_control/mod.rs"]
pub mod campaign_control;

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::time::Duration;

use casper_soak::{encoded, parse, record, text, MAX_BYTES};
use eyre::{ensure, eyre, Result};
use serde_json::{json, Value};

fn invoke(config: &campaign_control::Config, input: &Value, evidence: &Path) -> Result<Value> {
    let mut provider = campaign_control::transport::Cli::new(config.clone(), evidence)?;
    if input["action"] == "arm" {
        campaign_control::keys(input, &["action", "slot", "reservation_id"])?;
        campaign_control::operations::arm(
            &mut provider,
            config,
            text(&input["slot"])?,
            text(&input["reservation_id"])?,
        )
    } else {
        ensure!(
            input.as_object().is_some_and(|o| o.is_empty()),
            "Scheduled invocations must not supply deadlines or resource identities."
        );
        campaign_control::operations::supervise(&mut provider, config)
    }
}

fn body<R: BufRead>(reader: &mut R) -> Result<Vec<u8>> {
    let mut line = String::new();
    reader.read_line(&mut line)?;
    ensure!(
        line == "POST /call HTTP/1.1\r\n" || line == "POST / HTTP/1.1\r\n",
        "The function request method or path is unsupported."
    );
    let mut length = None;
    let mut chunked = false;
    let mut header_bytes = line.len();
    loop {
        line.clear();
        ensure!(
            reader.take(8193).read_line(&mut line)? > 0,
            "The function headers are truncated."
        );
        header_bytes += line.len();
        ensure!(
            header_bytes <= 16384,
            "The function headers exceed their bound."
        );
        if line == "\r\n" {
            break;
        }
        let (key, value) = line
            .split_once(':')
            .ok_or_else(|| eyre!("The function header is malformed."))?;
        if key.eq_ignore_ascii_case("content-length") {
            ensure!(length.is_none(), "The function length is duplicated.");
            length = Some(value.trim().parse::<u64>()?);
        }
        if key.eq_ignore_ascii_case("transfer-encoding") {
            ensure!(
                !chunked && value.trim().eq_ignore_ascii_case("chunked"),
                "The transfer encoding is unsupported."
            );
            chunked = true;
        }
    }
    ensure!(
        !(chunked && length.is_some()),
        "The function framing is ambiguous."
    );
    let mut bytes = Vec::new();
    if chunked {
        loop {
            line.clear();
            reader.take(128).read_line(&mut line)?;
            let size = u64::from_str_radix(line.trim(), 16)?;
            ensure!(
                bytes.len() as u64 + size <= MAX_BYTES,
                "The function payload exceeds its bound."
            );
            if size == 0 {
                let mut ending = [0; 2];
                reader.read_exact(&mut ending)?;
                ensure!(ending == *b"\r\n", "Function trailers are unsupported.");
                break;
            }
            let start = bytes.len();
            bytes.resize(start + size as usize, 0);
            reader.read_exact(&mut bytes[start..])?;
            let mut ending = [0; 2];
            reader.read_exact(&mut ending)?;
            ensure!(ending == *b"\r\n", "The function chunk is malformed.");
        }
    } else {
        let size = length.ok_or_else(|| eyre!("The function length is missing."))?;
        ensure!(size <= MAX_BYTES, "The function payload exceeds its bound.");
        bytes.resize(size as usize, 0);
        reader.read_exact(&mut bytes)?;
    }
    Ok(bytes)
}

fn run() -> Result<()> {
    let path = std::env::var("CASPER_CAMPAIGN_CONFIG")?;
    let config = campaign_control::Config::new(record(Path::new(&path))?)?;
    ensure!(
        std::env::var("CASPER_CAMPAIGN_CONFIG_SHA256")
            .ok()
            .as_deref()
            == Some(config.digest.as_str()),
        "The supervisor configuration differs from its trusted pin."
    );
    ensure!(
        std::env::var("OCI_CLI_AUTH").ok().as_deref() == Some("resource_principal"),
        "The supervisor requires its OCI resource principal."
    );
    let listener = std::env::var("FN_LISTENER")?;
    let socket = listener
        .strip_prefix("unix:")
        .ok_or_else(|| eyre!("The function listener must use a Unix socket."))?;
    let listener = UnixListener::bind(socket)?;
    for stream in listener.incoming() {
        let mut stream = stream?;
        stream.set_read_timeout(Some(Duration::from_secs(15)))?;
        stream.set_write_timeout(Some(Duration::from_secs(15)))?;
        let evidence = tempfile::tempdir()?;
        let result = (|| -> Result<Value> {
            let mut reader = BufReader::new(stream.try_clone()?);
            let bytes = body(&mut reader)?;
            let input = if bytes.is_empty() {
                json!({})
            } else {
                parse(&bytes)?
            };
            invoke(&config, &input, evidence.path())
        })();
        let (status, value) = match result {
            Ok(value) => ("200 OK", value),
            Err(_) => (
                "500 Internal Server Error",
                json!({"termination_confirmed":false,"result":"infrastructure_failure"}),
            ),
        };
        let bytes = encoded(&value)?;
        write!(stream,"HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",bytes.len())?;
        stream.write_all(&bytes)?;
        stream.flush()?;
        println!(
            "{}",
            json!({"scope":"oci-lifetime-supervisor","status":status,"result":value})
        );
    }
    Ok(())
}

fn main() {
    if run().is_err() {
        eprintln!("The OCI lifetime supervisor failed. Termination is not confirmed.");
        std::process::exit(2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_body_accepts_bounded_fixed_and_chunked_payloads() {
        let mut fixed = &b"POST /call HTTP/1.1\r\nContent-Length: 2\r\n\r\n{}"[..];
        assert_eq!(body(&mut fixed).unwrap(), b"{}");
        let mut chunked =
            &b"POST /call HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n2\r\n{}\r\n0\r\n\r\n"[..];
        assert_eq!(body(&mut chunked).unwrap(), b"{}");
    }

    #[test]
    fn http_body_rejects_ambiguous_or_truncated_framing() {
        for input in [
            "POST /call HTTP/1.1\r\nContent-Length: 2\r\nContent-Length: 2\r\n\r\n{}",
            "POST /call HTTP/1.1\r\nContent-Length: 2\r\nTransfer-Encoding: chunked\r\n\r\n{}",
            "POST /call HTTP/1.1\r\nContent-Length: 1048577\r\n\r\n",
            "POST /call HTTP/1.1\r\nContent-Length: 2\r\n\r\n{",
            "GET /call HTTP/1.1\r\nContent-Length: 0\r\n\r\n",
            "POST /call HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n2\r\n{}\r\n0\r\nX: y\r\n\r\n",
        ] {
            assert!(body(&mut input.as_bytes()).is_err());
        }
    }
}
