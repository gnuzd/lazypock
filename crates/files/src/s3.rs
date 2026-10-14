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

// ---------------------------------------------------------------------------
// The transport
// ---------------------------------------------------------------------------

use std::time::{SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use lazypock_core::error::{Error, Result};

use crate::config::FilesConfig;
use crate::storage::{Storage, StorageFuture, StoredObject};

/// Most keys one listing page may return.
const LIST_PAGE: u32 = 1000;

/// The current UTC time as `YYYYMMDDTHHMMSSZ`, which SigV4 requires.
///
/// Written out rather than pulling in a date library: the only thing needed is
/// this one format, and it is a fixed number of integer operations.
pub fn utc_timestamp() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0);
    format_timestamp(seconds)
}

/// Format unix seconds as `YYYYMMDDTHHMMSSZ`.
fn format_timestamp(seconds: i64) -> String {
    let days = seconds.div_euclid(86_400);
    let time = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}{month:02}{day:02}T{:02}{:02}{:02}Z",
        time / 3600,
        (time % 3600) / 60,
        time % 60
    )
}

/// The calendar date for a count of days since 1970-01-01.
///
/// Howard Hinnant's `civil_from_days`, which is exact for the proleptic Gregorian
/// calendar and needs no tables.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let march_month = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * march_month + 2) / 5 + 1) as u32;
    let month = if march_month < 10 {
        march_month + 3
    } else {
        march_month - 9
    } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

/// Percent-encode a query parameter value the way SigV4 requires.
pub fn encode_query_value(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// An S3-compatible backend: AWS S3, Cloudflare R2, MinIO and the rest.
///
/// Requests are addressed path-style, `{endpoint}/{bucket}/{key}`, which every
/// S3-compatible service accepts and which avoids needing a wildcard DNS name for
/// the bucket.
#[derive(Clone)]
pub struct S3Storage {
    endpoint: String,
    host: String,
    bucket: String,
    region: String,
    access_key: String,
    secret_key: String,
    client: reqwest::Client,
}

impl std::fmt::Debug for S3Storage {
    /// Never prints the secret.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("S3Storage")
            .field("endpoint", &self.endpoint)
            .field("bucket", &self.bucket)
            .field("region", &self.region)
            .field("access_key", &"<redacted>")
            .field("secret_key", &"<redacted>")
            .finish_non_exhaustive()
    }
}

impl S3Storage {
    /// Build a backend from configuration.
    pub fn new(config: &FilesConfig) -> Result<Self> {
        let endpoint = config
            .endpoint
            .as_deref()
            .ok_or_else(|| Error::config("the S3 adapter needs LAZYPOCK_S3_ENDPOINT"))?;
        let bucket = config
            .bucket
            .as_deref()
            .ok_or_else(|| Error::config("the S3 adapter needs LAZYPOCK_S3_BUCKET"))?;
        let access_key = config
            .access_key
            .as_deref()
            .ok_or_else(|| Error::config("the S3 adapter needs LAZYPOCK_S3_ACCESS_KEY"))?;
        let secret_key = config
            .secret_key
            .as_deref()
            .ok_or_else(|| Error::config("the S3 adapter needs LAZYPOCK_S3_SECRET_KEY"))?;

        let client = reqwest::Client::builder()
            .build()
            .map_err(|err| Error::config(format!("cannot build the S3 HTTP client: {err}")))?;

        Ok(Self {
            host: host_of(endpoint),
            endpoint: endpoint.trim_end_matches('/').to_string(),
            bucket: bucket.to_string(),
            region: config.region.clone(),
            access_key: access_key.to_string(),
            secret_key: secret_key.to_string(),
            client,
        })
    }

    /// The canonical URI for a key.
    fn uri_for(&self, key: &str) -> String {
        if key.is_empty() {
            format!("/{}/", self.bucket)
        } else {
            format!("/{}/{}", self.bucket, encode_path(key))
        }
    }

