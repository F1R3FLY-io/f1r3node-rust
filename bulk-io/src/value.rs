use std::collections::BTreeMap;

use crate::error::BulkError;
use crate::manifest::FieldType;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RawValue {
    Text(String),
    Json(serde_json::Value),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Decimal(String),
    Str(String),
    Bytes(Vec<u8>),
}

pub type Record = BTreeMap<String, Value>;

pub const FILE_MAGIC: &[u8; 5] = b"F1BK\x01";

const TAG_NULL: u8 = 0;
const TAG_BOOL: u8 = 1;
const TAG_INT: u8 = 2;
const TAG_DECIMAL: u8 = 3;
const TAG_STR: u8 = 4;
const TAG_BYTES: u8 = 5;

fn canonical_int(s: &str) -> Option<i64> {
    let digits = s.strip_prefix('-').unwrap_or(s);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if digits.len() > 1 && digits.starts_with('0') {
        return None;
    }
    if s == "-0" {
        return None;
    }
    s.parse().ok()
}

fn canonical_decimal(s: &str) -> bool {
    let body = s.strip_prefix('-').unwrap_or(s);
    let (int, frac) = match body.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (body, None),
    };
    if int.is_empty() || !int.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    if int.len() > 1 && int.starts_with('0') {
        return false;
    }
    if let Some(f) = frac {
        if f.is_empty() || !f.bytes().all(|b| b.is_ascii_digit()) || f.ends_with('0') {
            return false;
        }
    }
    if s.starts_with('-') && int == "0" && frac.is_none() {
        return false;
    }
    s.len() <= 128
}

fn hex_bytes(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2)
        || !s
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return None;
    }
    hex::decode(s).ok()
}

pub fn convert(raw: Option<&RawValue>, ty: FieldType) -> Result<Value, String> {
    let raw = match raw {
        None => return Ok(Value::Null),
        Some(RawValue::Text(t)) if t.is_empty() => return Ok(Value::Null),
        Some(RawValue::Json(serde_json::Value::Null)) => return Ok(Value::Null),
        Some(r) => r,
    };
    match (ty, raw) {
        (FieldType::String, RawValue::Text(t)) => Ok(Value::Str(t.clone())),
        (FieldType::String, RawValue::Json(serde_json::Value::String(t))) => {
            Ok(Value::Str(t.clone()))
        }
        (FieldType::Int, RawValue::Text(t)) => canonical_int(t)
            .map(Value::Int)
            .ok_or_else(|| format!("not a canonical integer: {t:?}")),
        (FieldType::Int, RawValue::Json(serde_json::Value::Number(n))) => n
            .as_i64()
            .map(Value::Int)
            .ok_or_else(|| format!("not a 64-bit integer: {n}")),
        (FieldType::Decimal, RawValue::Text(t))
        | (FieldType::Decimal, RawValue::Json(serde_json::Value::String(t))) => {
            if canonical_decimal(t) {
                Ok(Value::Decimal(t.clone()))
            } else {
                Err(format!("not a canonical decimal: {t:?}"))
            }
        }
        (FieldType::Decimal, RawValue::Json(serde_json::Value::Number(n))) => n
            .as_i64()
            .map(|i| Value::Decimal(i.to_string()))
            .ok_or_else(|| format!("decimal numbers must be integers or canonical strings: {n}")),
        (FieldType::Bool, RawValue::Text(t)) => match t.as_str() {
            "true" => Ok(Value::Bool(true)),
            "false" => Ok(Value::Bool(false)),
            _ => Err(format!("not a boolean: {t:?}")),
        },
        (FieldType::Bool, RawValue::Json(serde_json::Value::Bool(b))) => Ok(Value::Bool(*b)),
        (FieldType::Bytes, RawValue::Text(t))
        | (FieldType::Bytes, RawValue::Json(serde_json::Value::String(t))) => hex_bytes(t)
            .map(Value::Bytes)
            .ok_or_else(|| "bytes must be lowercase hex".to_string()),
        (ty, RawValue::Json(v)) => Err(format!("JSON value {v} does not fit type {ty:?}")),
    }
}

fn put_len(out: &mut Vec<u8>, n: usize) { out.extend_from_slice(&(n as u32).to_be_bytes()); }

