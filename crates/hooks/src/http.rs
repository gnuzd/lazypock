//! `$http`: the outbound requests a hook may make, and the rules they make them under.
//!
//! A hook is TypeScript from the deployment's own repository, but "own code" is not
//! the same as "safe to give the network to". The server usually sits inside a
//! private network with a metadata endpoint, an admin port and a database on it, and
//! a hook that can reach any of them turns one compromised template into a
//! compromised deployment. So the capability is bounded on every axis that matters:
//!
//! * **A host allowlist, empty by default.** `LAZYPOCK_HOOK_HTTP_ALLOWLIST` names the
//!   hosts egress is granted to; with nothing configured, every call is refused.
//!   Empty-means-everything is the default that gets found out too late.
//! * **The resolved address must be public.** The hostname is resolved here, every
//!   answer is checked against the private/loopback/link-local/reserved blocks, and
//!   the connection is pinned to the address that was checked — so a second DNS
//!   answer (the rebinding trick) cannot redirect it to `127.0.0.1` or
//!   `169.254.169.254` after the fact.
//! * **No redirects.** A redirect to another host is the classic allowlist bypass,
//!   and following one would need the whole check run again per hop. A 3xx is
//!   returned to the hook, which can follow it deliberately — and be checked again.
//! * **HTTPS only, a hard timeout, a response cap**, and nothing from the triggering
//!   request (no cookies, no authorization, no client headers) is ever forwarded:
//!   what the hook sends is exactly what the hook wrote.
//!
//! The blocking calls are deliberate. The hook runtime owns a thread of its own and
//! runs one dispatch at a time, so a synchronous request costs that dispatch and
//! nothing else — and the hook deadline (2 s) bounds it, with the per-call timeout
//! strictly inside it.

use std::io::Read;
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;

use lazypock_core::error::{Error, Result};
use lazypock_core::net::{is_public_ip, HostAllowlist};
use serde_json::{json, Map, Value};

/// The time one `$http` call may take, unless the hook asks for less.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_millis(1000);
/// The largest response a hook may read.
pub const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
/// The largest request body a hook may send.
pub const MAX_REQUEST_BODY_BYTES: usize = 256 * 1024;
/// The longest URL accepted.
pub const MAX_URL_LEN: usize = 2048;
/// How many request headers a hook may set.
const MAX_HEADERS: usize = 32;
/// How many response headers are handed back.
const MAX_RESPONSE_HEADERS: usize = 64;
/// How many bytes of response headers are handed back, all told.
const MAX_RESPONSE_HEADER_BYTES: usize = 16 * 1024;
const MAX_HEADER_NAME: usize = 64;
const MAX_HEADER_VALUE: usize = 4096;

/// The methods `$http` will send. Anything else is a typo or an attempt at a verb
/// that has no business leaving the server.
const ALLOWED_METHODS: [&str; 6] = ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD"];

/// Headers a hook may not set: they describe the connection or the framing, and
/// letting a caller write them is how request smuggling starts.
const FORBIDDEN_HEADERS: [&str; 8] = [
    "host",
    "content-length",
    "connection",
    "transfer-encoding",
    "upgrade",
    "proxy-authorization",
    "proxy-connection",
    "te",
];

/// One request a hook asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpCall {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
    /// The hook's own timeout, clamped to the dispatch budget in [`HookHttp::call`].
    pub timeout: Option<Duration>,
}

impl HttpCall {
    /// Read a call out of the JSON the JavaScript binding passes in.
    pub fn from_json(payload: &Value) -> Result<Self> {
        let method = payload
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or("GET")
            .trim()
            .to_ascii_uppercase();
        let url = payload
            .get("url")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::invalid("$http: a request needs a `url`"))?
            .trim()
            .to_string();

        let mut headers = Vec::new();
        match payload.get("headers") {
            None | Some(Value::Null) => {}
            Some(Value::Object(map)) => {
                for (name, value) in map {
                    let value = value
                        .as_str()
                        .ok_or_else(|| {
                            Error::invalid(format!("$http: the header {name:?} is not a string"))
                        })?
                        .to_string();
                    headers.push((name.clone(), value));
                }
            }
            Some(_) => {
                return Err(Error::invalid(
                    "$http: `headers` must be an object of string values",
                ))
            }
        }

