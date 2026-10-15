//! An optional external image service.
//!
//! Resizing runs **in this process** by default (see [`crate::thumbnail`]). Setting
//! `LAZYPOCK_IMAGE_SERVICE_URL` moves that work out: the source bytes and the
//! preset's parameters are POSTed to the service, and the encoded image comes back.
//! That takes the decode/encode CPU and, more importantly, its peak memory out of
//! the API process, where a single large image would otherwise be handled by a
//! request worker with no cgroup of its own.
//!
//! # The contract
//!
//! ```text
//! POST <LAZYPOCK_IMAGE_SERVICE_URL>?width=320&height=320&fit=inside&format=webp&quality=80
//! content-type: image/png            # the source's sniffed format
//! authorization: Bearer <token>      # only when LAZYPOCK_IMAGE_SERVICE_TOKEN is set
//!
//! <the source bytes>
//! ```
//!
//! The **response body** is the encoded image and its `Content-Type` is the format
//! to serve it as. A missing `width`/`height` means "derive it from the source",
//! `fit` is one of `cover|contain|fill|inside|outside`, `format` one of
//! `webp|png|jpeg|avif` and `quality` is 1-100 — the same vocabulary
//! `_image_presets` stores.
//!
//! Anything other than a 2xx is an error, and a `4xx` is reported as such to the
//! caller. A failure is **never** papered over by falling back to the local codecs:
//! a deployment that configured a service did so to keep the CPU elsewhere, and a
//! silent fallback would hide both the outage and the reason for the setting.
//!
//! Implementing the service is a small job in any language: decode, resize, encode.
//! `cargo build --no-default-features` builds a lazypock without codecs of its own,
//! which is what a thin API and a separate image service look like together.

use std::time::Duration;

use lazypock_core::error::{Error, Result};

use crate::thumbnail::ResizeRequest;

/// How long a resize may take before it is given up on.
///
/// A hung service must not hold an API request open indefinitely; the caller can
/// retry, and the thumbnail is generated and cached on the next attempt.
const TIMEOUT: Duration = Duration::from_secs(30);

/// How much of a service's error body is quoted back.
const BODY_LIMIT: usize = 300;

/// A configured external image service.
#[derive(Clone)]
pub struct ImageService {
    url: reqwest::Url,
    token: Option<String>,
    client: reqwest::Client,
}

impl std::fmt::Debug for ImageService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The token is a credential, so it is never printed.
        f.debug_struct("ImageService")
            .field("url", &self.url.as_str())
            .field("token", &self.token.as_ref().map(|_| "[redacted]"))
            .finish()
    }
}

impl ImageService {
    /// Build the client for a configured service.
    ///
    /// The URL is parsed here so a bad one is refused at boot rather than on the
    /// first thumbnail.
    pub fn new(url: String, token: Option<String>) -> Result<Self> {
        let parsed = reqwest::Url::parse(&url).map_err(|err| {
            Error::config(format!(
                "LAZYPOCK_IMAGE_SERVICE_URL is not a valid URL ({url:?}): {err}"
            ))
        })?;

        let client = reqwest::Client::builder()
            .timeout(TIMEOUT)
            .build()
            .map_err(|err| {
                Error::config(format!("cannot build the image service client: {err}"))
            })?;

        Ok(Self {
            url: parsed,
            token,
            client,
        })
    }

    /// The configured URL, for logs and the configuration summary.
    pub fn url(&self) -> &str {
        self.url.as_str()
    }

    /// Whether a shared secret is configured.
    pub fn has_token(&self) -> bool {
        self.token.is_some()
    }

