// See crypto/src/main/scala/coop/rchain/crypto/util/CertificateHelper.scala

use std::time::{SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose;
use base64::Engine as _;
use p256::elliptic_curve::sec1::ToEncodedPoint;
use p256::pkcs8::{DecodePrivateKey, EncodePrivateKey};
use p256::{PublicKey as P256PublicKey, SecretKey as P256SecretKey};
use rand_core::OsRng;
use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair};
use x509_certificate::X509Certificate;

use crate::rust::hash::keccak256::Keccak256;

/// Errors that can occur during certificate operations
#[derive(Debug, thiserror::Error)]
pub enum CertificateError {
    #[error("Invalid public key: {0}")]
    InvalidPublicKey(String),
    #[error("Invalid private key: {0}")]
    InvalidPrivateKey(String),
    #[error("Certificate generation failed: {0}")]
    CertificateGeneration(String),
    #[error("Certificate parsing failed: {0}")]
    CertificateParsing(String),
    #[error("Signature encoding failed: {0}")]
    SignatureEncoding(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("X509 error: {0}")]
    X509(#[from] x509_certificate::X509CertificateError),
    #[error("Certificate generation error: {0}")]
    RcgenError(#[from] rcgen::Error),
}

/// CertificateHelper provides functionality for secp256r1 certificates and F1r3fly peer identity
pub struct CertificateHelper;

impl CertificateHelper {
    /// The elliptic curve name used by F1r3fly
    pub const ELLIPTIC_CURVE_NAME: &'static str = "secp256r1";

    /// Validate that a public key uses the expected elliptic curve (secp256r1/P-256)
    ///
    /// In Rust with p256::PublicKey, we know it's always P-256/secp256r1 by type,
    /// but we should still validate the key is well-formed and on the correct curve.
    pub fn is_expected_elliptic_curve(public_key: &P256PublicKey) -> bool {
        // Validate the public key is well-formed by trying to get its encoded point
        let encoded_point = public_key.to_encoded_point(false);

        // Should be 65 bytes: 0x04 prefix + 32-byte x + 32-byte y for uncompressed P-256
        if encoded_point.len() != 65 {
            return false;
        }

        // Verify the first byte is 0x04 (uncompressed point indicator)
        if encoded_point.as_bytes()[0] != 0x04 {
            return false;
        }

        // Try to create a new PublicKey from the SEC1 encoding to validate it's on the curve
        // This validates that the point actually lies on the secp256r1 curve
        use p256::elliptic_curve::sec1::FromEncodedPoint;
        let ct_option = P256PublicKey::from_encoded_point(&encoded_point);

        // CtOption is a constant-time Option equivalent - check if it contains a valid value
        ct_option.is_some().into()
    }

    /// Compute the public address from a secp256r1 public key using F1r3fly's algorithm
    ///
    /// Algorithm:
    /// 1. Extract x,y coordinates from EC public key (32 bytes each)
    /// 2. Create 64-byte uncompressed key representation
    /// 3. Hash with Keccak256
    /// 4. Take last 20 bytes (drop first 12) - Ethereum-style address
    pub fn public_address(public_key: &P256PublicKey) -> Option<Vec<u8>> {
        // Get uncompressed point encoding (0x04 + x + y)
        let encoded_point = public_key.to_encoded_point(false);

        // Should be 65 bytes: 0x04 prefix + 32-byte x + 32-byte y
        if encoded_point.len() != 65 {
            return None;
        }

        // Extract the 64-byte uncompressed key (skip 0x04 prefix)
        let uncompressed_key = &encoded_point.as_bytes()[1..];

        // Apply Keccak256 hash and take last 20 bytes (like Ethereum addresses)
        let hash = Keccak256::hash(uncompressed_key.to_vec());
        Some(hash[12..].to_vec()) // Take last 20 bytes
    }

    /// Compute public address from raw 64-byte key data
    pub fn public_address_from_bytes(input: &[u8]) -> Vec<u8> {
        let hash = Keccak256::hash(input.to_vec());
        hash[12..].to_vec() // Take last 20 bytes
    }

    /// Parse an X.509 certificate from DER bytes
    pub fn parse_certificate(der_bytes: &[u8]) -> Result<X509Certificate, CertificateError> {
        X509Certificate::from_der(der_bytes).map_err(CertificateError::X509)
    }

    /// Parse an X.509 certificate from PEM format
    pub fn parse_certificate_pem(pem_data: &str) -> Result<X509Certificate, CertificateError> {
        X509Certificate::from_pem(pem_data.as_bytes()).map_err(CertificateError::X509)
    }

    /// Read an X.509 certificate from a file path
    pub fn from_file(cert_file_path: &str) -> Result<X509Certificate, CertificateError> {
        // Read the file contents
        let cert_bytes = std::fs::read(cert_file_path).map_err(CertificateError::Io)?;

        // Try to parse as DER first, then PEM if that fails
        match X509Certificate::from_der(&cert_bytes) {
            Ok(cert) => Ok(cert),
            Err(_) => {
                // If DER parsing fails, try PEM
                let cert_string = String::from_utf8(cert_bytes).map_err(|e| {
                    CertificateError::CertificateParsing(format!(
                        "Invalid UTF-8 in certificate file: {}",
                        e
                    ))
                })?;
                X509Certificate::from_pem(cert_string.as_bytes()).map_err(CertificateError::X509)
            }
        }
    }

    /// Read a key pair from a PEM file
    ///
    /// The Scala version manually reconstructs the public key using:
    /// ```scala
    /// val ecSpec = org.bouncycastle.jce.ECNamedCurveTable.getParameterSpec(EllipticCurveName)
    /// val Q = ecSpec.getG.multiply(sk.getS).normalize()  // Q = G * s (point multiplication)
    /// ```
    ///
    /// The Rust version uses `secret_key.public_key()` which performs the same
    /// elliptic curve point multiplication (Q = G * s) internally, making them
    /// functionally equivalent but with better type safety.
    pub fn read_key_pair(
        key_file_path: &str,
    ) -> Result<(P256SecretKey, P256PublicKey), CertificateError> {
        // Read the file content
        let pem_content = std::fs::read_to_string(key_file_path).map_err(CertificateError::Io)?;

        // Remove PEM headers and decode base64
        let pem_lines: Vec<&str> = pem_content
            .lines()
            .filter(|line| !line.contains("KEY"))
            .collect();
        let base64_content = pem_lines.join("");

        let der_bytes = general_purpose::STANDARD
            .decode(&base64_content)
            .map_err(|e| {
                CertificateError::InvalidPrivateKey(format!("Base64 decode failed: {}", e))
            })?;

        // Parse PKCS#8 private key
        let secret_key = P256SecretKey::from_pkcs8_der(&der_bytes).map_err(|e| {
            CertificateError::InvalidPrivateKey(format!("PKCS8 parse failed: {}", e))
        })?;

        let public_key = secret_key.public_key();
        Ok((secret_key, public_key))
    }

    /// Generate a new secp256r1 key pair using the OS CSPRNG (`OsRng`).
    pub fn generate_key_pair() -> (P256SecretKey, P256PublicKey) {
        let secret_key = P256SecretKey::random(&mut OsRng);

        let public_key = secret_key.public_key();
        (secret_key, public_key)
    }

    /// Generate a self-signed X.509 certificate from a key pair
    pub fn generate_certificate(
        secret_key: &P256SecretKey,
        public_key: &P256PublicKey,
    ) -> Result<Vec<u8>, CertificateError> {
        // Compute the F1r3fly address for the certificate CN
        let f1r3fly_address = Self::public_address(public_key)
            .map(|addr| hex::encode(&addr))
            .unwrap_or_else(|| "local".to_string());

        // Create certificate parameters with F1r3fly address as CN and SAN
        let mut params = CertificateParams::new(vec![
            // Add F1r3fly address as DNS name for identity verification
            f1r3fly_address.clone(),
        ])
        .map_err(|e| {
            CertificateError::CertificateGeneration(format!("Failed to create params: {}", e))
        })?;

        // Set subject DN to CN=<f1r3fly_address>
        let mut distinguished_name = DistinguishedName::new();
        distinguished_name.push(DnType::CommonName, &f1r3fly_address);
        params.distinguished_name = distinguished_name;

        // Set validity period to 365 days
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        params.not_before = time::OffsetDateTime::from_unix_timestamp(now as i64)
            .map_err(|e| CertificateError::CertificateGeneration(format!("Invalid time: {}", e)))?;
        params.not_after = time::OffsetDateTime::from_unix_timestamp(
            (now + 365 * 24 * 60 * 60) as i64,
        )
        .map_err(|e| CertificateError::CertificateGeneration(format!("Invalid time: {}", e)))?;

        // Set serial number
        // Generate a 64-bit random serial number
        let mut serial_bytes = [0u8; 8];
        use rand_core::RngCore;
        OsRng.fill_bytes(&mut serial_bytes);
        params.serial_number = Some(serial_bytes.to_vec().into());

        // Note: rcgen automatically uses ECDSA-SHA256 for P-256 keys,
        // which matches Scala's "SHA256withECDSA" algorithm

        // Convert our key to rcgen format
        let key_pair = Self::convert_to_rcgen_keypair(secret_key)?;

        // Generate the certificate using rcgen's self_signed method
        let cert = params.self_signed(&key_pair)?;
        Ok(cert.der().to_vec())
    }

    /// Convert p256::SecretKey to rcgen::KeyPair for certificate generation
    fn convert_to_rcgen_keypair(secret_key: &P256SecretKey) -> Result<KeyPair, CertificateError> {
        // Get the raw bytes of the secret key using the EncodePrivateKey trait
        let secret_bytes = secret_key.to_pkcs8_der().map_err(|e| {
            CertificateError::CertificateGeneration(format!("Key serialization failed: {}", e))
        })?;

        // Create an rcgen KeyPair from the PKCS8 DER bytes
        let key_pair = KeyPair::from_pem(&format!(
            "-----BEGIN PRIVATE KEY-----\n{}\n-----END PRIVATE KEY-----",
            general_purpose::STANDARD.encode(secret_bytes.as_bytes())
        ))
        .map_err(|e| {
            CertificateError::CertificateGeneration(format!("Key conversion failed: {}", e))
        })?;

        Ok(key_pair)
    }

    /// Encode signature from raw R,S format to DER format
    pub fn encode_signature_rs_to_der(signature_rs: &[u8]) -> Result<Vec<u8>, CertificateError> {
        if signature_rs.is_empty() {
            return Err(CertificateError::SignatureEncoding(
                "Input array must not be empty".to_string(),
            ));
        }

        // Exactly r||s (64) or r||s||v (65, Ethereum's recovery byte, which
        // DER verification drops). Anything else is a non-canonical encoding:
        // silently truncating it would let distinct byte strings verify as
        // the same signature.
        if signature_rs.len() != 64 && signature_rs.len() != 65 {
            return Err(CertificateError::SignatureEncoding(
                "Signature must be 64 bytes (32-byte R + 32-byte S) or 65 bytes (R + S + recovery byte)"
                    .to_string(),
            ));
        }

        let (r_bytes, s_bytes) = signature_rs[..64].split_at(32);

        // Create properly formatted DER sequence manually
        let mut der = Vec::new();

        // ASN.1 SEQUENCE tag
        der.push(0x30);

        // We'll build the content first, then add the length
        let mut content = Vec::new();

        // Add R as ASN.1 INTEGER
        Self::add_asn1_integer(&mut content, r_bytes)?;

        // Add S as ASN.1 INTEGER
        Self::add_asn1_integer(&mut content, s_bytes)?;

        // Add the content length to the sequence
        if content.len() < 128 {
            der.push(content.len() as u8);
        } else {
            // Long form length encoding for large sequences
            let len_bytes = (content.len() as u32).to_be_bytes();
            let mut first_nonzero = 0;
            while first_nonzero < 4 && len_bytes[first_nonzero] == 0 {
                first_nonzero += 1;
            }
            der.push(0x80 | (4 - first_nonzero) as u8);
            der.extend_from_slice(&len_bytes[first_nonzero..]);
        }

        // Add the content
        der.extend(content);

        Ok(der)
    }

    /// Decode signature from DER format to raw R,S format  
    pub fn decode_signature_der_to_rs(signature_der: &[u8]) -> Result<Vec<u8>, CertificateError> {
        if signature_der.is_empty() {
            return Err(CertificateError::SignatureEncoding(
                "Input array must not be empty".to_string(),
            ));
        }

        let signature = p256::ecdsa::Signature::from_der(signature_der).map_err(|e| {
            CertificateError::SignatureEncoding(format!("DER decode failed: {}", e))
        })?;

        // Extract R and S components and convert to bytes
        let (r, s) = signature.split_scalars();

        // Convert to exactly 32-byte arrays
        let mut result = Vec::with_capacity(64);

        // Add R as exactly 32 bytes (left-padded with zeros if needed)
        let r_bytes = r.to_bytes();
        if r_bytes.len() <= 32 {
            // Left-pad with zeros to make exactly 32 bytes
            result.extend(std::iter::repeat_n(0u8, 32 - r_bytes.len()));
            result.extend_from_slice(&r_bytes);
        } else {
            // Take the rightmost 32 bytes if somehow longer
            result.extend_from_slice(&r_bytes[r_bytes.len() - 32..]);
        }

        // Add S as exactly 32 bytes (left-padded with zeros if needed)
        let s_bytes = s.to_bytes();
        if s_bytes.len() <= 32 {
            // Left-pad with zeros to make exactly 32 bytes
            result.extend(std::iter::repeat_n(0u8, 32 - s_bytes.len()));
            result.extend_from_slice(&s_bytes);
        } else {
            // Take the rightmost 32 bytes if somehow longer
            result.extend_from_slice(&s_bytes[s_bytes.len() - 32..]);
        }

        Ok(result)
    }

    /// Helper method to add ASN.1 INTEGER encoding to a content vector
    fn add_asn1_integer(content: &mut Vec<u8>, bytes: &[u8]) -> Result<(), CertificateError> {
        if bytes.is_empty() {
            return Ok(());
        }

        // Add ASN.1 INTEGER tag
        content.push(0x02);

        // DER integer encoding requires:
        // 1. If the first bit is 1, prepend 0x00 to indicate positive number
        // 2. Remove leading zeros (but not if it would make first bit 1)
        let mut trimmed_bytes = bytes;

        // Remove leading zeros but keep at least one byte
        while trimmed_bytes.len() > 1 && trimmed_bytes[0] == 0 {
            trimmed_bytes = &trimmed_bytes[1..];
        }

        // Check if we need to add a padding byte to indicate positive number
        let needs_padding = !trimmed_bytes.is_empty() && (trimmed_bytes[0] & 0x80) != 0;

        if needs_padding {
            // Add length (trimmed bytes + 1 padding byte)
            content.push((trimmed_bytes.len() + 1) as u8);
            // Add padding byte
            content.push(0x00);
            // Add trimmed bytes
            content.extend_from_slice(trimmed_bytes);
        } else {
            // Add length of trimmed bytes
            content.push(trimmed_bytes.len() as u8);
            // Add trimmed bytes
            content.extend_from_slice(trimmed_bytes);
        }

        Ok(())
    }

    pub fn normalize_public_key_coordinates(mut public_key: Vec<u8>) -> eyre::Result<Vec<u8>> {
        if public_key.is_empty() {
            return Err(eyre::eyre!(
                "Certificate public key is empty; cannot derive node name"
            ));
        }

        if public_key.first() == Some(&0) {
            public_key.remove(0);
        }

        if public_key.len() == 65 && public_key.first() == Some(&0x04) {
            public_key.remove(0);
        }

        if public_key.len() != 64 {
            return Err(eyre::eyre!(format!(
                "Unexpected public key length {} bytes; expected 64-byte coordinates",
                public_key.len()
            )));
        }

        Ok(public_key)
    }
}

/// Certificate printing utilities
pub struct CertificatePrinter;

impl CertificatePrinter {
    /// Format a certificate as PEM string
    pub fn print_certificate(cert_der: &[u8]) -> String {
        let base64_cert = general_purpose::STANDARD.encode(cert_der);
        let lines = Self::split_into_lines(&base64_cert, 64);
        format!(
            "-----BEGIN CERTIFICATE-----\n{}\n-----END CERTIFICATE-----",
            lines.join("\n")
        )
    }

    /// Format a private key as PEM string from DER bytes
    pub fn print_private_key(key_der: &[u8]) -> String {
        let base64_key = general_purpose::STANDARD.encode(key_der);
        let lines = Self::split_into_lines(&base64_key, 64);
        format!(
            "-----BEGIN PRIVATE KEY-----\n{}\n-----END PRIVATE KEY-----",
            lines.join("\n")
        )
    }

    /// Format a private key as PEM string from P256SecretKey
    /// Corresponds to CertificatePrinter.printPrivateKey(keyPair.getPrivate) in Scala
    pub fn print_private_key_from_secret(
        secret_key: &P256SecretKey,
    ) -> Result<String, CertificateError> {
        let key_der = secret_key.to_pkcs8_der().map_err(|e| {
            CertificateError::CertificateGeneration(format!("Key serialization failed: {}", e))
        })?;
        Ok(Self::print_private_key(key_der.as_bytes()))
    }

    /// Split a string into lines of specified length
    fn split_into_lines(s: &str, line_length: usize) -> Vec<String> {
        s.chars()
            .collect::<Vec<char>>()
            .chunks(line_length)
            .map(|chunk| chunk.iter().collect())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_and_reads_certificates_and_keys() {
        let directory = tempfile::tempdir().unwrap();
        let (secret_key, public_key) = CertificateHelper::generate_key_pair();

        assert_eq!(CertificateHelper::ELLIPTIC_CURVE_NAME, "secp256r1");
        assert!(CertificateHelper::is_expected_elliptic_curve(&public_key));

        let address = CertificateHelper::public_address(&public_key).unwrap();
        let point = public_key.to_encoded_point(false);
        assert_eq!(
            address,
            CertificateHelper::public_address_from_bytes(&point.as_bytes()[1..])
        );

        let certificate =
            CertificateHelper::generate_certificate(&secret_key, &public_key).unwrap();
        CertificateHelper::parse_certificate(&certificate).unwrap();

        let certificate_pem = CertificatePrinter::print_certificate(&certificate);
        CertificateHelper::parse_certificate_pem(&certificate_pem).unwrap();

        let der_path = directory.path().join("certificate.der");
        std::fs::write(&der_path, &certificate).unwrap();
        CertificateHelper::from_file(der_path.to_str().unwrap()).unwrap();

        let pem_path = directory.path().join("certificate.pem");
        std::fs::write(&pem_path, certificate_pem).unwrap();
        CertificateHelper::from_file(pem_path.to_str().unwrap()).unwrap();

        let private_key_pem =
            CertificatePrinter::print_private_key_from_secret(&secret_key).unwrap();
        let key_path = directory.path().join("private-key.pem");
        std::fs::write(&key_path, private_key_pem).unwrap();
        let (loaded_secret_key, loaded_public_key) =
            CertificateHelper::read_key_pair(key_path.to_str().unwrap()).unwrap();
        assert_eq!(loaded_secret_key.to_bytes(), secret_key.to_bytes());
        assert_eq!(
            loaded_public_key.to_encoded_point(false),
            public_key.to_encoded_point(false)
        );
    }

    #[test]
    fn rejects_invalid_certificate_and_key_data() {
        let directory = tempfile::tempdir().unwrap();

        assert!(CertificateHelper::parse_certificate(b"invalid").is_err());
        assert!(CertificateHelper::parse_certificate_pem("invalid").is_err());
        assert!(CertificateHelper::from_file("missing-certificate").is_err());

        let binary_path = directory.path().join("binary-certificate");
        std::fs::write(&binary_path, [0xff, 0xfe]).unwrap();
        assert!(matches!(
            CertificateHelper::from_file(binary_path.to_str().unwrap()),
            Err(CertificateError::CertificateParsing(_))
        ));

        let invalid_base64_path = directory.path().join("invalid-base64.pem");
        std::fs::write(
            &invalid_base64_path,
            "-----BEGIN PRIVATE KEY-----\n!\n-----END PRIVATE KEY-----",
        )
        .unwrap();
        assert!(matches!(
            CertificateHelper::read_key_pair(invalid_base64_path.to_str().unwrap()),
            Err(CertificateError::InvalidPrivateKey(_))
        ));

        let invalid_der_path = directory.path().join("invalid-der.pem");
        std::fs::write(
            &invalid_der_path,
            "-----BEGIN PRIVATE KEY-----\nAA==\n-----END PRIVATE KEY-----",
        )
        .unwrap();
        assert!(matches!(
            CertificateHelper::read_key_pair(invalid_der_path.to_str().unwrap()),
            Err(CertificateError::InvalidPrivateKey(_))
        ));
    }

    #[test]
    fn normalizes_public_key_coordinates() {
        let (_, public_key) = CertificateHelper::generate_key_pair();
        let encoded = public_key.to_encoded_point(false).as_bytes().to_vec();
        let coordinates = encoded[1..].to_vec();

        assert_eq!(
            CertificateHelper::normalize_public_key_coordinates(encoded).unwrap(),
            coordinates
        );

        let mut zero_prefixed = vec![0];
        zero_prefixed.extend_from_slice(&coordinates);
        assert_eq!(
            CertificateHelper::normalize_public_key_coordinates(zero_prefixed).unwrap(),
            coordinates
        );

        assert!(CertificateHelper::normalize_public_key_coordinates(Vec::new()).is_err());
        assert!(CertificateHelper::normalize_public_key_coordinates(vec![1; 63]).is_err());
    }

    #[test]
    fn encodes_and_decodes_signatures() {
        let signature = vec![1; 64];
        let encoded = CertificateHelper::encode_signature_rs_to_der(&signature).unwrap();
        assert_eq!(
            CertificateHelper::decode_signature_der_to_rs(&encoded).unwrap(),
            signature
        );

        let eth_with_recovery_byte = vec![1; 65];
        let encoded =
            CertificateHelper::encode_signature_rs_to_der(&eth_with_recovery_byte).unwrap();
        assert_eq!(
            CertificateHelper::decode_signature_der_to_rs(&encoded).unwrap(),
            vec![1; 64]
        );

        assert!(CertificateHelper::encode_signature_rs_to_der(&[]).is_err());
        assert!(CertificateHelper::encode_signature_rs_to_der(&[1; 63]).is_err());
        assert!(CertificateHelper::encode_signature_rs_to_der(&[1; 66]).is_err());
        assert!(CertificateHelper::encode_signature_rs_to_der(&[1; 128]).is_err());
        assert!(CertificateHelper::decode_signature_der_to_rs(&[]).is_err());
        assert!(CertificateHelper::decode_signature_der_to_rs(&[1; 64]).is_err());

        let mut content = Vec::new();
        CertificateHelper::add_asn1_integer(&mut content, &[]).unwrap();
        assert!(content.is_empty());

        CertificateHelper::add_asn1_integer(&mut content, &[0, 0, 1]).unwrap();
        assert_eq!(content, [0x02, 0x01, 0x01]);

        content.clear();
        CertificateHelper::add_asn1_integer(&mut content, &[0x80]).unwrap();
        assert_eq!(content, [0x02, 0x02, 0x00, 0x80]);
    }

    #[test]
    fn formats_pem_lines() {
        let data = vec![7; 128];
        let certificate = CertificatePrinter::print_certificate(&data);
        let private_key = CertificatePrinter::print_private_key(&data);

        assert!(certificate.starts_with("-----BEGIN CERTIFICATE-----\n"));
        assert!(certificate.ends_with("\n-----END CERTIFICATE-----"));
        assert!(private_key.starts_with("-----BEGIN PRIVATE KEY-----\n"));
        assert!(private_key.ends_with("\n-----END PRIVATE KEY-----"));
        assert!(CertificatePrinter::split_into_lines("abcdefgh", 3)
            .iter()
            .eq(["abc", "def", "gh"].iter()));
    }
}
