//! The mailer: an SMTP transport, operator-editable templates, and the payload a
//! send job carries.
//!
//! Three pieces, deliberately separate:
//!
//! * [`MailConfig`] is read from the `mail` setting **at send time**, so a
//!   superuser can change SMTP settings in the Studio without a restart — which is
//!   what the reference does.
//! * [`Templates`] renders the subject, the HTML body and the plain-text
//!   alternative with [MiniJinja](https://docs.rs/minijinja). Three layers, newest
//!   first: a body **saved through the API** (stored in `_mail_templates`), a **file**
//!   in a template directory, and the **built-in** compiled into the binary. Which
//!   layer a part came from is reported alongside it, so an editor can say whether an
//!   edit is in effect and what a reset would restore. A deployment can add a name
//!   (`receipt.subject`, `receipt.html`, `receipt.text`) from either layer, which is
//!   how application mail is sent from `POST /api/mail`.
//! * [`Mailer`] joins a rendered message to a transport. [`Mailer::capture`]
//!   records instead of sending, which is what the tests use and what a
//!   deployment can use to inspect what would go out.
//!
//! Sending is called from the job queue's `mail` kind, never from a request
//! handler: a provider that is slow or down must not hold a request open.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::{Arc, Mutex};

use lazypock_core::error::{Error, Result};
use lettre::message::{Mailbox, MultiPart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Tokio1Executor};
use minijinja::{Environment, UndefinedBehavior};
use serde_json::{json, Map, Value};

/// The templates shipped in the binary.
///
/// A file with the same `<name>.<part>` name in the template directory overrides
/// the one here.
pub const BUILTIN_NAMES: [&str; 4] = ["verification", "password_reset", "email_change", "otp"];

/// How the SMTP connection is secured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encryption {
    /// No TLS. Only for a trusted network or a local relay.
    None,
    /// STARTTLS, normally on port 587.
    Starttls,
    /// Implicit TLS, normally on port 465.
    Tls,
}

impl Encryption {
    /// Parse `mail.encryption`; `None` for an unrecognised value.
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "none" | "plain" => Some(Self::None),
            "starttls" | "start_tls" => Some(Self::Starttls),
            "tls" | "ssl" | "implicit" => Some(Self::Tls),
            _ => None,
        }
    }

    /// The port assumed when `mail.port` is absent.
    pub fn default_port(self) -> u16 {
        match self {
            Self::None => 25,
            Self::Starttls => 587,
            Self::Tls => 465,
        }
    }
}

/// Everything needed to reach an SMTP server and to address a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailConfig {
    /// SMTP host.
    pub host: String,
    /// SMTP port.
    pub port: u16,
    /// SMTP username, when the server authenticates.
    pub username: Option<String>,
    /// SMTP password; set together with [`MailConfig::username`].
    pub password: Option<String>,
    /// How the connection is secured.
    pub encryption: Encryption,
    /// The display name on the `From` header.
    pub sender_name: String,
    /// The address on the `From` header.
    pub sender_address: String,
    /// The application name, offered to templates as `app_name`.
    pub app_name: String,
}

impl MailConfig {
    /// Read the `mail` setting.
    ///
    /// `Ok(None)` means no `mail` setting is present, which is "mail is not
    /// configured" rather than an error. Once a `mail` object exists, a value it
    /// gets wrong is an error: a mailer that silently does not send is worse than
    /// one that refuses to start sending.
    pub fn from_settings(settings: &Value) -> Result<Option<Self>> {
        let Some(mail) = settings.get("mail").filter(|value| value.is_object()) else {
            return Ok(None);
        };

        let text = |key: &str| {
            mail.get(key)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        };

        let host = text("host").ok_or_else(|| {
            Error::config("mail.host is required when a `mail` setting is present")
        })?;

        let encryption = match text("encryption") {
            Some(raw) => Encryption::parse(&raw).ok_or_else(|| {
                Error::config(format!(
                    "mail.encryption must be one of none|starttls|tls, got {raw:?}"
                ))
            })?,
            None => Encryption::Starttls,
        };

        let port = match mail.get("port") {
            Some(Value::Number(number)) => {
                number.as_u64().and_then(|port| u16::try_from(port).ok())
            }
            Some(Value::String(raw)) => raw.trim().parse::<u16>().ok(),
            None => Some(encryption.default_port()),
            _ => None,
        }
        .filter(|port| *port > 0)
        .ok_or_else(|| Error::config("mail.port must be a number between 1 and 65535"))?;

        let username = text("username");
        let password = text("password");
        if username.is_some() != password.is_some() {
            return Err(Error::config(
                "mail.username and mail.password must be set together",
            ));
        }

        let sender_address =
            text("sender_address").unwrap_or_else(|| "noreply@lazypock.app".to_string());
        if !sender_address.contains('@') {
            return Err(Error::config(format!(
                "mail.sender_address must be an email address, got {sender_address:?}"
            )));
        }
        let sender_name = text("sender_name").unwrap_or_else(|| "lazypock".to_string());

        let app_name = settings
            .get("app_name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("lazypock")
            .to_string();

        Ok(Some(Self {
            host,
            port,
            username,
            password,
            encryption,
            sender_name,
            sender_address,
            app_name,
        }))
    }
}

/// Which part of a template is being rendered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    Subject,
    Html,
    Text,
}