        let body = match payload.get("body") {
            None | Some(Value::Null) => None,
            Some(Value::String(text)) => Some(text.clone()),
            // Anything else is serialized rather than refused: a hook posting a
            // record should not have to stringify it first.
            Some(other) => Some(other.to_string()),
        };

        let timeout = payload
            .get("timeoutMs")
            .and_then(Value::as_u64)
            .map(Duration::from_millis);

        Ok(Self {
            method,
            url,
            headers,
            body,
            timeout,
        })
    }
}

/// A response, as the hook sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl HttpResponse {
    /// The JSON handed back to the JavaScript binding.
    pub fn to_json(&self) -> Value {
        let mut headers = Map::new();
        for (name, value) in &self.headers {
            headers.insert(name.clone(), json!(value));
        }
        json!({
            "status": self.status,
            "headers": headers,
            "body": self.body,
        })
    }
}

/// Turns a host and port into addresses.
///
/// A seam because the interesting cases — a host that resolves to loopback, or one
/// that answers twice — cannot be arranged against real DNS in a test.
pub trait Resolver: Send + Sync {
    fn resolve(&self, host: &str, port: u16) -> Result<Vec<IpAddr>>;
}

/// Performs the request that [`HookHttp`] has already decided is allowed.
///
/// It receives the validated request and nothing else, so a transport cannot
/// accidentally send a header the gate refused. The address is not a hint: the
/// transport must connect to it, so the address that was checked is the address
/// that is reached.
pub trait Transport: Send + Sync {
    fn send(&self, request: &Prepared) -> Result<HttpResponse>;
}

/// The system resolver.
#[derive(Debug, Default)]
pub struct SystemResolver;

impl Resolver for SystemResolver {
    fn resolve(&self, host: &str, port: u16) -> Result<Vec<IpAddr>> {
        let addresses = (host, port)
            .to_socket_addrs()
            .map_err(|err| Error::internal(format!("$http: cannot resolve {host:?}: {err}")))?;
        let mut unique: Vec<IpAddr> = Vec::new();
        for address in addresses {
            if !unique.contains(&address.ip()) {
                unique.push(address.ip());
            }
        }
        Ok(unique)
    }
}

/// The real transport: one blocking request, no redirects, no proxy.
#[derive(Debug, Default)]
pub struct ReqwestTransport;

impl Transport for ReqwestTransport {
    fn send(&self, prepared: &Prepared) -> Result<HttpResponse> {
        // Pinned to the address the gate checked; a proxy would undo that, so
        // there is none — `HTTP_PROXY` in the environment is not the hook's to use.
        let client = reqwest::blocking::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .timeout(prepared.timeout)
            .connect_timeout(prepared.timeout.min(Duration::from_secs(1)))
            .user_agent(concat!("lazypock-hooks/", env!("CARGO_PKG_VERSION")))
            .resolve(&prepared.host, prepared.address)
            .build()
            .map_err(|err| Error::internal(format!("$http: cannot build the client: {err}")))?;

        let method = reqwest::Method::from_bytes(prepared.method.as_bytes())
            .map_err(|_| Error::invalid(format!("$http: {} is not a method", prepared.method)))?;
        let mut request = client.request(method, &prepared.url);
        for (name, value) in &prepared.headers {
            request = request.header(name.as_str(), value.as_str());
        }
        if let Some(body) = &prepared.body {
            request = request.body(body.clone());
        }

        let response = request
            .send()
            // `without_url` because reqwest's own message quotes the URL, query
            // string included; the gate adds the redacted one.
            .map_err(|err| Error::internal(err.without_url().to_string()))?;

        let status = response.status().as_u16();
        let mut headers: Vec<(String, String)> = Vec::new();
        let mut header_bytes = 0usize;
        for (name, value) in response.headers() {
            if headers.len() >= MAX_RESPONSE_HEADERS {
                break;
            }
            let value = String::from_utf8_lossy(value.as_bytes()).into_owned();
            header_bytes += name.as_str().len() + value.len();
            if header_bytes > MAX_RESPONSE_HEADER_BYTES {
                break;
            }
            headers.push((name.as_str().to_string(), value));
        }

        // One byte past the cap, so "exactly at the cap" is still readable and
        // "over it" is detectable without reading the whole response.
        let mut body = Vec::new();
        response
            .take(MAX_RESPONSE_BYTES as u64 + 1)
            .read_to_end(&mut body)
            .map_err(|err| Error::internal(format!("cannot read the response: {err}")))?;

        Ok(HttpResponse {
            status,
            headers,
            body: String::from_utf8_lossy(&body).into_owned(),
        })
    }
}