pub fn encode_value(v: &Value, out: &mut Vec<u8>) {
    match v {
        Value::Null => out.push(TAG_NULL),
        Value::Bool(b) => {
            out.push(TAG_BOOL);
            out.push(u8::from(*b));
        }
        Value::Int(i) => {
            out.push(TAG_INT);
            out.extend_from_slice(&i.to_be_bytes());
        }
        Value::Decimal(s) => {
            out.push(TAG_DECIMAL);
            put_len(out, s.len());
            out.extend_from_slice(s.as_bytes());
        }
        Value::Str(s) => {
            out.push(TAG_STR);
            put_len(out, s.len());
            out.extend_from_slice(s.as_bytes());
        }
        Value::Bytes(b) => {
            out.push(TAG_BYTES);
            put_len(out, b.len());
            out.extend_from_slice(b);
        }
    }
}

pub fn encode_record(r: &Record) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(r.len() as u16).to_be_bytes());
    for (name, v) in r {
        out.push(name.len() as u8);
        out.extend_from_slice(name.as_bytes());
        encode_value(v, &mut out);
    }
    out
}

pub fn encode_key(values: &[&Value]) -> Vec<u8> {
    let mut out = Vec::new();
    for v in values {
        encode_value(v, &mut out);
    }
    out
}

struct Cursor<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], BulkError> {
        if self.i + n > self.b.len() {
            return Err(BulkError::Record("truncated record".into()));
        }
        let s = &self.b[self.i..self.i + n];
        self.i += n;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, BulkError> { Ok(self.take(1)?[0]) }
    fn u32(&mut self) -> Result<usize, BulkError> {
        let s = self.take(4)?;
        Ok(u32::from_be_bytes([s[0], s[1], s[2], s[3]]) as usize)
    }
    fn string(&mut self) -> Result<String, BulkError> {
        let n = self.u32()?;
        String::from_utf8(self.take(n)?.to_vec())
            .map_err(|_| BulkError::Record("invalid UTF-8".into()))
    }
}

fn decode_value(c: &mut Cursor<'_>) -> Result<Value, BulkError> {
    match c.u8()? {
        TAG_NULL => Ok(Value::Null),
        TAG_BOOL => match c.u8()? {
            0 => Ok(Value::Bool(false)),
            1 => Ok(Value::Bool(true)),
            _ => Err(BulkError::Record("bad boolean".into())),
        },
        TAG_INT => {
            let s = c.take(8)?;
            let mut a = [0u8; 8];
            a.copy_from_slice(s);
            Ok(Value::Int(i64::from_be_bytes(a)))
        }
        TAG_DECIMAL => Ok(Value::Decimal(c.string()?)),
        TAG_STR => Ok(Value::Str(c.string()?)),
        TAG_BYTES => {
            let n = c.u32()?;
            Ok(Value::Bytes(c.take(n)?.to_vec()))
        }
        t => Err(BulkError::Record(format!("unknown value tag {t}"))),
    }
}

pub fn decode_record(b: &[u8]) -> Result<Record, BulkError> {
    let mut c = Cursor { b, i: 0 };
    let hdr = c.take(2)?;
    let n = u16::from_be_bytes([hdr[0], hdr[1]]) as usize;
    let mut r = Record::new();
    for _ in 0..n {
        let len = c.u8()? as usize;
        let name = String::from_utf8(c.take(len)?.to_vec())
            .map_err(|_| BulkError::Record("invalid field name".into()))?;
        let v = decode_value(&mut c)?;
        if let Some((last, _)) = r.last_key_value() {
            if last.as_str() >= name.as_str() {
                return Err(BulkError::Record("fields out of order".into()));
            }
        }
        r.insert(name, v);
    }
    if c.i != b.len() {
        return Err(BulkError::Record("trailing bytes in record".into()));
    }
    Ok(r)
}

pub fn frame(key: &[u8], rec: &[u8], out: &mut Vec<u8>) {
    put_len(out, key.len());
    out.extend_from_slice(key);
    put_len(out, rec.len());
    out.extend_from_slice(rec);
}

pub type KeyedRecord = (Vec<u8>, Vec<u8>);