    /// Ask the service for one resized image.
    pub async fn resize(&self, request: &ResizeRequest<'_>) -> Result<Vec<u8>> {
        let mut parameters: Vec<(&str, String)> = Vec::with_capacity(6);
        if let Some(width) = request.width {
            parameters.push(("width", width.to_string()));
        }
        if let Some(height) = request.height {
            parameters.push(("height", height.to_string()));
        }
        parameters.push(("fit", request.mode.as_str().to_string()));
        parameters.push(("format", request.format.as_str().to_string()));
        parameters.push(("quality", request.quality.to_string()));

        // Built with `Url` rather than `RequestBuilder::query`, which needs a reqwest
        // feature this crate does not otherwise use.
        let mut url = self.url.clone();
        {
            let mut pairs = url.query_pairs_mut();
            for (name, value) in &parameters {
                pairs.append_pair(name, value);
            }
        }

        let mut call = self
            .client
            .post(url)
            .header(
                reqwest::header::CONTENT_TYPE,
                request.source_format.content_type(),
            )
            .header(reqwest::header::ACCEPT, "image/*")
            .body(request.source.to_vec());

        if let Some(token) = &self.token {
            call = call.bearer_auth(token);
        }

        let response = call.send().await.map_err(|err| {
            Error::internal(format!(
                "the image service at {} could not be reached: {err}",
                self.url
            ))
        })?;

        let status = response.status();
        let body = response.bytes().await.map_err(|err| {
            Error::internal(format!(
                "the image service at {} returned an unreadable body: {err}",
                self.url
            ))
        })?;

        if !status.is_success() {
            let detail = summarise(&body);
            // A refusal is the caller's problem (an undecodable source, an
            // unsupported format); a 5xx is ours. Both say where they came from.
            let message = format!(
                "the image service at {} refused the resize ({}): {detail}",
                self.url, status
            );
            return Err(if status.is_client_error() {
                Error::invalid(message)
            } else {
                Error::internal(message)
            });
        }

        if body.is_empty() {
            return Err(Error::internal(format!(
                "the image service at {} returned an empty image",
                self.url
            )));
        }

        Ok(body.to_vec())
    }
}