/// The outbound HTTP capability, with its rules.
pub struct HookHttp {
    allow: HostAllowlist,
    budget: Duration,
    resolver: Arc<dyn Resolver>,
    transport: Arc<dyn Transport>,
}

impl std::fmt::Debug for HookHttp {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HookHttp")
            .field("allowed_hosts", &self.allow.len())
            .field("budget", &self.budget)
            .finish_non_exhaustive()
    }
}

impl HookHttp {
    /// Build the gate for an allowlist, within a dispatch budget.
    pub fn new(allow: HostAllowlist, budget: Duration) -> Self {
        Self {
            allow,
            budget,
            resolver: Arc::new(SystemResolver),
            transport: Arc::new(ReqwestTransport),
        }
    }

    /// The gate as a hook sees it when no host is allowed: every call is refused.
    pub fn disabled(budget: Duration) -> Self {
        Self::new(HostAllowlist::default(), budget)
    }

    /// Build a gate around a given resolver and transport. Used by tests, and by an
    /// embedder that wants to route hook egress through its own client.
    #[doc(hidden)]
    pub fn with_parts(
        allow: HostAllowlist,
        budget: Duration,
        resolver: Arc<dyn Resolver>,
        transport: Arc<dyn Transport>,
    ) -> Self {
        Self {
            allow,
            budget,
            resolver,
            transport,
        }
    }

    /// Whether any host is reachable at all.
    pub fn allows_any(&self) -> bool {
        !self.allow.is_empty()
    }

    /// Check the call against every rule, then send it.
    pub fn call(&self, call: &HttpCall) -> Result<HttpResponse> {
        let prepared = self.prepare(call)?;
        // One place adds the URL to a message, and it adds the redacted form: a
        // transport's own error text may name the request, and a request may carry a
        // token in its query string.
        let response = self.transport.send(&prepared).map_err(|err| {
            Error::internal(format!(
                "$http: {}: {}",
                redact(&prepared.url),
                err.message()
            ))
        })?;
        // The transport stops reading one byte past the cap; refusing here keeps
        // that bound true for any transport an embedder supplies.
        if response.body.len() > MAX_RESPONSE_BYTES {
            return Err(Error::invalid(format!(
                "$http: the response from {} is larger than {MAX_RESPONSE_BYTES} bytes",
                redact(&prepared.url)
            )));
        }
        let header_bytes: usize = response
            .headers
            .iter()
            .map(|(name, value)| name.len() + value.len())
            .sum();
        if header_bytes > MAX_RESPONSE_HEADER_BYTES {
            return Err(Error::invalid(format!(
                "$http: the response headers from {} are larger than \
                 {MAX_RESPONSE_HEADER_BYTES} bytes",
                redact(&prepared.url)
            )));
        }
        Ok(response)
    }