impl Part {
    /// Every part, in the order an editor shows them.
    const ALL: [Part; 3] = [Part::Subject, Part::Html, Part::Text];

    fn suffix(self) -> &'static str {
        match self {
            Self::Subject => "subject",
            Self::Html => "html",
            Self::Text => "text",
        }
    }

    /// The part a stored row names, or `None` for anything else.
    fn parse(raw: &str) -> Option<Self> {
        match raw {
            "subject" => Some(Self::Subject),
            "html" => Some(Self::Html),
            "text" => Some(Self::Text),
            _ => None,
        }
    }
}

/// The layer a template body was resolved from.
///
/// Reporting it is what lets an editor say whether an edit is in effect and what a
/// reset would restore: a body served from the database and one served from the
/// binary are otherwise indistinguishable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// Saved through the API and stored in `_mail_templates`.
    Database,
    /// A file in the template directory.
    File,
    /// Compiled into the binary.
    Builtin,
}

impl Origin {
    /// The name the API reports this layer by.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Database => "database",
            Self::File => "file",
            Self::Builtin => "builtin",
        }
    }
}

/// One resolved part of a template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// `subject`, `html` or `text`.
    pub part: &'static str,
    /// The body to render.
    pub body: String,
    /// Where the body came from.
    pub origin: Origin,
}

/// A body applied on top of the files and built-ins.
///
/// Named rather than a tuple so a caller building one from a database row cannot
/// transpose the part and the body.
#[derive(Debug, Clone)]
pub struct Override {
    /// The template name.
    pub name: String,
    /// `subject`, `html` or `text`.
    pub part: String,
    /// The body.
    pub body: String,
}

/// All three rendered parts of one template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    /// The rendered subject.
    pub subject: String,
    /// The rendered HTML body.
    pub html: String,
    /// The rendered plain-text alternative.
    pub text: String,
}

/// One part's body and the layer it came from.
#[derive(Debug, Clone)]
struct Entry {
    body: String,
    origin: Origin,
}

/// The templates this node serves: the built-in defaults, the file overrides in the
/// template directory, and the bodies saved through the API.
pub struct Templates {
    env: Environment<'static>,
    /// Every template name the environment holds, built-in and custom.
    names: BTreeSet<String>,
    /// The body of every part, with the layer it was resolved from.
    entries: BTreeMap<(String, &'static str), Entry>,
}

impl Templates {
    /// Load the built-in templates, applying file overrides from `directory`.
    ///
    /// A template with a syntax error is refused here rather than when the first
    /// email is sent, so a bad edit is caught at boot.
    pub fn load(directory: Option<&Path>) -> Result<Self> {
        Self::load_with(directory, &[])
    }