/// A service's error body, quoted back to the caller but bounded.
fn summarise(body: &[u8]) -> String {
    let text = String::from_utf8_lossy(body);
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return "no detail given".to_string();
    }
    if trimmed.chars().count() > BODY_LIMIT {
        let head: String = trimmed.chars().take(BODY_LIMIT).collect();
        return format!("{head}…");
    }
    trimmed.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::thumbnail::{OutputFormat, ResizeMode, ResizeRequest};
    use crate::validation::ImageFormat;
    use std::io::{Read, Write};
    use std::sync::{Arc, Mutex};

    /// The request every test below sends.
    fn request<'a>(source: &'a [u8]) -> ResizeRequest<'a> {
        ResizeRequest {
            source,
            source_format: ImageFormat::Png,
            width: Some(320),
            height: Some(320),
            mode: ResizeMode::Inside,
            format: OutputFormat::Webp,
            quality: 80,
        }
    }

    /// A stub image service: answers every request with `status` and `body`, and
    /// records the raw request heads it saw.
    fn stub(status: u16, body: &'static [u8]) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let address = listener.local_addr().expect("address");
        let seen = Arc::new(Mutex::new(Vec::new()));
        let recorder = Arc::clone(&seen);

        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let mut received: Vec<u8> = Vec::new();
                let mut buffer = [0u8; 4096];
                loop {
                    let read = stream.read(&mut buffer).unwrap_or(0);
                    if read == 0 {
                        break;
                    }
                    received.extend_from_slice(&buffer[..read]);
                    if let Some(head_end) = find(&received, b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&received[..head_end]).to_string();
                        let length = content_length(&head);
                        if received.len() >= head_end + 4 + length {
                            recorder.lock().expect("lock").push(head);
                            break;
                        }
                    }
                }

                let head = format!(
                    "HTTP/1.1 {status} STUB\r\ncontent-type: image/webp\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(body);
            }
        });

        (format!("http://{address}/resize"), seen)
    }

    fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        haystack
            .windows(needle.len())
            .position(|window| window == needle)
    }

    fn content_length(head: &str) -> usize {
        head.lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.trim()
                    .eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse().ok())?
            })
            .unwrap_or(0)
    }

    #[tokio::test]
    async fn a_configured_service_does_the_work_and_is_sent_the_contract() {
        let (url, seen) = stub(200, b"FROM-THE-SERVICE");
        let service = ImageService::new(url, None).expect("service");

        let bytes = service
            .resize(&request(b"a source image"))
            .await
            .expect("resize");
        assert_eq!(bytes, b"FROM-THE-SERVICE");

        let calls = seen.lock().expect("lock");
        assert_eq!(calls.len(), 1);
        let head = &calls[0];
        assert!(head.starts_with("POST /resize?"), "{head}");
        for expected in [
            "width=320",
            "height=320",
            "fit=inside",
            "format=webp",
            "quality=80",
        ] {
            assert!(head.contains(expected), "{expected} missing from {head}");
        }
        // The source format is declared, so the service does not have to sniff it.
        assert!(
            head.to_lowercase().contains("content-type: image/png"),
            "{head}"
        );
        // No token was configured, so none is sent.
        assert!(!head.to_lowercase().contains("authorization"), "{head}");
    }

    #[tokio::test]
    async fn the_configured_token_is_sent_as_a_bearer() {
        let (url, seen) = stub(200, b"ok");
        let service = ImageService::new(url, Some("s3cret".into())).expect("service");

        service
            .resize(&request(b"a source image"))
            .await
            .expect("resize");

        let calls = seen.lock().expect("lock");
        assert!(
            calls[0]
                .to_lowercase()
                .contains("authorization: bearer s3cret"),
            "{}",
            calls[0]
        );
    }

    #[tokio::test]
    async fn the_dispatcher_prefers_the_service_over_the_local_codecs() {
        let (url, _) = stub(200, b"FROM-THE-SERVICE");
        let service = ImageService::new(url, None).expect("service");

        // With codecs compiled in, a real PNG would come back as a real PNG. Getting
        // the stub's bytes instead is the proof that the service did the work.
        let bytes = crate::thumbnail::resize(request(b"not really a png"), Some(&service))
            .await
            .expect("resize");
        assert_eq!(bytes, b"FROM-THE-SERVICE");
    }

    #[tokio::test]
    async fn a_service_refusal_is_an_error_that_quotes_it() {
        let (url, _) = stub(500, b"no workers available");
        let service = ImageService::new(url, None).expect("service");

        let err = service
            .resize(&request(b"a source image"))
            .await
            .expect_err("refused");
        assert!(
            err.message().contains("no workers available"),
            "{}",
            err.message()
        );
        assert!(err.message().contains("500"), "{}", err.message());
    }

    #[tokio::test]
    async fn an_unreachable_service_fails_instead_of_resizing_locally() {
        // A port nothing is listening on: bound, then released.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let address = listener.local_addr().expect("address");
        drop(listener);

        let service = ImageService::new(format!("http://{address}/resize"), None).expect("service");

        // The local codecs are compiled in here, so a silent fallback would succeed.
        // The whole point of configuring a service is that the work happens there, so
        // it must not.
        let err = crate::thumbnail::resize(request(b"a source image"), Some(&service))
            .await
            .expect_err("unreachable");
        assert!(
            err.message().contains("could not be reached"),
            "{}",
            err.message()
        );
    }

    #[test]
    fn a_service_error_body_is_bounded() {
        assert_eq!(summarise(b"  "), "no detail given");
        assert_eq!(summarise(b"nope"), "nope");
        let long = "x".repeat(500);
        let shortened = summarise(long.as_bytes());
        assert!(shortened.ends_with('…'), "{shortened}");
        assert_eq!(shortened.chars().count(), BODY_LIMIT + 1);
    }

    #[test]
    fn a_token_is_never_printed() {
        let service = ImageService::new("https://images.example.com".into(), Some("s3cret".into()))
            .expect("client");
        let debug = format!("{service:?}");
        assert!(!debug.contains("s3cret"), "{debug}");
        assert!(debug.contains("redacted"), "{debug}");
        assert!(service.has_token());
    }
}