    /// Sign a request without sending it.
    ///
    /// Separated from sending so the request can be asserted on directly: signing
    /// is where the mistakes are, and a test that needs a live bucket would not
    /// run.
    pub fn signed(
        &self,
        method: &str,
        canonical_uri: &str,
        canonical_query: &str,
        payload: &[u8],
        content_type: Option<&str>,
        timestamp: &str,
    ) -> SignedRequest {
        let mut headers = vec![
            ("host".to_string(), self.host.clone()),
            ("x-amz-content-sha256".to_string(), payload_hash(payload)),
        ];
        if let Some(content_type) = content_type {
            headers.push(("content-type".to_string(), content_type.to_string()));
        }

        let hash = payload_hash(payload);
        sign(SignableRequest {
            method,
            canonical_uri,
            canonical_query,
            headers,
            payload_hash: &hash,
            region: &self.region,
            service: "s3",
            access_key: &self.access_key,
            secret_key: &self.secret_key,
            timestamp,
        })
    }

    /// Send a signed request.
    async fn send(&self, signed: SignedRequest, body: Vec<u8>) -> Result<reqwest::Response> {
        let url = if signed.query.is_empty() {
            format!("{}{}", self.endpoint, signed.uri)
        } else {
            format!("{}{}?{}", self.endpoint, signed.uri, signed.query)
        };

        let method = reqwest::Method::from_bytes(signed.method.as_bytes())
            .map_err(|err| Error::internal(format!("bad S3 method: {err}")))?;
        let mut request = self.client.request(method, &url);

        for (name, value) in &signed.headers {
            // `host` is signed but must be set by the client from the URL, so
            // setting it here as well would be redundant at best.
            if name.eq_ignore_ascii_case("host") {
                continue;
            }
            request = request.header(name.as_str(), value.as_str());
        }
        request = request.header("authorization", signed.authorization.as_str());

        request
            .body(body)
            .send()
            .await
            .map_err(|err| Error::internal(format!("the S3 request to {url} failed: {err}")))
    }

    /// Turn a failed response into an error that names the cause.
    async fn failure(what: &str, response: reqwest::Response) -> Error {
        let status = response.status();
        // S3 explains itself in the body and in a code header; keep enough of it
        // to be actionable without dumping a whole document into the log.
        let code = response
            .headers()
            .get("x-amz-error-code")
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        let body = response.text().await.unwrap_or_default();
        let detail = body.chars().take(300).collect::<String>();

        Error::internal(format!(
            "S3 {what} failed: {status}{}{}",
            code.map(|code| format!(" ({code})")).unwrap_or_default(),
            if detail.is_empty() {
                String::new()
            } else {
                format!(": {detail}")
            }
        ))
    }
}

impl Storage for S3Storage {
    fn put<'a>(
        &'a self,
        key: &'a str,
        content_type: &'a str,
        body: Bytes,
    ) -> StorageFuture<'a, ()> {
        Box::pin(async move {
            let signed = self.signed(
                "PUT",
                &self.uri_for(key),
                "",
                &body,
                Some(content_type),
                &utc_timestamp(),
            );
            let response = self.send(signed, body.to_vec()).await?;
            if !response.status().is_success() {
                return Err(Self::failure(&format!("PUT {key}"), response).await);
            }
            Ok(())
        })
    }

    fn get<'a>(&'a self, key: &'a str) -> StorageFuture<'a, Option<StoredObject>> {
        Box::pin(async move {
            let signed = self.signed("GET", &self.uri_for(key), "", b"", None, &utc_timestamp());
            let response = self.send(signed, Vec::new()).await?;

            match response.status().as_u16() {
                200 => {
                    let content_type = response
                        .headers()
                        .get(reqwest::header::CONTENT_TYPE)
                        .and_then(|value| value.to_str().ok())
                        .unwrap_or("application/octet-stream")
                        .to_string();
                    let body = response
                        .bytes()
                        .await
                        .map_err(|err| Error::internal(format!("cannot read {key}: {err}")))?;
                    Ok(Some(StoredObject { content_type, body }))
                }
                404 => Ok(None),
                _ => Err(Self::failure(&format!("GET {key}"), response).await),
            }
        })
    }

    fn delete<'a>(&'a self, key: &'a str) -> StorageFuture<'a, ()> {
        Box::pin(async move {
            let signed = self.signed(
                "DELETE",
                &self.uri_for(key),
                "",
                b"",
                None,
                &utc_timestamp(),
            );
            let response = self.send(signed, Vec::new()).await?;

            match response.status().as_u16() {
                // A missing object is not an error, so a retried delete succeeds.
                200 | 204 | 404 => Ok(()),
                _ => Err(Self::failure(&format!("DELETE {key}"), response).await),
            }
        })
    }

    fn health(&self) -> StorageFuture<'_, ()> {
        Box::pin(async move {
            // One key is enough to prove the credentials and the bucket work.
            let query = format!("list-type=2&max-keys=1&prefix={}", encode_query_value(""));
            let signed = self.signed(
                "GET",
                &self.uri_for(""),
                &query,
                b"",
                None,
                &utc_timestamp(),
            );
            let response = self.send(signed, Vec::new()).await?;
            if !response.status().is_success() {
                return Err(Self::failure("bucket check", response).await);
            }
            Ok(())
        })
    }

    fn list<'a>(&'a self, prefix: &'a str) -> StorageFuture<'a, Vec<String>> {
        Box::pin(async move {
            let mut keys = Vec::new();
            let mut token: Option<String> = None;

            loop {
                let mut query = format!(
                    "list-type=2&max-keys={LIST_PAGE}&prefix={}",
                    encode_query_value(prefix)
                );
                if let Some(token) = &token {
                    query.push_str(&format!(
                        "&continuation-token={}",
                        encode_query_value(token)
                    ));
                }

                let signed = self.signed(
                    "GET",
                    &self.uri_for(""),
                    &query,
                    b"",
                    None,
                    &utc_timestamp(),
                );
                let response = self.send(signed, Vec::new()).await?;
                if !response.status().is_success() {
                    return Err(Self::failure("LIST", response).await);
                }

                let body = response
                    .text()
                    .await
                    .map_err(|err| Error::internal(format!("cannot read the S3 listing: {err}")))?;
                keys.extend(parse_listing(&body));

                // Keep going only while the service says there is more; an
                // ignored truncation would make the sweeper think a partial view
                // was the whole store.
                token = if is_truncated(&body) {
                    parse_continuation(&body)
                } else {
                    None
                };
                if token.is_none() {
                    break;
                }
            }

            keys.sort();
            Ok(keys)
        })
    }
}