pub fn unframe_file(bytes: &[u8]) -> Result<Vec<KeyedRecord>, BulkError> {
    if bytes.len() < FILE_MAGIC.len() || &bytes[..FILE_MAGIC.len()] != FILE_MAGIC {
        return Err(BulkError::Record("bad record file header".into()));
    }
    let mut c = Cursor {
        b: bytes,
        i: FILE_MAGIC.len(),
    };
    let mut out = Vec::new();
    while c.i < bytes.len() {
        let kl = c.u32()?;
        let k = c.take(kl)?.to_vec();
        let rl = c.u32()?;
        let r = c.take(rl)?.to_vec();
        out.push((k, r));
    }
    Ok(out)
}

pub fn value_to_text(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::Int(i) => i.to_string(),
        Value::Decimal(s) | Value::Str(s) => s.clone(),
        Value::Bytes(b) => hex::encode(b),
    }
}

pub fn value_to_json(v: &Value) -> serde_json::Value {
    match v {
        Value::Null => serde_json::Value::Null,
        Value::Bool(b) => serde_json::Value::Bool(*b),
        Value::Int(i) => serde_json::Value::from(*i),
        Value::Decimal(s) | Value::Str(s) => serde_json::Value::String(s.clone()),
        Value::Bytes(b) => serde_json::Value::String(hex::encode(b)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> RawValue { RawValue::Text(s.into()) }

    #[test]
    fn integers_must_be_canonical() {
        assert_eq!(convert(Some(&t("42")), FieldType::Int), Ok(Value::Int(42)));
        assert_eq!(convert(Some(&t("-7")), FieldType::Int), Ok(Value::Int(-7)));
        for bad in ["042", "+1", "-0", "1.0", " 1", "x", "99999999999999999999"] {
            assert!(convert(Some(&t(bad)), FieldType::Int).is_err(), "{bad}");
        }
    }

    #[test]
    fn decimals_must_be_canonical() {
        for ok in ["0", "1.5", "-2.25", "10", "0.001"] {
            assert!(convert(Some(&t(ok)), FieldType::Decimal).is_ok(), "{ok}");
        }
        for bad in ["1.50", "01", "1.", ".5", "-0", "1e3"] {
            assert!(convert(Some(&t(bad)), FieldType::Decimal).is_err(), "{bad}");
        }
    }

    #[test]
    fn empty_text_and_missing_are_null() {
        assert_eq!(convert(Some(&t("")), FieldType::String), Ok(Value::Null));
        assert_eq!(convert(None, FieldType::Int), Ok(Value::Null));
        assert_eq!(
            convert(
                Some(&RawValue::Json(serde_json::Value::Null)),
                FieldType::Bool
            ),
            Ok(Value::Null)
        );
    }

    #[test]
    fn json_types_are_strict() {
        let j = |v: serde_json::Value| RawValue::Json(v);
        assert!(convert(Some(&j(serde_json::json!("1"))), FieldType::Int).is_err());
        assert!(convert(Some(&j(serde_json::json!(1.5))), FieldType::Int).is_err());
        assert_eq!(
            convert(Some(&j(serde_json::json!(true))), FieldType::Bool),
            Ok(Value::Bool(true))
        );
        assert!(convert(Some(&j(serde_json::json!({"a": 1}))), FieldType::String).is_err());
    }

    #[test]
    fn record_round_trip_and_order_check() {
        let mut r = Record::new();
        r.insert("a".into(), Value::Int(1));
        r.insert("b".into(), Value::Str("x".into()));
        r.insert("c".into(), Value::Bytes(vec![1, 2]));
        r.insert("d".into(), Value::Null);
        r.insert("e".into(), Value::Bool(true));
        r.insert("f".into(), Value::Decimal("1.5".into()));
        let bytes = encode_record(&r);
        assert_eq!(decode_record(&bytes).unwrap(), r);
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert!(decode_record(&trailing).is_err());
    }

    #[test]
    fn frames_round_trip() {
        let mut f = FILE_MAGIC.to_vec();
        frame(b"k1", b"r1", &mut f);
        frame(b"k2", b"r2", &mut f);
        let items = unframe_file(&f).unwrap();
        assert_eq!(items, vec![
            (b"k1".to_vec(), b"r1".to_vec()),
            (b"k2".to_vec(), b"r2".to_vec())
        ]);
        assert!(unframe_file(&f[..f.len() - 1]).is_err());
    }
}
