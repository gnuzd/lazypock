//! AWS SigV4 request signing for S3-compatible storage.
//!
//! Signing is implemented here rather than pulled in as a dependency, and the
//! core is verified against the worked example in the AWS documentation
//! (`docs.aws.amazon.com/general/latest/gr/sigv4-create-canonical-request.html`).
//!
//! What is **not** done yet is the transport: these functions produce a signed
//! request, but nothing sends it. The S3 adapter therefore refuses to start
//! rather than pretending, and the missing piece is tracked in `PLAN.md`
//! section 10.

use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

/// The algorithm name that goes in the `Authorization` header.
pub const ALGORITHM: &str = "AWS4-HMAC-SHA256";

/// A request to sign.
#[derive(Debug, Clone)]
pub struct SignableRequest<'a> {
    /// HTTP method.
    pub method: &'a str,
    /// The already-encoded path, e.g. `/bucket/photos/a.png`.
    pub canonical_uri: &'a str,
    /// The already-encoded query string, without `?`.
    pub canonical_query: &'a str,
    /// Headers to sign. `x-amz-date` is added; `host` must be present.
    pub headers: Vec<(String, String)>,
    /// Hex SHA-256 of the body.
    pub payload_hash: &'a str,
    /// AWS region, or `auto` for R2.
    pub region: &'a str,
    /// Service name, `s3` for S3-compatible storage.
    pub service: &'a str,
    /// Access key id.
    pub access_key: &'a str,
    /// Secret.
    pub secret_key: &'a str,
    /// `YYYYMMDDTHHMMSSZ`.
    pub timestamp: &'a str,
}

/// A signed request: everything needed to send it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedRequest {
    /// HTTP method.
    pub method: String,
    /// The canonical URI.
    pub uri: String,
    /// The canonical query string.
    pub query: String,
    /// Every header to send, sorted, with `x-amz-date` added.
    pub headers: Vec<(String, String)>,
    /// The `Authorization` header value.
    pub authorization: String,
}

impl SignedRequest {
    /// Look a header up by name.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

/// Sign a request.
///
/// # Panics
///
/// Panics when `timestamp` is shorter than eight characters, which would mean a
/// caller built a malformed request rather than a runtime condition.
pub fn sign(mut request: SignableRequest<'_>) -> SignedRequest {
    let date = &request.timestamp[..8];

    request
        .headers
        .push(("x-amz-date".to_string(), request.timestamp.to_string()));
    // Signed headers must be sorted by name and lowercased.
    for (name, _) in request.headers.iter_mut() {
        *name = name.to_ascii_lowercase();
    }
    request.headers.sort();

    let signed_headers = request
        .headers
        .iter()
        .map(|(name, _)| name.as_str())
        .collect::<Vec<_>>()
        .join(";");
    let canonical_headers = request
        .headers
        .iter()
        .map(|(name, value)| format!("{name}:{value}\n"))
        .collect::<String>();

    let canonical_request = format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        request.method,
        request.canonical_uri,
        request.canonical_query,
        canonical_headers,
        signed_headers,
        request.payload_hash,
    );

    let scope = format!("{date}/{}/{}/aws4_request", request.region, request.service);
    let string_to_sign = format!(
        "{ALGORITHM}\n{}\n{scope}\n{}",
        request.timestamp,
        hex(&Sha256::digest(canonical_request.as_bytes())),
    );

    let signing_key = signing_key(request.secret_key, date, request.region, request.service);
    let signature = hex(&hmac(&signing_key, string_to_sign.as_bytes()));

    let authorization = format!(
        "{ALGORITHM} Credential={}/{scope}, SignedHeaders={signed_headers}, Signature={signature}",
        request.access_key,
    );

    SignedRequest {
        method: request.method.to_string(),
        uri: request.canonical_uri.to_string(),
        query: request.canonical_query.to_string(),
        headers: request.headers,
        authorization,
    }
}

/// The derived signing key for a day, region and service.
fn signing_key(secret: &str, date: &str, region: &str, service: &str) -> Vec<u8> {
    let initial = hmac(format!("AWS4{secret}").as_bytes(), date.as_bytes());
    let region_key = hmac(&initial, region.as_bytes());
    let service_key = hmac(&region_key, service.as_bytes());
    hmac(&service_key, b"aws4_request")
}