/// The `<Key>` values in a ListObjectsV2 response.
pub fn parse_listing(xml: &str) -> Vec<String> {
    let mut keys = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<Key>") {
        let after = &rest[start + "<Key>".len()..];
        match after.find("</Key>") {
            Some(end) => {
                keys.push(unescape_xml(&after[..end]));
                rest = &after[end + "</Key>".len()..];
            }
            None => break,
        }
    }
    keys
}

/// Whether the listing says more results follow.
pub fn is_truncated(xml: &str) -> bool {
    element(xml, "IsTruncated").is_some_and(|value| value == "true")
}

/// The continuation token, when the service issued one.
pub fn parse_continuation(xml: &str) -> Option<String> {
    element(xml, "NextContinuationToken").filter(|token| !token.is_empty())
}

/// The text of the first `name` element.
fn element(xml: &str, name: &str) -> Option<String> {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&close)? + start;
    Some(unescape_xml(&xml[start..end]))
}

/// Undo the five entities XML requires.
fn unescape_xml(value: &str) -> String {
    value
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        // Ampersand last, so an escaped entity is not unescaped twice.
        .replace("&amp;", "&")
}

#[cfg(test)]
mod transport_tests {
    use super::*;

    fn storage() -> S3Storage {
        S3Storage::new(&FilesConfig {
            adapter: crate::config::StorageAdapter::S3,
            directory: String::new(),
            public_base_url: None,
            endpoint: Some("https://account.r2.cloudflarestorage.com".into()),
            bucket: Some("media".into()),
            region: "auto".into(),
            access_key: Some("AKIDEXAMPLE".into()),
            secret_key: Some("secret".into()),
        })
        .expect("storage")
    }

    #[test]
    fn the_debug_output_never_carries_the_credentials() {
        let rendered = format!("{:?}", storage());
        // The field names appear; the credential *values* must not.
        assert!(!rendered.contains("\"secret\""), "{rendered}");
        assert!(!rendered.contains("AKIDEXAMPLE"), "{rendered}");
    }

    #[test]
    fn a_timestamp_is_formatted_the_way_sigv4_requires() {
        assert_eq!(format_timestamp(0), "19700101T000000Z");
        // Two well-known instants, as a check on the calendar arithmetic.
        assert_eq!(format_timestamp(1_000_000_000), "20010909T014640Z");
        assert_eq!(format_timestamp(1_700_000_000), "20231114T221320Z");
        // The shape is always the same, which is what the signature depends on.
        let now = utc_timestamp();
        assert_eq!(now.len(), 16, "{now}");
        assert!(now.ends_with('Z'), "{now}");
        assert_eq!(now.as_bytes()[8], b'T', "{now}");
    }