    /// Everything decided before a socket is opened.
    fn prepare(&self, call: &HttpCall) -> Result<Prepared> {
        if call.url.len() > MAX_URL_LEN {
            return Err(Error::invalid(format!(
                "$http: a URL is limited to {MAX_URL_LEN} bytes"
            )));
        }
        let url = reqwest::Url::parse(&call.url)
            .map_err(|err| Error::invalid(format!("$http: {err}")))?;

        // HTTPS only. Plaintext to an allowlisted host would put the request (and
        // whatever it carries) on the wire for anyone on the path to read.
        if url.scheme() != "https" {
            return Err(Error::invalid(format!(
                "$http: {} is {}, and only https is allowed",
                redact(&call.url),
                url.scheme()
            )));
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Err(Error::invalid(
                "$http: credentials in a URL are refused; send an Authorization header instead",
            ));
        }
        let host = url
            .host_str()
            .ok_or_else(|| Error::invalid("$http: the request has no host"))?
            .to_ascii_lowercase()
            .trim_end_matches('.')
            .to_string();

        let port = url
            .port_or_known_default()
            .ok_or_else(|| Error::invalid(format!("$http: {} has no port", redact(&call.url))))?;

        if self.allow.is_empty() {
            return Err(Error::invalid(
                "$http: no hosts are allowed — set LAZYPOCK_HOOK_HTTP_ALLOWLIST to name them",
            ));
        }
        if !self.allow.allows(&host, port) {
            return Err(Error::invalid(format!(
                "$http: {host}:{port} is not in LAZYPOCK_HOOK_HTTP_ALLOWLIST"
            )));
        }

        if !ALLOWED_METHODS.contains(&call.method.as_str()) {
            return Err(Error::invalid(format!(
                "$http: {} is not an allowed method (allowed: {})",
                call.method,
                ALLOWED_METHODS.join(", ")
            )));
        }

        let headers = self.check_headers(&call.headers)?;
        let body = self.check_body(&call.method, call.body.as_deref())?;
        let timeout = call
            .timeout
            .unwrap_or(DEFAULT_TIMEOUT)
            .clamp(Duration::from_millis(50), self.budget);

        // Resolve once, check every answer, then pin the first. A host that answers
        // with one public and one private address is refused outright rather than
        // gambled on.
        let addresses = self.resolver.resolve(&host, port)?;
        if addresses.is_empty() {
            return Err(Error::invalid(format!(
                "$http: {host} resolved to no address"
            )));
        }
        for address in &addresses {
            if !is_public_ip(*address) {
                return Err(Error::invalid(format!(
                    "$http: {host} resolves to {address}, which is not a public address"
                )));
            }
        }

        Ok(Prepared {
            method: call.method.clone(),
            url: call.url.clone(),
            host,
            headers,
            body,
            address: SocketAddr::new(addresses[0], port),
            timeout,
        })
    }

    fn check_headers(&self, headers: &[(String, String)]) -> Result<Vec<(String, String)>> {
        if headers.len() > MAX_HEADERS {
            return Err(Error::invalid(format!(
                "$http: a request may set at most {MAX_HEADERS} headers"
            )));
        }
        let mut checked = Vec::with_capacity(headers.len());
        for (name, value) in headers {
            let lower = name.to_ascii_lowercase();
            if FORBIDDEN_HEADERS.contains(&lower.as_str()) {
                return Err(Error::invalid(format!(
                    "$http: the {name:?} header is set by the server, not by a hook"
                )));
            }
            if name.is_empty() || name.len() > MAX_HEADER_NAME || value.len() > MAX_HEADER_VALUE {
                return Err(Error::invalid(format!(
                    "$http: the header {name:?} is too long (name ≤ {MAX_HEADER_NAME}, value ≤ \
                     {MAX_HEADER_VALUE})"
                )));
            }
            // A newline in a header value is header injection.
            if name.chars().any(|c| c.is_control() || c == ' ' || c == ':')
                || value.chars().any(char::is_control)
            {
                return Err(Error::invalid(format!(
                    "$http: the header {name:?} has a character a header cannot hold"
                )));
            }
            checked.push((name.clone(), value.clone()));
        }
        Ok(checked)
    }

    fn check_body(&self, method: &str, body: Option<&str>) -> Result<Option<String>> {
        let Some(body) = body else {
            return Ok(None);
        };
        if body.len() > MAX_REQUEST_BODY_BYTES {
            return Err(Error::invalid(format!(
                "$http: a request body is limited to {MAX_REQUEST_BODY_BYTES} bytes"
            )));
        }
        if !matches!(method, "POST" | "PUT" | "PATCH") {
            return Err(Error::invalid(format!(
                "$http: {method} does not take a body"
            )));
        }
        if body.chars().any(|c| c == '\0') {
            return Err(Error::invalid("$http: a request body cannot hold a NUL"));
        }
        Ok(Some(body.to_string()))
    }
}

/// A URL as it may appear in an error message: scheme, host, port and path only.
///
/// A query string is exactly where a hook puts a token, and a hook failure is
/// logged — so the query and the fragment never reach a message, and neither do
/// credentials.
fn redact(url: &str) -> String {
    match reqwest::Url::parse(url) {
        Ok(mut parsed) => {
            parsed.set_query(None);
            parsed.set_fragment(None);
            let _ = parsed.set_username("");
            let _ = parsed.set_password(None);
            parsed.to_string()
        }
        Err(_) => "<unparseable url>".to_string(),
    }
}