    /// Load the templates, then apply `overrides` on top of the files and built-ins.
    pub fn load_with(directory: Option<&Path>, overrides: &[Override]) -> Result<Self> {
        let mut entries: BTreeMap<(String, &'static str), Entry> = BTreeMap::new();

        for name in BUILTIN_NAMES {
            for part in Part::ALL {
                let key = format!("{name}.{}", part.suffix());
                let (body, origin) = match read_override(directory, &key)? {
                    Some(body) => (body, Origin::File),
                    None => (
                        builtin(name, part)
                            .ok_or_else(|| {
                                Error::internal(format!("no built-in template {key:?}"))
                            })?
                            .to_string(),
                        Origin::Builtin,
                    ),
                };
                entries.insert((name.to_string(), part.suffix()), Entry { body, origin });
            }
        }

        // Application mail: a name the directory defines beyond the built-ins.
        for name in custom_names(directory)? {
            for part in Part::ALL {
                let key = format!("{name}.{}", part.suffix());
                let Some(body) = read_override(directory, &key)? else {
                    continue;
                };
                entries.insert(
                    (name.clone(), part.suffix()),
                    Entry {
                        body,
                        origin: Origin::File,
                    },
                );
            }
        }

        apply(&mut entries, overrides)?;
        Self::from_entries(entries)
    }

    /// A copy of these templates with `overrides` applied on top.
    ///
    /// The bodies already resolved are carried over rather than re-read, so an
    /// override changes only the parts it names and cannot disturb the layers beneath
    /// it.
    pub fn with_overrides(&self, overrides: &[Override]) -> Result<Self> {
        let mut entries = self.entries.clone();
        apply(&mut entries, overrides)?;
        Self::from_entries(entries)
    }

    /// Build the environment from a resolved body map.
    ///
    /// The one construction path, so a template loaded at boot and one saved through
    /// the API are validated the same way. A name that is only partly there is refused
    /// here — at boot, or at save time — rather than at the first send, because "the
    /// email arrived without a body" is not a failure anyone notices in time.
    fn from_entries(entries: BTreeMap<(String, &'static str), Entry>) -> Result<Self> {
        let names: BTreeSet<String> = entries.keys().map(|(name, _)| name.clone()).collect();
        for name in &names {
            for part in Part::ALL {
                if !entries.contains_key(&(name.clone(), part.suffix())) {
                    return Err(Error::config(format!(
                        "mail template {name:?} is incomplete: it needs a subject, an html \
                         and a text part"
                    )));
                }
            }
        }

        let mut env = Environment::new();
        // A variable a template uses but the caller did not supply is an error:
        // an OTP email rendered with an empty code is worse than one that fails.
        env.set_undefined_behavior(UndefinedBehavior::Strict);
        for ((name, part), entry) in &entries {
            env.add_template_owned(format!("{name}.{part}"), entry.body.clone())
                .map_err(|err| Error::config(format!("mail template {name}.{part}: {err}")))?;
        }

        Ok(Self {
            env,
            names,
            entries,
        })
    }

    /// Every part of one template, in editor order, with the layer each came from.
    pub fn template(&self, name: &str) -> Option<Vec<Resolved>> {
        if !self.names.contains(name) {
            return None;
        }
        Some(
            Part::ALL
                .iter()
                .map(|part| {
                    let entry = &self.entries[&(name.to_string(), part.suffix())];
                    Resolved {
                        part: part.suffix(),
                        body: entry.body.clone(),
                        origin: entry.origin,
                    }
                })
                .collect(),
        )
    }

    /// Check that the parts of `name` parse, before they are stored.
    ///
    /// Parsing in a scratch environment is what makes a syntax error a rejected save
    /// rather than a job that fails later, on an email nobody is watching.
    pub fn check(name: &str, parts: &[(&str, &str)]) -> Result<()> {
        let mut env = Environment::new();
        for (part, body) in parts {
            let Some(parsed) = Part::parse(part) else {
                return Err(Error::invalid(format!(
                    "a mail template part must be subject, html or text, got {part:?}"
                )));
            };
            env.add_template_owned(format!("{name}.{}", parsed.suffix()), (*body).to_string())
                .map_err(|err| Error::invalid(format!("mail template {name}.{part}: {err}")))?;
        }
        Ok(())
    }

    /// Render all three parts with `context`.
    pub fn render_all(&self, name: &str, context: &Value) -> Result<Rendered> {
        Ok(Rendered {
            subject: self.render(name, Part::Subject, context)?,
            html: self.render(name, Part::Html, context)?,
            text: self.render(name, Part::Text, context)?,
        })
    }

    /// The template names that can be sent, built-in and custom.
    pub fn names(&self) -> Vec<&str> {
        self.names.iter().map(String::as_str).collect()
    }

    /// Render one part of a template with `context`.
    fn render(&self, name: &str, part: Part, context: &Value) -> Result<String> {
        let key = format!("{name}.{}", part.suffix());
        let template = self.env.get_template(&key).map_err(|_| {
            Error::invalid(format!(
                "no mail template named {name:?} (known: {})",
                self.names().join(", ")
            ))
        })?;
        template
            .render(minijinja::Value::from_serialize(context))
            .map_err(|err| Error::internal(format!("cannot render mail template {key:?}: {err}")))
    }
}

/// Template names the directory defines that are not built in.
///
/// Derived from the directory listing (`<name>.<part>`), so adding a template is
/// dropping files in, and a name with a typo'd part is simply not a name — its
/// incompleteness is reported by the caller.
fn custom_names(directory: Option<&Path>) -> Result<BTreeSet<String>> {
    let mut names = BTreeSet::new();
    let Some(directory) = directory else {
        return Ok(names);
    };
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(names),
        Err(err) => {
            return Err(Error::config(format!(
                "cannot read the mail template directory {}: {err}",
                directory.display()
            )))
        }
    };

    for entry in entries {
        let entry = entry.map_err(|err| {
            Error::config(format!(
                "cannot read the mail template directory {}: {err}",
                directory.display()
            ))
        })?;
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else {
            continue;
        };
        let Some((name, part)) = file_name.rsplit_once('.') else {
            continue;
        };
        if !matches!(part, "subject" | "html" | "text") {
            continue;
        }
        if BUILTIN_NAMES.contains(&name) {
            continue;
        }
        names.insert(name.to_string());
    }
    Ok(names)
}

/// A readable plain-text alternative for a message sent as HTML only.
///
/// Deliberately crude: strip tags, collapse blank lines, decode the entities a
/// sentence actually contains. It is a fallback for a caller who did not supply a
/// text part, not an HTML-to-text converter.
fn plain_text_from_html(html: &str) -> String {
    let mut stripped = String::with_capacity(html.len());
    let mut in_tag = false;
    for character in html.chars() {
        match character {
            '<' => {
                in_tag = true;
                if !stripped.ends_with(' ') {
                    stripped.push(' ');
                }
            }
            '>' => in_tag = false,
            other if !in_tag => stripped.push(other),
            _ => {}
        }
    }

    let decoded = stripped
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&");

    decoded
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Apply saved bodies over resolved ones.
///
/// A name the API adds is a new template; a part it names replaces the body beneath
/// it, which is what makes a reset a delete rather than a copy of the default.
fn apply(
    entries: &mut BTreeMap<(String, &'static str), Entry>,
    overrides: &[Override],
) -> Result<()> {
    for template in overrides {
        let Some(part) = Part::parse(&template.part) else {
            return Err(Error::invalid(format!(
                "a mail template part must be subject, html or text, got {:?}",
                template.part
            )));
        };
        if template.name.trim().is_empty() {
            return Err(Error::invalid("a mail template needs a name"));
        }
        entries.insert(
            (template.name.clone(), part.suffix()),
            Entry {
                body: template.body.clone(),
                origin: Origin::Database,
            },
        );
    }
    Ok(())
}

/// The context a send fills in: the caller's assigns, with `name` and `app_name`
/// added when they are absent.
///
/// Shared by a send and by a preview, so what an editor is shown cannot differ from
/// what goes out.
pub fn context_for(to_name: &str, app_name: &str, assigns: &Value) -> Result<Value> {
    let mut context: Map<String, Value> = match assigns {
        Value::Object(map) => map.clone(),
        Value::Null => Map::new(),
        _ => return Err(Error::invalid("mail assigns must be an object")),
    };
    context
        .entry("name".to_string())
        .or_insert_with(|| json!(to_name));
    context
        .entry("app_name".to_string())
        .or_insert_with(|| json!(app_name));
    Ok(Value::Object(context))
}

fn read_override(directory: Option<&Path>, key: &str) -> Result<Option<String>> {
    let Some(directory) = directory else {
        return Ok(None);
    };
    let path = directory.join(key);
    match std::fs::read_to_string(&path) {
        Ok(source) => Ok(Some(source)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(Error::config(format!(
            "cannot read mail template {}: {err}",
            path.display()
        ))),
    }
}

/// A rendered message, ready to hand to a transport.
#[derive(Debug, Clone)]
pub struct Message {
    /// The recipient's display name.
    pub to_name: String,
    /// The recipient's address.
    pub to: String,
    /// The rendered subject.
    pub subject: String,
    /// The rendered HTML body.
    pub html: String,
    /// The rendered plain-text alternative.
    pub text: String,
}

enum Transport {
    Smtp(Box<AsyncSmtpTransport<Tokio1Executor>>),
    Memory(Arc<Mutex<Vec<Message>>>),
}

/// A configured mailer.
pub struct Mailer {
    templates: Arc<Templates>,
    config: MailConfig,
    transport: Transport,
}

impl Mailer {
    /// Build a mailer that sends over SMTP.
    pub fn new(templates: Arc<Templates>, config: MailConfig) -> Result<Self> {
        let transport = build_smtp(&config)?;
        Ok(Self {
            templates,
            config,
            transport: Transport::Smtp(Box::new(transport)),
        })
    }

    /// A mailer that records messages instead of sending them.
    pub fn capture(templates: Arc<Templates>, config: MailConfig) -> Self {
        Self {
            templates,
            config,
            transport: Transport::Memory(Arc::new(Mutex::new(Vec::new()))),
        }
    }

    /// Render and send one message from a template.
    ///
    /// `assigns` is the template context; `name` and `app_name` are filled in when
    /// it does not carry them.
    pub async fn deliver(
        &self,
        template: &str,
        to_name: &str,
        to: &str,
        assigns: &Value,
    ) -> Result<()> {
        let context = context_for(to_name, &self.config.app_name, assigns)?;
        // The same path a preview renders through, so what an editor shows and what is
        // sent cannot drift apart.
        let rendered = self.templates.render_all(template, &context)?;

        let message = Message {
            to_name: to_name.to_string(),
            to: to.to_string(),
            subject: rendered.subject,
            html: rendered.html,
            text: rendered.text,
        };

        match &self.transport {
            Transport::Memory(buffer) => {
                buffer
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .push(message);
                Ok(())
            }
            Transport::Smtp(transport) => send_smtp(transport, &self.config, &message).await,
        }
    }

    /// Send one fully rendered message.
    ///
    /// This is the application-mail path (`POST /api/mail` with a subject and a
    /// body): no template, no context, the caller supplies the parts. A message with
    /// only HTML gets a plain-text alternative derived from it, because some clients
    /// show nothing for an HTML-only message.
    pub async fn deliver_raw(
        &self,
        to_name: &str,
        to: &str,
        subject: &str,
        html: &str,
        text: &str,
    ) -> Result<()> {
        if subject.trim().is_empty() {
            return Err(Error::invalid("a mail subject must not be empty"));
        }
        if html.trim().is_empty() && text.trim().is_empty() {
            return Err(Error::invalid(
                "a mail body must not be empty (html or text)",
            ));
        }

        let message = Message {
            to_name: to_name.to_string(),
            to: to.to_string(),
            subject: subject.to_string(),
            html: html.to_string(),
            text: if text.trim().is_empty() {
                plain_text_from_html(html)
            } else {
                text.to_string()
            },
        };

        match &self.transport {
            Transport::Memory(buffer) => {
                buffer
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .push(message);
                Ok(())
            }
            Transport::Smtp(transport) => send_smtp(transport, &self.config, &message).await,
        }
    }

    /// The messages a [`Mailer::capture`] mailer has recorded, oldest first.
    pub fn captured(&self) -> Vec<Message> {
        match &self.transport {
            Transport::Memory(buffer) => buffer
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone(),
            Transport::Smtp(_) => Vec::new(),
        }
    }
}

fn build_smtp(config: &MailConfig) -> Result<AsyncSmtpTransport<Tokio1Executor>> {
    let builder = match config.encryption {
        Encryption::None => AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&config.host),
        Encryption::Starttls => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&config.host)
            .map_err(|err| {
                Error::config(format!(
                    "cannot configure TLS for the mail host {}: {err}",
                    config.host
                ))
            })?,
        Encryption::Tls => {
            AsyncSmtpTransport::<Tokio1Executor>::relay(&config.host).map_err(|err| {
                Error::config(format!(
                    "cannot configure TLS for the mail host {}: {err}",
                    config.host
                ))
            })?
        }
    };

    let builder = builder.port(config.port);
    let builder = match (&config.username, &config.password) {
        (Some(username), Some(password)) => {
            builder.credentials(Credentials::new(username.clone(), password.clone()))
        }
        _ => builder,
    };

    Ok(builder.build())
}

async fn send_smtp(
    transport: &AsyncSmtpTransport<Tokio1Executor>,
    config: &MailConfig,
    message: &Message,
) -> Result<()> {
    let from = Mailbox::new(
        Some(config.sender_name.clone()),
        parse_address(&config.sender_address)?,
    );
    let to = Mailbox::new(Some(message.to_name.clone()), parse_address(&message.to)?);

    let email = lettre::Message::builder()
        .from(from)
        .to(to)
        .subject(message.subject.clone())
        .multipart(MultiPart::alternative_plain_html(
            message.text.clone(),
            message.html.clone(),
        ))
        .map_err(|err| Error::internal(format!("cannot build the email: {err}")))?;

    transport
        .send(email)
        .await
        .map_err(|err| Error::internal(format!("could not send mail: {err}")))?;
    Ok(())
}

fn parse_address(raw: &str) -> Result<lettre::Address> {
    raw.parse()
        .map_err(|_| Error::config(format!("{raw:?} is not a valid email address")))
}

/// The built-in subject, HTML body and text body for a template.
fn builtin(name: &str, part: Part) -> Option<&'static str> {
    Some(match (name, part) {
        ("verification", Part::Subject) => "Confirm your email for {{ app_name }}",
        ("verification", Part::Html) => {
            "<p>Hi {{ name }},</p>\
             <p>Confirm this email address for {{ app_name }}.</p>\
             <p>Use this code: <code>{{ token }}</code></p>"
        }
        ("verification", Part::Text) => {
            "Hi {{ name }},\n\nConfirm this email address for {{ app_name }}.\n\nUse this code: {{ token }}\n"
        }
        ("password_reset", Part::Subject) => "Reset your {{ app_name }} password",
        ("password_reset", Part::Html) => {
            "<p>Hi {{ name }},</p>\
             <p>Somebody asked to reset the password for this {{ app_name }} account.</p>\
             <p>Use this code: <code>{{ token }}</code></p>\
             <p>If this was not you, nothing has changed.</p>"
        }
        ("password_reset", Part::Text) => {
            "Hi {{ name }},\n\nSomebody asked to reset the password for this {{ app_name }} account.\n\nUse this code: {{ token }}\n\nIf this was not you, nothing has changed.\n"
        }
        ("email_change", Part::Subject) => "Confirm your new {{ app_name }} email",
        ("email_change", Part::Html) => {
            "<p>Hi {{ name }},</p>\
             <p>Confirm {{ email }} as the new address for your {{ app_name }} account.</p>\
             <p>Use this code: <code>{{ token }}</code></p>"
        }
        ("email_change", Part::Text) => {
            "Hi {{ name }},\n\nConfirm {{ email }} as the new address for your {{ app_name }} account.\n\nUse this code: {{ token }}\n"
        }
        ("otp", Part::Subject) => "Your {{ app_name }} sign-in code",
        ("otp", Part::Html) => {
            "<p>Hi {{ name }},</p>\
             <p>Your {{ app_name }} sign-in code is:</p>\
             <p style=\"font-size: 24px; letter-spacing: 4px\"><strong>{{ code }}</strong></p>\
             <p>It expires in {{ expires_minutes }} minutes.</p>"
        }
        ("otp", Part::Text) => {
            "Hi {{ name }},\n\nYour {{ app_name }} sign-in code is: {{ code }}\n\nIt expires in {{ expires_minutes }} minutes.\n"
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> MailConfig {
        MailConfig {
            host: "smtp.example".into(),
            port: 587,
            username: None,
            password: None,
            encryption: Encryption::Starttls,
            sender_name: "My App".into(),
            sender_address: "noreply@example.com".into(),
            app_name: "My App".into(),
        }
    }

    fn templates() -> Arc<Templates> {
        Arc::new(Templates::load(None).expect("built-in templates"))
    }

    #[test]
    fn a_mail_setting_is_required_before_mail_counts_as_configured() {
        assert!(MailConfig::from_settings(&json!({})).unwrap().is_none());
        assert!(MailConfig::from_settings(&json!({ "app_name": "x" }))
            .unwrap()
            .is_none());

        let config = MailConfig::from_settings(&json!({
            "app_name": "My App",
            "mail": { "host": "smtp.example" }
        }))
        .unwrap()
        .expect("configured");
        assert_eq!(config.port, 587, "starttls defaults to 587");
        assert_eq!(config.sender_address, "noreply@lazypock.app");
        assert_eq!(config.app_name, "My App");
    }

    #[test]
    fn the_port_follows_the_encryption_and_bad_values_are_refused() {
        let tls = MailConfig::from_settings(&json!({
            "mail": { "host": "smtp.example", "encryption": "tls" }
        }))
        .unwrap()
        .expect("configured");
        assert_eq!(tls.port, 465);

        let plain = MailConfig::from_settings(&json!({
            "mail": { "host": "smtp.example", "encryption": "none" }
        }))
        .unwrap()
        .expect("configured");
        assert_eq!(plain.port, 25);

        for bad in [
            json!({ "mail": { "encryption": "starttls" } }),
            json!({ "mail": { "host": "smtp.example", "port": 0 } }),
            json!({ "mail": { "host": "smtp.example", "encryption": "ssl3" } }),
            json!({ "mail": { "host": "smtp.example", "username": "u" } }),
            json!({ "mail": { "host": "smtp.example", "sender_address": "nope" } }),
        ] {
            assert!(
                MailConfig::from_settings(&bad).is_err(),
                "should have been refused: {bad}"
            );
        }
    }

    #[test]
    fn a_builtin_template_renders_all_three_parts() {
        let templates = templates();
        let context =
            json!({ "name": "Ada", "app_name": "My App", "code": "123456", "expires_minutes": 10 });
        assert_eq!(
            templates.render("otp", Part::Subject, &context).unwrap(),
            "Your My App sign-in code"
        );
        let html = templates.render("otp", Part::Html, &context).unwrap();
        assert!(html.contains("123456"), "{html}");
        let text = templates.render("otp", Part::Text, &context).unwrap();
        assert!(text.contains("123456"), "{text}");
    }

    #[test]
    fn a_template_using_an_unsupplied_variable_is_an_error() {
        let templates = templates();
        let err = templates
            .render(
                "otp",
                Part::Html,
                &json!({ "name": "Ada", "app_name": "x" }),
            )
            .expect_err("a missing variable must fail");
        assert!(err.to_string().contains("render"), "{err}");
    }

    #[test]
    fn a_file_overrides_the_built_in_template() {
        let directory = std::env::temp_dir().join(format!("lazypock-mail-{}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("temp dir");
        std::fs::write(directory.join("otp.subject"), "Code for {{ app_name }}").expect("write");

        let templates = Templates::load(Some(&directory)).expect("templates");
        let rendered = templates
            .render(
                "otp",
                Part::Subject,
                &json!({ "app_name": "My App", "code": "1" }),
            )
            .unwrap();
        assert_eq!(rendered, "Code for My App");
        // A part with no override still comes from the built-in.
        assert!(templates
            .render(
                "otp",
                Part::Html,
                &json!({ "name": "A", "app_name": "x", "code": "1", "expires_minutes": 5 })
            )
            .unwrap()
            .contains("1"));

        std::fs::remove_dir_all(&directory).ok();
    }

    /// A template directory unique to one test, removed by the test.
    fn template_directory(label: &str) -> std::path::PathBuf {
        let directory =
            std::env::temp_dir().join(format!("lazypock-mail-{label}-{}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("temp dir");
        directory
    }

    #[test]
    fn a_template_directory_can_add_a_name() {
        let directory = template_directory("custom");
        std::fs::write(directory.join("receipt.subject"), "Your receipt").expect("write");
        std::fs::write(directory.join("receipt.html"), "<p>Thanks, {{ name }}</p>").expect("write");
        std::fs::write(directory.join("receipt.text"), "Thanks, {{ name }}").expect("write");

        let templates = Templates::load(Some(&directory)).expect("templates");
        assert!(
            templates.names().contains(&"receipt"),
            "custom names: {:?}",
            templates.names()
        );
        // The built-ins are still there.
        assert!(templates.names().contains(&"otp"));
        assert_eq!(
            templates
                .render("receipt", Part::Subject, &json!({}))
                .unwrap(),
            "Your receipt"
        );

        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn a_half_defined_custom_template_is_refused_at_load() {
        let directory = template_directory("incomplete");
        std::fs::write(directory.join("receipt.subject"), "Your receipt").expect("write");
        // No `receipt.html` / `receipt.text`: an email that arrives empty is worse than
        // one that refuses to load.
        let err = match Templates::load(Some(&directory)) {
            Ok(_) => panic!("an incomplete custom template must be refused"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("incomplete"), "{err}");

        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn an_unknown_template_names_what_is_available() {
        let err = templates()
            .render("nope", Part::Html, &json!({}))
            .expect_err("unknown template");
        assert!(err.to_string().contains("otp"), "{err}");
    }

    /// An override as the API builds one from a `_mail_templates` row.
    fn override_of(name: &str, part: &str, body: &str) -> Override {
        Override {
            name: name.to_string(),
            part: part.to_string(),
            body: body.to_string(),
        }
    }

    #[test]
    fn a_saved_body_wins_over_the_file_and_the_built_in() {
        let directory = template_directory("layers");
        std::fs::write(directory.join("otp.subject"), "From the file").expect("write");

        let templates = Templates::load_with(
            Some(&directory),
            &[
                override_of("otp", "subject", "From the database"),
                override_of("otp", "html", "<p>Saved {{ code }}</p>"),
            ],
        )
        .expect("templates");

        let parts = templates.template("otp").expect("a template");
        let by_part = |part: &str| {
            parts
                .iter()
                .find(|resolved| resolved.part == part)
                .expect("a part")
                .clone()
        };
        assert_eq!(by_part("subject").body, "From the database");
        assert_eq!(by_part("subject").origin, Origin::Database);
        assert_eq!(by_part("html").body, "<p>Saved {{ code }}</p>");
        assert_eq!(by_part("html").origin, Origin::Database);
        // A part nobody overrode keeps the layer beneath it, and the origin is reported
        // per part rather than per template: a deployment that changed only the subject
        // did not change the body.
        assert_eq!(by_part("text").origin, Origin::Builtin);
        assert!(by_part("text").body.contains("{{ code }}"));

        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn a_saved_body_can_add_a_name_and_only_needs_the_parts_it_sets() {
        // A built-in name may be overridden one part at a time — the layers beneath
        // fill the rest — but a new name must arrive complete, because there is
        // nothing beneath it to fall back to.
        assert!(
            Templates::load_with(None, &[override_of("receipt", "subject", "Receipt")]).is_err()
        );

        let templates = Templates::load_with(
            None,
            &[
                override_of("receipt", "subject", "Your receipt"),
                override_of("receipt", "html", "<p>Thanks {{ name }}</p>"),
                override_of("receipt", "text", "Thanks {{ name }}"),
            ],
        )
        .expect("templates");
        assert!(templates.names().contains(&"receipt"));
        let rendered = templates
            .render_all("receipt", &json!({ "name": "Ada" }))
            .expect("render");
        assert_eq!(rendered.subject, "Your receipt");
        assert_eq!(rendered.text, "Thanks Ada");
    }

    #[test]
    fn a_syntax_error_is_refused_before_the_body_is_stored() {
        assert!(Templates::check("otp", &[("html", "{% if x %}unclosed")]).is_err());
        Templates::check("otp", &[("html", "<p>{{ code }}</p>")]).expect("parses");
        assert!(Templates::check("otp", &[("body", "<p>x</p>")]).is_err());
    }

    #[test]
    fn an_override_leaves_the_parts_it_does_not_name_alone() {
        let base = Templates::load(None).expect("built-ins");
        let overlaid = base
            .with_overrides(&[override_of("otp", "subject", "Just the subject")])
            .expect("overlaid");

        let before = base.template("otp").expect("a template");
        let after = overlaid.template("otp").expect("a template");
        for (before, after) in before.iter().zip(&after) {
            if before.part == "subject" {
                assert_eq!(after.body, "Just the subject");
                assert_eq!(after.origin, Origin::Database);
            } else {
                assert_eq!(after, before, "{} must be untouched", before.part);
            }
        }
    }

    #[tokio::test]
    async fn a_raw_message_is_recorded_with_the_parts_it_was_given() {
        let mailer = Mailer::capture(templates(), config());
        mailer
            .deliver_raw(
                "Ada",
                "ada@example.com",
                "Your order shipped",
                "<p>It is on the way</p>",
                "It is on the way",
            )
            .await
            .expect("deliver");

        let sent = mailer.captured();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].subject, "Your order shipped");
        assert_eq!(sent[0].html, "<p>It is on the way</p>");
        assert_eq!(sent[0].text, "It is on the way");
    }

    #[tokio::test]
    async fn a_raw_message_without_a_text_part_gets_a_readable_one() {
        let mailer = Mailer::capture(templates(), config());
        mailer
            .deliver_raw(
                "Ada",
                "ada@example.com",
                "Hello",
                "<p>Hello&nbsp;Ada</p><p>Second line &amp; more</p>",
                "",
            )
            .await
            .expect("deliver");

        let text = mailer.captured()[0].text.clone();
        assert!(
            !text.contains('<'),
            "markup leaked into the text part: {text}"
        );
        assert!(text.contains("Hello Ada"), "{text}");
        assert!(text.contains("Second line & more"), "{text}");
    }

    #[tokio::test]
    async fn an_empty_raw_message_is_refused() {
        let mailer = Mailer::capture(templates(), config());
        for (subject, html, text) in [("", "<p>x</p>", ""), ("Subject", "", "")] {
            assert!(
                mailer
                    .deliver_raw("Ada", "ada@example.com", subject, html, text)
                    .await
                    .is_err(),
                "should have been refused"
            );
        }
    }

    #[tokio::test]
    async fn a_capture_mailer_records_the_rendered_message() {
        let mailer = Mailer::capture(templates(), config());
        mailer
            .deliver(
                "verification",
                "Ada",
                "ada@example.com",
                &json!({ "token": "abc" }),
            )
            .await
            .expect("deliver");

        let sent = mailer.captured();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].to, "ada@example.com");
        // `name` and `app_name` are filled in from the mailer.
        assert_eq!(sent[0].subject, "Confirm your email for My App");
        // The token is what the email carries; the app builds the link around it.
        assert!(sent[0].html.contains("abc"), "html={}", sent[0].html);
        assert!(sent[0].text.contains("abc"), "text={}", sent[0].text);
    }
}