    #[test]
    fn query_values_are_encoded_for_signing() {
        assert_eq!(encode_query_value("posts"), "posts");
        assert_eq!(encode_query_value("photos/a b.png"), "photos%2Fa%20b.png");
        // A slash has to be encoded in a query value, unlike in a path.
        assert!(!encode_query_value("a/b").contains('/'));
    }

    #[test]
    fn a_get_is_signed_with_the_empty_payload_hash() {
        let signed = storage().signed(
            "GET",
            "/media/posts/p1/photo.png",
            "",
            b"",
            None,
            "20260101T000000Z",
        );
        assert!(signed.authorization.starts_with(ALGORITHM));
        assert!(signed
            .authorization
            .contains("Credential=AKIDEXAMPLE/20260101/auto/s3/aws4_request"));
        assert!(signed
            .authorization
            .contains("SignedHeaders=host;x-amz-content-sha256;x-amz-date"));
        assert_eq!(signed.method, "GET");
        assert_eq!(signed.uri, "/media/posts/p1/photo.png");
        assert_eq!(signed.query, "");
    }

    #[test]
    fn a_put_signs_the_content_type_and_the_body() {
        let signed = storage().signed(
            "PUT",
            "/media/posts/p1/photo.png",
            "",
            b"the bytes",
            Some("image/png"),
            "20260101T000000Z",
        );
        assert!(signed
            .authorization
            .contains("SignedHeaders=content-type;host;x-amz-content-sha256;x-amz-date"));
        assert_eq!(signed.header("content-type"), Some("image/png"));
        // The body hash is the one for these bytes, not the empty hash.
        assert_eq!(
            signed.header("x-amz-content-sha256"),
            Some(payload_hash(b"the bytes").as_str())
        );
        assert_ne!(
            signed.header("x-amz-content-sha256"),
            Some(empty_payload_hash().as_str())
        );
    }

    #[test]
    fn the_same_request_at_a_different_time_signs_differently() {
        let storage = storage();
        let first = storage.signed("GET", "/media/k", "", b"", None, "20260101T000000Z");
        let second = storage.signed("GET", "/media/k", "", b"", None, "20260102T000000Z");
        assert_ne!(first.authorization, second.authorization);
    }

    #[test]
    fn keys_are_addressed_under_the_bucket() {
        let storage = storage();
        assert_eq!(
            storage.uri_for("posts/p1/photo.png"),
            "/media/posts/p1/photo.png"
        );
        // A key needing encoding is encoded, and the separators survive.
        assert_eq!(
            storage.uri_for("posts/p1/a b.png"),
            "/media/posts/p1/a%20b.png"
        );
        assert_eq!(storage.uri_for(""), "/media/");
    }

    #[test]
    fn a_listing_is_parsed_including_escaped_keys() {
        let xml = r#"<?xml version="1.0"?>
<ListBucketResult>
  <IsTruncated>true</IsTruncated>
  <Contents><Key>posts/p1/a.png</Key><Size>10</Size></Contents>
  <Contents><Key>posts/p1/b&amp;c.png</Key><Size>20</Size></Contents>
  <NextContinuationToken>1ueGcxLPRx1Tr</NextContinuationToken>
</ListBucketResult>"#;
        assert_eq!(
            parse_listing(xml),
            vec!["posts/p1/a.png".to_string(), "posts/p1/b&c.png".to_string()]
        );
        assert!(is_truncated(xml));
        assert_eq!(parse_continuation(xml).as_deref(), Some("1ueGcxLPRx1Tr"));
    }

    #[test]
    fn a_complete_listing_has_no_continuation() {
        let xml = r#"<ListBucketResult><IsTruncated>false</IsTruncated>
  <Contents><Key>a</Key></Contents></ListBucketResult>"#;
        assert!(!is_truncated(xml));
        assert_eq!(parse_continuation(xml), None);
        assert_eq!(parse_listing(xml), vec!["a".to_string()]);
    }

    #[test]
    fn an_empty_listing_parses_to_nothing_rather_than_failing() {
        let xml = r#"<ListBucketResult><IsTruncated>false</IsTruncated></ListBucketResult>"#;
        assert!(parse_listing(xml).is_empty());
        assert!(!is_truncated(xml));
        assert_eq!(parse_continuation(xml), None);
    }
}