fn hmac(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Hex SHA-256 of a body, which S3 requires as `x-amz-content-sha256`.
pub fn payload_hash(body: &[u8]) -> String {
    hex(&Sha256::digest(body))
}

/// The empty-body hash, which is what every GET and DELETE uses.
pub fn empty_payload_hash() -> String {
    payload_hash(b"")
}

/// Percent-encode a path for a canonical URI: unreserved characters and `/` are
/// kept, everything else is escaped.
pub fn encode_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for byte in path.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                out.push(byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// The `host` header for an endpoint.
pub fn host_of(endpoint: &str) -> String {
    endpoint
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The worked example from the AWS documentation: a GET with no query and
    /// only `host` and `x-amz-date` to sign.
    #[test]
    fn the_aws_worked_example_produces_the_documented_signature() {
        let signed = sign(SignableRequest {
            method: "GET",
            canonical_uri: "/",
            canonical_query: "",
            headers: vec![("host".to_string(), "example.amazonaws.com".to_string())],
            payload_hash: &empty_payload_hash(),
            region: "us-east-1",
            service: "service",
            access_key: "AKIDEXAMPLE",
            secret_key: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
            timestamp: "20150830T123600Z",
        });

        assert_eq!(
            signed.authorization,
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/service/aws4_request, \
             SignedHeaders=host;x-amz-date, \
             Signature=5fa00fa31553b73ebf1942676e86291e8372ff2a2260956d9b8aae1d763fbf31"
        );
        assert_eq!(signed.header("x-amz-date"), Some("20150830T123600Z"));
        assert_eq!(signed.header("host"), Some("example.amazonaws.com"));
    }

    #[test]
    fn the_empty_body_hash_is_the_documented_constant() {
        assert_eq!(
            empty_payload_hash(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn signed_headers_are_lowercased_and_sorted() {
        let signed = sign(SignableRequest {
            method: "PUT",
            canonical_uri: "/bucket/key.png",
            canonical_query: "",
            headers: vec![
                ("Host".to_string(), "example.com".to_string()),
                ("Content-Type".to_string(), "image/png".to_string()),
                ("x-amz-content-sha256".to_string(), payload_hash(b"abc")),
            ],
            payload_hash: &payload_hash(b"abc"),
            region: "auto",
            service: "s3",
            access_key: "key",
            secret_key: "secret",
            timestamp: "20260101T000000Z",
        });

        assert!(
            signed
                .authorization
                .contains("SignedHeaders=content-type;host;x-amz-content-sha256;x-amz-date"),
            "{}",
            signed.authorization
        );
    }

    #[test]
    fn a_different_body_produces_a_different_signature() {
        let base = |body: &'static [u8]| {
            sign(SignableRequest {
                method: "PUT",
                canonical_uri: "/bucket/key.png",
                canonical_query: "",
                headers: vec![("host".to_string(), "example.com".to_string())],
                payload_hash: &payload_hash(body),
                region: "auto",
                service: "s3",
                access_key: "key",
                secret_key: "secret",
                timestamp: "20260101T000000Z",
            })
            .authorization
        };
        assert_ne!(base(b"one"), base(b"two"));
    }

    #[test]
    fn a_different_secret_produces_a_different_signature() {
        let with = |secret: &'static str| {
            sign(SignableRequest {
                method: "GET",
                canonical_uri: "/",
                canonical_query: "",
                headers: vec![("host".to_string(), "example.com".to_string())],
                payload_hash: &empty_payload_hash(),
                region: "auto",
                service: "s3",
                access_key: "key",
                secret_key: secret,
                timestamp: "20260101T000000Z",
            })
            .authorization
        };
        assert_ne!(with("secret"), with("other"));
        // ... and the secret never appears in the header.
        assert!(!with("secret").contains("secret "), "{}", with("secret"));
    }

    #[test]
    fn paths_are_encoded_but_separators_survive() {
        assert_eq!(encode_path("/bucket/a b.png"), "/bucket/a%20b.png");
        assert_eq!(encode_path("/bucket/a+b"), "/bucket/a%2Bb");
        assert_eq!(
            encode_path("/bucket/plain-name_1.2~3"),
            "/bucket/plain-name_1.2~3"
        );
        assert_eq!(encode_path("/bucket/ünicode"), "/bucket/%C3%BCnicode");
    }

    #[test]
    fn hosts_are_taken_from_the_endpoint() {
        assert_eq!(
            host_of("https://account.r2.cloudflarestorage.com"),
            "account.r2.cloudflarestorage.com"
        );
        assert_eq!(host_of("http://localhost:9000/"), "localhost:9000");
    }
}