/// What [`HookHttp::prepare`] decided, before anything is sent.
///
/// The transport gets this and only this: every field has been through the gate, so
/// there is no path from a hook to an unchecked header, method or address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prepared {
    pub method: String,
    pub url: String,
    /// The host that was checked, for the client's address pinning.
    pub host: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
    /// The address the host resolved to, which is the one to connect to.
    pub address: SocketAddr,
    pub timeout: Duration,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;
    use std::sync::Mutex;

    /// A resolver with answers written by the test.
    struct FakeResolver(Vec<IpAddr>);

    impl Resolver for FakeResolver {
        fn resolve(&self, _host: &str, _port: u16) -> Result<Vec<IpAddr>> {
            Ok(self.0.clone())
        }
    }

    /// A transport that records the request and answers with what the test says.
    struct FakeTransport {
        response: HttpResponse,
        seen: Mutex<Vec<Prepared>>,
    }

    impl Transport for FakeTransport {
        fn send(&self, request: &Prepared) -> Result<HttpResponse> {
            self.seen
                .lock()
                .expect("not poisoned")
                .push(request.clone());
            Ok(self.response.clone())
        }
    }

    fn gate(allow: &str, addresses: &[&str]) -> (HookHttp, Arc<FakeTransport>) {
        let transport = Arc::new(FakeTransport {
            response: HttpResponse {
                status: 200,
                headers: vec![("content-type".into(), "application/json".into())],
                body: "{\"ok\":true}".into(),
            },
            seen: Mutex::new(Vec::new()),
        });
        let resolver = Arc::new(FakeResolver(
            addresses
                .iter()
                .map(|raw| raw.parse().expect("an IP"))
                .collect(),
        ));
        let allow = HostAllowlist::parse(allow).expect("a valid allowlist");
        let gate = HookHttp::with_parts(
            allow,
            Duration::from_secs(2),
            resolver,
            Arc::clone(&transport) as Arc<dyn Transport>,
        );
        (gate, transport)
    }

    fn call(method: &str, url: &str) -> HttpCall {
        HttpCall {
            method: method.to_string(),
            url: url.to_string(),
            headers: Vec::new(),
            body: None,
            timeout: None,
        }
    }

    #[test]
    fn nothing_is_reachable_with_no_allowlist() {
        let (gate, transport) = gate("", &["93.184.216.34"]);
        assert!(!gate.allows_any());
        let err = gate
            .call(&call("GET", "https://example.com/x"))
            .expect_err("refused");
        assert!(
            format!("{err}").contains("LAZYPOCK_HOOK_HTTP_ALLOWLIST"),
            "the message says how to allow it: {err}"
        );
        assert!(
            transport.seen.lock().expect("not poisoned").is_empty(),
            "a refused call never reaches the transport"
        );
    }

    #[test]
    fn a_host_that_is_not_allowlisted_is_refused() {
        let (gate, _) = gate("api.example.com", &["93.184.216.34"]);
        let err = gate
            .call(&call("GET", "https://evil.example.net/x"))
            .expect_err("refused");
        assert!(format!("{err}").contains("not in LAZYPOCK_HOOK_HTTP_ALLOWLIST"));
    }

    #[test]
    fn plaintext_and_credentials_in_a_url_are_refused() {
        let (gate, _) = gate("api.example.com", &["93.184.216.34"]);
        for url in [
            "http://api.example.com/x",
            "https://user:pass@api.example.com/x",
            "ftp://api.example.com/x",
        ] {
            assert!(
                gate.call(&call("GET", url)).is_err(),
                "{url} should be refused"
            );
        }
    }

    #[test]
    fn an_allowlisted_host_that_resolves_inside_the_network_is_refused() {
        // The allowlist is not the whole check: `localhost` is a name an operator
        // could plausibly allow, and it must still not reach loopback.
        for address in ["127.0.0.1", "169.254.169.254", "10.0.0.5", "::1", "fd00::1"] {
            let (gate, transport) = gate("localhost", &[address]);
            let err = gate
                .call(&call("GET", "https://localhost/x"))
                .expect_err("refused");
            assert!(
                format!("{err}").contains("not a public address"),
                "{address}: {err}"
            );
            assert!(transport.seen.lock().expect("not poisoned").is_empty());
        }
    }

    #[test]
    fn one_private_answer_among_public_ones_refuses_the_call() {
        // The rebinding case: the second answer would be the one a resolver-caching
        // client picked up.
        let (gate, _) = gate("api.example.com", &["93.184.216.34", "127.0.0.1"]);
        assert!(gate
            .call(&call("GET", "https://api.example.com/x"))
            .is_err());
    }

    #[test]
    fn the_checked_address_is_the_one_the_transport_is_given() {
        let (gate, transport) = gate("api.example.com", &["93.184.216.34"]);
        let response = gate
            .call(&call("GET", "https://api.example.com/x"))
            .expect("sent");
        assert_eq!(response.status, 200);
        assert_eq!(response.body, "{\"ok\":true}");

        let seen = transport.seen.lock().expect("not poisoned");
        assert_eq!(
            seen[0].address,
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34)), 443)
        );
        // The default is inside the dispatch budget, so a dead host cannot hold a
        // request open past the hook deadline.
        assert_eq!(seen[0].timeout, DEFAULT_TIMEOUT);
    }

    #[test]
    fn a_hook_cannot_write_the_headers_that_frame_the_connection() {
        let (gate, _) = gate("api.example.com", &["93.184.216.34"]);
        for name in ["Host", "Content-Length", "Transfer-Encoding", "Connection"] {
            let mut request = call("POST", "https://api.example.com/x");
            request.body = Some("{}".into());
            request.headers = vec![(name.to_string(), "x".into())];
            let err = gate.call(&request).expect_err("refused");
            assert!(
                format!("{err}").contains("set by the server"),
                "{name}: {err}"
            );
        }
        // A newline in a value never gets out either.
        let mut injected = call("GET", "https://api.example.com/x");
        injected.headers = vec![("x-test".into(), "a\r\nHost: evil".into())];
        assert!(gate.call(&injected).is_err());
    }

    #[test]
    fn a_body_is_bounded_and_only_sent_where_it_belongs() {
        let (gate, _) = gate("api.example.com", &["93.184.216.34"]);

        let mut get_with_body = call("GET", "https://api.example.com/x");
        get_with_body.body = Some("{}".into());
        assert!(gate.call(&get_with_body).is_err());

        let mut huge = call("POST", "https://api.example.com/x");
        huge.body = Some("x".repeat(MAX_REQUEST_BODY_BYTES + 1));
        let err = gate.call(&huge).expect_err("refused");
        assert!(format!("{err}").contains("limited to"));
    }

    #[test]
    fn only_the_documented_methods_are_sent() {
        let (gate, _) = gate("api.example.com", &["93.184.216.34"]);
        for method in ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD"] {
            let mut request = call(method, "https://api.example.com/x");
            if method == "POST" || method == "PUT" || method == "PATCH" {
                request.body = Some("{}".into());
            }
            gate.call(&request)
                .unwrap_or_else(|err| panic!("{method}: {err}"));
        }
        assert!(gate
            .call(&call("TRACE", "https://api.example.com/x"))
            .is_err());
        assert!(gate
            .call(&call("CONNECT", "https://api.example.com/x"))
            .is_err());
    }

    #[test]
    fn a_hooks_timeout_is_clamped_to_the_dispatch_budget() {
        let (gate, transport) = gate("api.example.com", &["93.184.216.34"]);

        let mut request = call("GET", "https://api.example.com/x");
        request.timeout = Some(Duration::from_secs(60));
        gate.call(&request).expect("sent");
        assert_eq!(
            transport.seen.lock().expect("not poisoned")[0].timeout,
            Duration::from_secs(2),
            "a hook cannot ask for more than the dispatch budget"
        );

        let mut quick = call("GET", "https://api.example.com/x");
        quick.timeout = Some(Duration::from_millis(1));
        gate.call(&quick).expect("sent");
        assert_eq!(
            transport.seen.lock().expect("not poisoned")[1].timeout,
            Duration::from_millis(50),
            "and not for less than a floor that would only produce noise"
        );
    }

    #[test]
    fn an_allowlist_entry_grants_its_port_and_no_other() {
        let (gate, transport) = gate("api.example.com:8443", &["93.184.216.34"]);

        // The same host on the default port is a different grant, and is refused.
        let err = gate
            .call(&call("GET", "https://api.example.com/x"))
            .expect_err("refused");
        assert!(format!("{err}").contains("api.example.com:443"), "{err}");

        gate.call(&call("GET", "https://api.example.com:8443/x"))
            .expect("allowed");
        assert_eq!(
            transport.seen.lock().expect("not poisoned")[0]
                .address
                .port(),
            8443
        );
    }

    #[test]
    fn an_error_message_never_carries_the_query_string() {
        // A hook that puts a token in a query string must not see it echoed into a
        // message that the server then logs.
        struct Failing;

        impl Transport for Failing {
            fn send(&self, _request: &Prepared) -> Result<HttpResponse> {
                Err(Error::internal("connection refused"))
            }
        }

        let gate = HookHttp::with_parts(
            HostAllowlist::parse("api.example.com").expect("allowlist"),
            Duration::from_secs(2),
            Arc::new(FakeResolver(vec!["93.184.216.34".parse().expect("an IP")])),
            Arc::new(Failing) as Arc<dyn Transport>,
        );

        let err = gate
            .call(&call(
                "GET",
                "https://api.example.com/x?token=super-secret&other=1",
            ))
            .expect_err("fails");
        let message = format!("{err}");
        assert!(
            !message.contains("super-secret") && !message.contains("other=1"),
            "the query must not reach the message: {message}"
        );
        assert!(
            message.contains("api.example.com/x"),
            "the useful part stays: {message}"
        );

        // Neither does a plaintext URL's credentials, which the scheme check
        // refuses before anything else looks at it.
        let scheme = gate
            .call(&call("GET", "http://user:pass@api.example.com/x"))
            .expect_err("refused");
        let scheme = format!("{scheme}");
        assert!(!scheme.contains("pass"), "{scheme}");
    }

    #[test]
    fn oversized_response_headers_are_refused() {
        // A transport that ignores the cap still cannot get past the gate.
        let huge: Vec<(String, String)> = (0..80)
            .map(|index| (format!("x-header-{index}"), "v".repeat(1024)))
            .collect();
        let gate = HookHttp::with_parts(
            HostAllowlist::parse("api.example.com").expect("allowlist"),
            Duration::from_secs(2),
            Arc::new(FakeResolver(vec!["93.184.216.34".parse().expect("an IP")])),
            Arc::new(FixedTransport(HttpResponse {
                status: 200,
                headers: huge,
                body: String::new(),
            })) as Arc<dyn Transport>,
        );
        let err = gate
            .call(&call("GET", "https://api.example.com/x"))
            .expect_err("refused");
        assert!(format!("{err}").contains("headers"), "{err}");
    }

    #[test]
    fn an_oversized_body_from_a_transport_is_refused() {
        let gate = HookHttp::with_parts(
            HostAllowlist::parse("api.example.com").expect("allowlist"),
            Duration::from_secs(2),
            Arc::new(FakeResolver(vec!["93.184.216.34".parse().expect("an IP")])),
            Arc::new(FixedTransport(HttpResponse {
                status: 200,
                headers: Vec::new(),
                body: "x".repeat(MAX_RESPONSE_BYTES + 1),
            })) as Arc<dyn Transport>,
        );
        let err = gate
            .call(&call("GET", "https://api.example.com/x"))
            .expect_err("refused");
        assert!(format!("{err}").contains("larger than"), "{err}");
    }

    /// A transport that always answers with one fixed response.
    struct FixedTransport(HttpResponse);

    impl Transport for FixedTransport {
        fn send(&self, _request: &Prepared) -> Result<HttpResponse> {
            Ok(self.0.clone())
        }
    }

    #[test]
    fn a_call_is_read_out_of_the_json_the_binding_passes() {
        let payload = json!({
            "method": "post",
            "url": "https://api.example.com/x",
            "headers": { "Content-Type": "application/json" },
            "body": { "id": "p1" },
            "timeoutMs": 500,
        });
        let call = HttpCall::from_json(&payload).expect("parsed");
        assert_eq!(call.method, "POST");
        assert_eq!(
            call.headers,
            vec![("Content-Type".to_string(), "application/json".to_string())]
        );
        // An object body is serialized rather than refused.
        assert_eq!(call.body.as_deref(), Some("{\"id\":\"p1\"}"));
        assert_eq!(call.timeout, Some(Duration::from_millis(500)));

        assert!(HttpCall::from_json(&json!({ "method": "GET" })).is_err());
    }

    #[test]
    fn a_response_becomes_the_json_the_hook_reads() {
        let response = HttpResponse {
            status: 204,
            headers: vec![("x-request-id".into(), "abc".into())],
            body: String::new(),
        };
        let json = response.to_json();
        assert_eq!(json["status"], 204);
        assert_eq!(json["headers"]["x-request-id"], "abc");
        assert_eq!(json["body"], "");
    }
}
