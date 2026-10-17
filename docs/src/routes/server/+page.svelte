<script lang="ts">
  import CodeBlock from "$lib/components/CodeBlock.svelte";
  import Seo from '$lib/components/Seo.svelte';

  const title = 'Server Guide — Lazypock Docs';
  const description =
    'Run and operate a Lazypock server: Docker image or from source, first-time setup, filters, connecting an existing database, production checklist and every environment variable.';
</script>

<Seo {title} {description} />

<div class="prose-doc w-full">
  <!-- INTRO -->
  <section class="scroll-mt-20 mb-14">
    <div class="flex flex-wrap items-center gap-2">
      <span
        class="rounded-field bg-info/10 text-info px-2 py-0.5 text-xs font-medium"
        >Server · Rust + axum + PostgreSQL</span
      >
    </div>
    <h1 class="mt-3 text-3xl font-bold tracking-tight">Server Guide</h1>
    <p class="mt-3 text-base-content/80 leading-relaxed">
      How to run and operate a Lazypock server. The repo is a monorepo with two
      parts: the
      <strong>core</strong> backend (Rust, axum, PostgreSQL) and the
      <strong>Studio</strong>
      admin UI (SvelteKit, built into the binary and served at
      <code class="doc-inline px-1 py-0.5">/_/</code>). Your app talks to the
      server through one of the
      <a class="text-primary underline" href="/sdk">SDKs</a> — no backend code required.
    </p>
    <p class="mt-3 text-base-content/80 leading-relaxed">
      Everything is one binary: the API, auth, realtime, the TypeScript hooks
      runtime, migrations and the CLI. Postgres is the source of truth and the
      message bus (data, row-level security, jobs, realtime via
      <code class="doc-inline px-1 py-0.5">LISTEN/NOTIFY</code>).
    </p>

    <h2 class="mt-8 text-xl font-semibold border-b border-base-300 pb-2">
      Prerequisites
    </h2>
    <ul class="mt-3 space-y-2 text-base-content/80 list-disc list-inside">
      <li><strong>Rust 1.85+</strong> (only for running from source)</li>
      <li><strong>PostgreSQL 15+</strong> — a direct connection, not a pooler</li>
      <li>
        <strong>Docker</strong> (only for the container route) — no other system
        package is required
      </li>
      <li>
        <strong>Bun</strong> (only for the Studio dev server)
      </li>
    </ul>
    <p class="mt-3 text-sm text-base-content/70">
      There is no ImageMagick, no external queue and no other sidecar: thumbnails
      and scaled images are produced in-process, or by a service you point the
      server at — see
      <a class="text-primary underline" href="#images">Image processing</a>.
    </p>
  </section>

  <!-- QUICK START (DOCKER) -->
  <section id="quick-start" class="scroll-mt-20 mb-10">
    <h2 class="text-xl font-semibold border-b border-base-300 pb-2">
      Quick start — Docker
    </h2>
    <p class="mt-3 text-base-content/80 leading-relaxed">
      No Rust toolchain needed. The repository root has a
      <code class="doc-inline px-1 py-0.5">Dockerfile</code> and a
      <code class="doc-inline px-1 py-0.5">compose.yml</code> that runs the server
      next to Postgres, so the shortest path is:
    </p>
    <CodeBlock
      lang="bash"
      code={`git clone https://github.com/gnuzd/lazypock.git
cd lazypock

# set LAZYPOCK_DATABASE_URL and LAZYPOCK_SECRET_KEY in compose.yml first
docker compose up --build

# Studio: http://localhost:4000/_/`}
    />
    <p class="mt-3 text-base-content/80 leading-relaxed">
      A tagged release also publishes a multi-arch image
      (<code class="doc-inline px-1 py-0.5">ghcr.io/gnuzd/lazypock</code>), so a
      deployment can pull a built artifact instead of building one:
    </p>
    <CodeBlock
      lang="yaml"
      code={`services:
  lazypock:
    image: ghcr.io/gnuzd/lazypock:latest
    ports: ["4000:4000"]
    environment:
      LAZYPOCK_DATABASE_URL: postgres://lazypock:change-me@db:5432/lazypock
      # openssl rand -hex 32
      LAZYPOCK_SECRET_KEY: change-me
      LAZYPOCK_ENV: production
    volumes: ["lazypock-data:/data"]
  db:
    image: postgres:16-alpine
    environment:
      POSTGRES_USER: lazypock
      POSTGRES_PASSWORD: change-me
      POSTGRES_DB: lazypock
    volumes: ["db-data:/var/lib/postgresql/data"]
volumes:
  lazypock-data:
  db-data:`}
    />
    <div
      class="rounded-box border border-base-300 bg-base-200/60 p-4 my-4 text-sm leading-relaxed"
    >
      <ul class="space-y-1.5 list-disc list-inside">
        <li>
          Server + Studio admin UI: <code class="doc-inline px-1 py-0.5"
            >http://localhost:4000</code
          >
          (login at <code class="doc-inline px-1 py-0.5">/_/</code>)
        </li>
        <li>
          REST API: <code class="doc-inline px-1 py-0.5"
            >http://localhost:4000/api/...</code
          >
        </li>
        <li>
          The image runs as a non-root user and writes only to
          <code class="doc-inline px-1 py-0.5">/data</code>, which the compose
          file mounts as a volume. Migrations run automatically on boot.
        </li>
        <li>
          Pin a version rather than <code class="doc-inline px-1 py-0.5">latest</code>
          in production.
        </li>
      </ul>
    </div>
    <p class="mt-3 text-base-content/80 leading-relaxed">
      To reset everything (including the database):
    </p>
    <CodeBlock lang="bash" code={`docker compose down -v`} />
    <p class="mt-3 text-base-content/80 leading-relaxed">
      Prefer to run the binary directly? See
      <a class="text-primary underline" href="#source">Run from source</a> below.
    </p>
    <p class="mt-3 text-base-content/80 leading-relaxed">
      Ready to connect an app? Follow the
      <a class="text-primary underline" href="/sdk/typescript"
        >TypeScript SDK quick start</a
      >.
    </p>
  </section>

  <!-- RUN FROM SOURCE -->
  <section id="source" class="scroll-mt-20 mb-10">
    <h2 class="text-xl font-semibold border-b border-base-300 pb-2">
      Run from source
    </h2>
    <p class="mt-2 text-base-content/80 leading-relaxed">
      Requires Rust 1.85+ and a reachable PostgreSQL 15+ instance (Docker is
      fine). The CLI is the same binary you deploy.
    </p>
    <CodeBlock
      lang="bash"
      code={`git clone https://github.com/gnuzd/lazypock.git
cd lazypock

cp .env.example .env       # then point LAZYPOCK_DATABASE_URL at your Postgres
cargo run -- migrate        # create the system tables
cargo run -- config         # show the resolved config (credentials redacted)
cargo run -- serve          # start the API on http://localhost:4000
curl http://localhost:4000/api/health`}
    />
    <p class="mt-2 text-sm text-base-content/70">
      <code class="doc-inline px-1 py-0.5">serve</code> also applies pending
      migrations on boot and loads the schema before accepting requests, so a
      first run against an empty database is the whole installation step. Run
      <code class="doc-inline px-1 py-0.5">lazypock migrate --status</code> to see
      what is applied.
    </p>
    <h3 class="mt-6 text-lg font-semibold">Studio (admin UI)</h3>
    <p class="mt-2 text-base-content/80 leading-relaxed">
      A plain <code class="doc-inline px-1 py-0.5">cargo build</code> embeds no
      Studio. To run the SvelteKit dev server against a local API:
    </p>
    <CodeBlock
      lang="bash"
      code={`cd studio
bun install
bun run dev          # Vite dev server on http://localhost:5173`}
    />
    <p class="mt-2 text-base-content/80 leading-relaxed">
      The SvelteKit dev server proxies <code class="doc-inline px-1 py-0.5"
        >/api</code
      >
      requests to the Rust backend on port 4000, and serves the Studio at
      <code class="doc-inline px-1 py-0.5">http://localhost:5173/_/</code>. To
      build the bundle into the binary:
    </p>
    <CodeBlock
      lang="bash"
      code={`cd studio && bun run build
cd ..
LAZYPOCK_STUDIO_DIR=studio/build/studio cargo run -- serve`}
    />
    <p class="mt-4 text-base-content/80 leading-relaxed">
      Then connect an app with one of the
      <a class="text-primary underline" href="/sdk">SDKs</a> — see the
      <a class="text-primary underline" href="/sdk/typescript#install"
        >TypeScript SDK install</a
      >.
    </p>
  </section>

  <!-- FIRST-TIME SETUP -->
  <section id="first-time" class="scroll-mt-20 mb-10">
    <h2 class="text-xl font-semibold border-b border-base-300 pb-2">
      First-time setup
    </h2>
    <p class="mt-2 text-base-content/80 leading-relaxed">
      There is <strong>no default superuser and no default password</strong>. Any
      one of these three creates the first account:
    </p>
    <ol class="mt-3 list-decimal list-inside space-y-1 text-base-content/80">
      <li>
        <strong>From the Studio.</strong> Open
        <code class="doc-inline px-1 py-0.5">/_/</code>; with no superuser yet you
        are offered <strong>Setup</strong> instead of login. Setup only works
        while no superuser exists, so it cannot be replayed later.
      </li>
      <li>
        <strong>From the CLI.</strong>
        <code class="doc-inline px-1 py-0.5"
          >lazypock superuser create --email you@example.com --password …</code
        >
        (or set
        <code class="doc-inline px-1 py-0.5">LAZYPOCK_SUPERUSER_EMAIL</code> /
        <code class="doc-inline px-1 py-0.5">LAZYPOCK_SUPERUSER_PASSWORD</code> to
        keep both out of shell history — there is no interactive prompt, and a
        missing value is an error). This applies migrations first, so it is safe
        on a brand-new database.
      </li>
      <li>
        <strong>From the environment at boot,</strong> for a deployment with no
        browser and nothing to <code class="doc-inline px-1 py-0.5">exec</code>
        into yet: set both
        <code class="doc-inline px-1 py-0.5">LAZYPOCK_SUPERUSER_EMAIL</code> and
        <code class="doc-inline px-1 py-0.5">LAZYPOCK_SUPERUSER_PASSWORD</code>
        and start the server. The account is created only while none exists, so a
        redeploy never resets a password or makes a second one; setting only one
        of the two is a boot error rather than a guess. The same advisory lock as
        the Setup page is taken, so the two cannot race on a fresh installation.
      </li>
    </ol>
    <p class="mt-3 text-base-content/80 leading-relaxed">
      Then log in and start creating collections — every collection you create in
      Studio gets an instant REST API + realtime channel + rules, ready to
      call from any SDK.
    </p>
  </section>

  <!-- OAUTH2 -->
  <section id="oauth2" class="scroll-mt-20 mb-10">
    <h2 class="text-xl font-semibold border-b border-base-300 pb-2">
      OAuth2 sign-in
    </h2>
    <p class="mt-2 text-base-content/80 leading-relaxed">
      Auth collections can sign in through an external provider. Providers are
      configured once, as an application setting, so no rebuild is needed:
    </p>
    <CodeBlock
      lang="bash"
      code={`curl -X PATCH https://your-host/api/settings \
  -H "Authorization: Bearer <superuser token>" \
  -H "Content-Type: application/json" \
  -d '{"oauth2":{"providers":[{
        "name": "google",
        "config": {
          "client_id": "...",
          "client_secret": "...",
          "redirect_uri": "https://your-host/api/oauth2-redirect"
        }
      }]}}'`}
    />
    <p class="mt-3 text-base-content/80 leading-relaxed">
      <code class="doc-inline px-1 py-0.5">google</code> and
      <code class="doc-inline px-1 py-0.5">github</code> have built-in presets (the
      three provider URLs and the scopes). Any other provider names
      <code class="doc-inline px-1 py-0.5">authorize_url</code>,
      <code class="doc-inline px-1 py-0.5">token_url</code> and
      <code class="doc-inline px-1 py-0.5">user_url</code> explicitly. A provider
      missing one of them is refused at use, not silently dropped.
    </p>
    <CodeBlock
      lang="http"
      code={`# what the client asks for
GET /api/collections/users/auth-methods
# -> oauth2.providers: [{ name, authURL, state, codeVerifier }, ...]

# open authURL in a popup; the provider returns to
GET /api/oauth2-redirect?code=...&state=...

# exchange the code (the popup bridge posts it back to the app origin)
POST /api/collections/users/auth-with-oauth2
{ "provider": "google", "code": "...", "codeVerifier": "...",
  "redirectUrl": "https://your-host/api/oauth2-redirect", "createData": {} }`}
    />
    <p class="mt-3 text-sm text-base-content/70">
      The first sign-in creates an account in the collection (the email comes from
      the provider and is marked verified) and links it in
      <code class="doc-inline px-1 py-0.5">_external_auths</code>; later sign-ins
      reuse it. <code class="doc-inline px-1 py-0.5">createData</code> may fill
      ordinary fields, but never the email, password or
      <code class="doc-inline px-1 py-0.5">verified</code>. The PKCE flow uses S256,
      the pending flow is single-use and expires after 10 minutes, and the popup
      result is posted only to the origin that started it.
    </p>
  </section>

  <!-- MAIL -->
  <section id="mail" class="scroll-mt-20 mb-10">
    <h2 class="text-xl font-semibold border-b border-base-300 pb-2">
      Email
    </h2>
    <p class="mt-2 text-base-content/80 leading-relaxed">
      Outbound email — verification, password reset, sign-in codes — goes through
      SMTP configured as the <code class="doc-inline px-1 py-0.5">mail</code>
      setting:
    </p>
    <CodeBlock
      lang="bash"
      code={`PATCH /api/settings
{
  "app_name": "My App",
  "mail": {
    "host": "smtp.example",
    "port": 587,
    "encryption": "starttls",
    "username": "...",
    "password": "...",
    "sender_name": "My App",
    "sender_address": "noreply@example.com"
  }
}`}
    />
    <p class="mt-3 text-base-content/80 leading-relaxed">
      <code class="doc-inline px-1 py-0.5">encryption</code> is
      <code class="doc-inline px-1 py-0.5">none</code>,
      <code class="doc-inline px-1 py-0.5">starttls</code> or
      <code class="doc-inline px-1 py-0.5">tls</code>, and the port defaults from
      it (25 / 587 / 465). The configuration is read <strong>at send time</strong>,
      so a change applies to the next email without a restart. Sends are queued and
      retried, never made inside a request.
    </p>
    <h3 class="mt-5 font-semibold">Templates</h3>
    <p class="mt-2 text-base-content/80 leading-relaxed">
      The subject, HTML body and plain-text alternative are rendered with
      <a
        class="text-primary underline"
        href="https://docs.rs/minijinja"
        target="_blank"
        rel="noreferrer">MiniJinja</a
      >. Built-in templates ship for
      <code class="doc-inline px-1 py-0.5">otp</code>,
      <code class="doc-inline px-1 py-0.5">verification</code>,
      <code class="doc-inline px-1 py-0.5">password_reset</code> and
      <code class="doc-inline px-1 py-0.5">email_change</code>. A file named for
      the part — <code class="doc-inline px-1 py-0.5">otp.subject</code>,
      <code class="doc-inline px-1 py-0.5">otp.html</code>,
      <code class="doc-inline px-1 py-0.5">otp.text</code> — in
      <code class="doc-inline px-1 py-0.5">LAZYPOCK_MAIL_TEMPLATES_DIR</code>
      replaces it, so changing an email does not need a rebuild. A template that
      uses a variable the server did not supply fails loudly rather than sending
      an empty code.
    </p>
  </section>

  <!-- EMAIL FLOWS -->
  <section id="email-flows" class="scroll-mt-20 mb-10">
    <h2 class="text-xl font-semibold border-b border-base-300 pb-2">
      Email verification, password reset and email change
    </h2>
    <p class="mt-2 text-base-content/80 leading-relaxed">
      Six endpoints on an auth collection, all backed by one-time tokens in
      <code class="doc-inline px-1 py-0.5">_otps</code>. The <em>request</em> half
      queues an email through the mailer; the <em>confirm</em> half applies the
      change:
    </p>
    <CodeBlock
      lang="http"
      code={`POST /api/collections/users/request-verification  { "email": "..." }
POST /api/collections/users/confirm-verification  { "token": "..." }

POST /api/collections/users/request-password-reset { "email": "..." }
POST /api/collections/users/confirm-password-reset { "token", "password", "passwordConfirm" }

# these two need a signed-in account
POST /api/collections/users/request-email-change { "newEmail": "..." }
POST /api/collections/users/confirm-email-change { "token", "password" }`}
    />
    <p class="mt-3 text-base-content/80 leading-relaxed">
      The request half always answers
      <code class="doc-inline px-1 py-0.5">204</code>, whether or not the address
      has an account, so it cannot be used to find out which addresses exist. A
      token is 32 random bytes, valid for 24 hours and
      <strong>single-use</strong>. Confirming a password reset also revokes the
      account's sessions, and an email change additionally requires the current
      password and is applied to the address the token was mailed to, not to any
      the caller supplies.
    </p>
  </section>

  <!-- FILTERS -->
  <section id="filters" class="scroll-mt-20 mb-10">
    <h2 class="text-xl font-semibold border-b border-base-300 pb-2">
      Filters
    </h2>
    <p class="mt-2 text-base-content/80 leading-relaxed">
      Every list endpoint accepts a PocketBase-compatible
      <code class="doc-inline px-1 py-0.5">?filter=</code> expression (combined with
      the collection's <code class="doc-inline px-1 py-0.5">listRule</code>). Clauses are
      <code class="doc-inline px-1 py-0.5">field op value</code>, joined with
      <code class="doc-inline px-1 py-0.5">&amp;&amp;</code> (AND) and
      <code class="doc-inline px-1 py-0.5">||</code> (OR); use
      <code class="doc-inline px-1 py-0.5">!</code> and parentheses to negate / group.
    </p>
    <CodeBlock
      lang="http"
      code={`GET /api/posts?filter=(title~'hello' && published=true) || author='USER_ID'`}
    />
    <div class="mt-3 overflow-x-auto rounded-box border border-base-300">
      <table class="w-full text-sm">
        <thead class="bg-base-200 text-left">
          <tr>
            <th class="px-3 py-2 font-semibold">Operator</th>
            <th class="px-3 py-2 font-semibold">Meaning</th>
          </tr>
        </thead>
        <tbody class="divide-y divide-base-300">
          <tr>
            <td class="px-3 py-2"><code class="doc-inline px-1 py-0.5">=</code> <code class="doc-inline px-1 py-0.5">!=</code></td>
            <td class="px-3 py-2">Equal / not equal</td>
          </tr>
          <tr>
            <td class="px-3 py-2"><code class="doc-inline px-1 py-0.5">&gt;</code> <code class="doc-inline px-1 py-0.5">&gt;=</code> <code class="doc-inline px-1 py-0.5">&lt;</code> <code class="doc-inline px-1 py-0.5">&lt;=</code></td>
            <td class="px-3 py-2">Greater / less than (or equal)</td>
          </tr>
          <tr>
            <td class="px-3 py-2"><code class="doc-inline px-1 py-0.5">~</code> <code class="doc-inline px-1 py-0.5">!~</code></td>
            <td class="px-3 py-2">Like / not like (case-insensitive, auto-wrapped in <code class="doc-inline px-1 py-0.5">%…%</code>)</td>
          </tr>
          <tr>
            <td class="px-3 py-2"><code class="doc-inline px-1 py-0.5">?=</code> <code class="doc-inline px-1 py-0.5">?!=</code></td>
            <td class="px-3 py-2">Any element equal / not equal</td>
          </tr>
          <tr>
            <td class="px-3 py-2"><code class="doc-inline px-1 py-0.5">?~</code> <code class="doc-inline px-1 py-0.5">?!~</code></td>
            <td class="px-3 py-2">Any element like / not like</td>
          </tr>
          <tr>
            <td class="px-3 py-2"><code class="doc-inline px-1 py-0.5">?&gt;</code> <code class="doc-inline px-1 py-0.5">?&gt;=</code> <code class="doc-inline px-1 py-0.5">?&lt;</code> <code class="doc-inline px-1 py-0.5">?&lt;=</code></td>
            <td class="px-3 py-2">Any element compares</td>
          </tr>
        </tbody>
      </table>
    </div>
    <p class="mt-3 text-base-content/80 leading-relaxed">
      <strong>Array fields</strong> (multi-select, multiple relation, multiple file)
      apply a <em>match-all</em> constraint by default. Prefix the operator with
      <code class="doc-inline px-1 py-0.5">?</code> for an
      <em>any / at-least-one-of</em> match:
    </p>
    <CodeBlock
      lang="http"
      code={`GET /api/posts?filter=tags ?= 'news'                 # has the 'news' tag
GET /api/posts?filter=tags ?= 'news' && published=true
GET /api/posts?filter=tags ?~ 'new'                  # any tag contains 'new'`}
    />
    <p class="mt-3 text-base-content/80 leading-relaxed">
      <strong>Relation fields</strong> can be filtered through a dot-path —
      <code class="doc-inline px-1 py-0.5">author.email = 'ada@example.com'</code>
      — including multi-level paths and multi-relations.
      <strong>Null checks</strong> use
      <code class="doc-inline px-1 py-0.5">field = null</code> /
      <code class="doc-inline px-1 py-0.5">field != null</code>.
    </p>
    <CodeBlock
      lang="http"
      code={`GET /api/posts?filter=author.email = 'ada@example.com'
GET /api/posts?filter=author.manager.name ~ 'Ada'
GET /api/posts?filter=deleted_at = null`}
    />
    <p class="mt-3 text-sm text-base-content/70">
      Filters work the same in collection rules and through the typed SDK
      (<code class="doc-inline px-1 py-0.5">getList(1, 20, &#123; filter: "tags ?= 'news'" &#125;)</code>).
    </p>
  </section>

  <!-- EXISTING POSTGRES DATABASE -->
  <section id="existing-db" class="scroll-mt-20 mb-10">
    <h2 class="text-xl font-semibold border-b border-base-300 pb-2">
      Connect an existing PostgreSQL database
    </h2>
    <p class="mt-2 text-base-content/80 leading-relaxed">
      LazyPock is designed to run on top of a database you already have —
      point it at an existing Postgres (15+) instance and its tables show up
      in the Studio automatically. Just set
      <code class="doc-inline px-1 py-0.5">LAZYPOCK_DATABASE_URL</code> to your
      database:
    </p>
    <CodeBlock
      lang="bash"
      code={`export LAZYPOCK_DATABASE_URL="postgres://user:password@db-host:5432/my_existing_db"
cargo run -- serve`}
    />
    <p class="mt-3 text-base-content/80 leading-relaxed">
      On boot LazyPock does three things to the database:
    </p>
    <ol class="mt-3 list-decimal list-inside space-y-1 text-base-content/80">
      <li>
        <strong>System migrations</strong> create its internal
        <code class="doc-inline px-1 py-0.5">_</code>-prefixed tables
        (<code class="doc-inline px-1 py-0.5">_collections</code>,
        <code class="doc-inline px-1 py-0.5">_fields</code>,
        <code class="doc-inline px-1 py-0.5">_superusers</code>,
        <code class="doc-inline px-1 py-0.5">_settings</code>,
        <code class="doc-inline px-1 py-0.5">_logs</code>, …) plus its
        migration bookkeeping. These are namespaced and never touch your tables.
      </li>
      <li>
        <strong>Adoption (reflection)</strong>: every table it did not create
        becomes a collection, and every column becomes a field — so your existing
        tables appear in the Studio sidebar and are served through the dynamic
        <code class="doc-inline px-1 py-0.5">/api/collections/&#123;collection&#125;/records</code> routes
        immediately. This is reflection, not migration: <strong>nothing alters
        the existing tables</strong>, and a column lazypock has no field type for
        becomes text with the original type kept in its options.
      </li>
      <li>
        <strong>Default-deny.</strong> An adopted collection starts with no rules,
        which means <strong>superuser-only</strong>. Adopting a table must not
        silently expose somebody else&rsquo;s data; open it up deliberately in
        the Studio. Tables and schemas that are internal, or already a collection,
        are left alone.
      </li>
    </ol>
    <p class="mt-3 text-sm text-base-content/70">
      Adoption is controlled by
      <code class="doc-inline px-1 py-0.5">LAZYPOCK_REFLECT_EXISTING</code> (on by
      default). Row-level security needs the restricted
      <code class="doc-inline px-1 py-0.5">lazypock_anon</code> /
      <code class="doc-inline px-1 py-0.5">lazypock_authenticated</code> roles to
      exist and be reachable by the connecting role; if they are not, the server
      <strong>refuses to start</strong> rather than serving without them, because a
      request that cannot switch role fails anyway (every anonymous and
      authenticated call would answer 500 while the Studio kept working). A
      deployment that runs on superuser tokens alone can set
      <code class="doc-inline px-1 py-0.5">LAZYPOCK_REQUIRE_RLS=off</code> to keep
      the warning instead. See
      <code class="doc-inline px-1 py-0.5">README.md</code> for the CLI
      (<code class="doc-inline px-1 py-0.5">serve</code>,
      <code class="doc-inline px-1 py-0.5">migrate</code>,
      <code class="doc-inline px-1 py-0.5">superuser</code>,
      <code class="doc-inline px-1 py-0.5">config</code>,
      <code class="doc-inline px-1 py-0.5">healthcheck</code>).
    </p>
  </section>

  <!-- PRODUCTION -->
  <section id="production" class="scroll-mt-20 mb-10">
    <h2 class="text-xl font-semibold border-b border-base-300 pb-2">
      Production deployment
    </h2>
    <p class="mt-2 text-base-content/80 leading-relaxed">
      Lazypock is one binary: the API and the Studio together. The repository
      root has a <code class="doc-inline px-1 py-0.5">Dockerfile</code> that
      builds it, and a <code class="doc-inline px-1 py-0.5">compose.yml</code>
      that runs it next to Postgres:
    </p>
    <CodeBlock
      lang="bash"
      code={`docker compose up --build
# Studio: http://localhost:4000/_/`}
    />
    <p class="mt-3 text-base-content/80 leading-relaxed">
      Two variables have to be set before the stack is reachable from anywhere:
    </p>
    <ul
      class="mt-2 space-y-1 text-base-content/80 leading-relaxed list-disc pl-5"
    >
      <li>
        <code class="doc-inline px-1 py-0.5">LAZYPOCK_DATABASE_URL</code> — a
        <strong>direct</strong> connection. A transaction-mode pooler such as
        PgBouncer, and the pooled endpoint managed providers offer, are not
        supported: realtime needs
        <code class="doc-inline px-1 py-0.5">LISTEN/NOTIFY</code> on the direct
        endpoint, and a prepared statement has to survive a pool checkout.
      </li>
      <li>
        <code class="doc-inline px-1 py-0.5">LAZYPOCK_SECRET_KEY</code> — 32
        bytes as hex (<code class="doc-inline px-1 py-0.5"
          >openssl rand -hex 32</code
        >). It encrypts the secrets stored in the database, so losing it means
        those can no longer be decrypted.
      </li>
    </ul>
    <p class="mt-3 text-sm text-base-content/70">
      Setting <code class="doc-inline px-1 py-0.5">LAZYPOCK_SIGNING_KEY</code>
      is optional, and it keeps the token signing key out of the database — and
      therefore out of every dump and replica. Left unset, it is generated on
      first run and kept there, encrypted with
      <code class="doc-inline px-1 py-0.5">LAZYPOCK_SECRET_KEY</code> when that
      is set (a plaintext key from an older version is rewritten encrypted the
      next time the server starts). A key that cannot be decrypted stops the
      boot and names the variable, rather than quietly signing everybody out
      with a new one.
    </p>
    <p class="mt-3 text-sm text-base-content/70">
      Behind a reverse proxy, set
      <code class="doc-inline px-1 py-0.5">LAZYPOCK_TRUSTED_PROXIES</code> to the
      proxy&rsquo;s addresses. Without it no forwarding header is read at all, so
      rate limits and request logs would key on the proxy rather than the client.
    </p>
    <p class="mt-3 text-sm text-base-content/70">
      The image runs as a non-root user and writes only to
      <code class="doc-inline px-1 py-0.5">/data</code>, which compose mounts as
      a volume. It needs no
      <code class="doc-inline px-1 py-0.5">ImageMagick</code>: thumbnails and
      scaled images are produced in-process. A health check is built in
      (<code class="doc-inline px-1 py-0.5">lazypock healthcheck</code>), and
      <code class="doc-inline px-1 py-0.5"
        >docker run … superuser create</code
      >
      works for first-time setup.
    </p>
    <p class="mt-3 text-sm text-base-content/70">
      On <a
        class="text-primary underline"
        href="https://coolify.io"
        target="_blank"
        rel="noreferrer">Coolify</a
      >, point a resource at this repository and let it build the Dockerfile, or
      deploy the published image. Coolify terminates TLS in front, and schedules
      database and volume backups.
    </p>
  </section>

  <!-- IMAGE PROCESSING -->
  <section id="images" class="scroll-mt-20 mb-10">
    <h2 class="text-xl font-semibold border-b border-base-300 pb-2">
      Image processing
    </h2>
    <p class="mt-2 text-base-content/80 leading-relaxed">
      Resizing runs <strong>in the API process</strong> by default, using image
      codecs compiled into the binary. There is no ImageMagick and no sidecar:
      that is what keeps the binary self-contained. The named presets, upload
      thumbnails and on-demand
      <code class="doc-inline px-1 py-0.5">GET /api/files/:id/scale/:preset</code>
      requests all go through the same code path.
    </p>

    <h3 class="mt-5 font-semibold">Moving resizing out of the process</h3>
    <p class="mt-2 text-base-content/80 leading-relaxed">
      A large image decodes into peak memory in the request worker. Set
      <code class="doc-inline px-1 py-0.5">LAZYPOCK_IMAGE_SERVICE_URL</code> to
      move that work — and its memory — to a service you run:
    </p>
    <CodeBlock
      lang="bash"
      code={`# a service that accepts the source bytes and the resize parameters
export LAZYPOCK_IMAGE_SERVICE_URL="https://images.example.com/resize"
# optional bearer token
export LAZYPOCK_IMAGE_SERVICE_TOKEN="$(openssl rand -hex 32)"

# a thin binary with no codecs of its own: the service does all resizing
cargo build --release --no-default-features`}
    />
    <p class="mt-2 text-sm text-base-content/70">
      The request is
      <code class="doc-inline px-1 py-0.5"
        >POST &lt;url&gt;?width=…&amp;height=…&amp;fit=…&amp;format=…&amp;quality=…</code
      >
      with the source bytes as the body and the encoded image as the response.
      <code class="doc-inline px-1 py-0.5">fit</code> is one of
      <code class="doc-inline px-1 py-0.5">cover|contain|fill|inside|outside</code>,
      <code class="doc-inline px-1 py-0.5">format</code> one of
      <code class="doc-inline px-1 py-0.5">webp|png|jpeg|avif</code>. A failure is
      <strong>never</strong> silently fallen back to the local codecs: a
      deployment that pointed at a service did so to keep the CPU elsewhere. The
      full contract is in
      <code class="doc-inline px-1 py-0.5">crates/files/src/image_service.rs</code>.
    </p>
    <p class="mt-3 text-sm text-base-content/70">
      Which variants exist, their sizes, and whether they are generated during
      upload (eager) or on first request (lazy) is configured with
      <code class="doc-inline px-1 py-0.5">_image_presets</code> in the Studio —
      see
      <a class="text-primary underline" href="/files">File storage &amp; images</a>.
    </p>
  </section>

  <!-- ENV VARS -->
  <section id="env-vars" class="scroll-mt-20 mb-14">
    <h2 class="text-xl font-semibold border-b border-base-300 pb-2">
      Environment variables
    </h2>
    <p class="mt-3 text-base-content/80 leading-relaxed">
      Only these <code class="doc-inline px-1 py-0.5">LAZYPOCK_</code>-prefixed
      names are read. A value that cannot be parsed is refused at boot rather
      than silently defaulted. The same list lives in
      <code class="doc-inline px-1 py-0.5">.env.example</code>.
    </p>
    <p class="mt-3 text-base-content/80 leading-relaxed">
      <strong>Only <code class="doc-inline px-1 py-0.5">LAZYPOCK_DATABASE_URL</code> is
      required</strong> — everything else has a default, so a server starts with a
      database URL and nothing more. The exceptions are per row: an S3 bucket needs
      its credentials, and a stored secret needs
      <code class="doc-inline px-1 py-0.5">LAZYPOCK_SECRET_KEY</code> before it can
      be saved.
    </p>

    <div class="mt-5 rounded-box border border-base-300 bg-base-200/40 p-4">
      <p class="font-semibold">The environment supplies defaults; the database overrides them</p>
      <p class="mt-2 text-sm text-base-content/80 leading-relaxed">
        There are three kinds of setting, and they behave differently:
      </p>
      <ul class="mt-2 space-y-2 text-sm text-base-content/80 list-disc list-inside">
        <li>
          <strong>Bootstrap.</strong> The database URL, listen address, pool size,
          log level and the secrets. There is no other source — these are read
          from the environment and nowhere else.
        </li>
        <li>
          <strong>Defaults for a setting the database also holds.</strong> File
          storage is the one case today:
          <code class="doc-inline px-1 py-0.5">LAZYPOCK_FILES_*</code>,
          <code class="doc-inline px-1 py-0.5">LAZYPOCK_S3_*</code> and
          <code class="doc-inline px-1 py-0.5">LAZYPOCK_IMAGE_SERVICE_*</code>
          supply the starting configuration, and a value saved in <strong>Studio →
          Settings → Storage</strong> <em>wins over all of them</em> — and takes
          effect without a restart. Leave them unset and configure storage in the
          UI once, or set them so a deployment is reproducible from its
          environment alone.
        </li>
        <li>
          <strong>Database only.</strong> CORS origins, SMTP credentials, cron
          jobs, API keys, collection rules and image presets have no environment
          variable at all; they are edited in the Studio.
        </li>
      </ul>
      <p class="mt-2 text-sm text-base-content/80 leading-relaxed">
        One direction only: the environment is the default, the database is the
        override. <code class="doc-inline px-1 py-0.5">deploy/compose.example.yml</code>
        is a worked deployment showing how they fit together.
      </p>
    </div>

    <h3 class="mt-6 text-lg font-semibold">Core</h3>
    <div class="mt-3 overflow-x-auto rounded-box border border-base-300">
      <table class="w-full text-sm">
        <thead class="bg-base-200 text-left">
          <tr>
            <th class="px-3 py-2 font-semibold">Variable</th>
            <th class="px-3 py-2 font-semibold">Description</th>
            <th class="px-3 py-2 font-semibold">Default / example</th>
            <th class="px-3 py-2 font-semibold">Required?</th>
          </tr>
        </thead>
        <tbody class="divide-y divide-base-300">
          <tr>
            <td class="px-3 py-2"
              ><code class="doc-inline px-1 py-0.5">LAZYPOCK_DATABASE_URL</code></td
            >
            <td class="px-3 py-2"
              >Direct Postgres connection string (<strong>required</strong>)</td
            >
            <td class="px-3 py-2 font-mono text-xs"
              >postgres://user:pass@host:5432/lazypock</td
            >
          <td class="px-3 py-2"><strong>yes</strong></td>
          </tr>
          <tr>
            <td class="px-3 py-2"
              ><code class="doc-inline px-1 py-0.5">LAZYPOCK_ENV</code></td
            >
            <td class="px-3 py-2"
              ><code class="doc-inline px-1 py-0.5">development</code> /
              <code class="doc-inline px-1 py-0.5">production</code> /
              <code class="doc-inline px-1 py-0.5">test</code></td
            >
            <td class="px-3 py-2 font-mono text-xs">development</td>
          <td class="px-3 py-2">no</td>
          </tr>
          <tr>
            <td class="px-3 py-2"
              ><code class="doc-inline px-1 py-0.5">LAZYPOCK_LISTEN_ADDR</code></td
            >
            <td class="px-3 py-2">Address the HTTP server binds to</td>
            <td class="px-3 py-2 font-mono text-xs">0.0.0.0:4000</td>
          <td class="px-3 py-2">no</td>
          </tr>
          <tr>
            <td class="px-3 py-2"
              ><code class="doc-inline px-1 py-0.5">LAZYPOCK_DATA_DIR</code></td
            >
            <td class="px-3 py-2">Local data directory (local files, outbox spool)</td>
            <td class="px-3 py-2 font-mono text-xs">data</td>
          <td class="px-3 py-2">no</td>
          </tr>
          <tr>
            <td class="px-3 py-2"
              ><code class="doc-inline px-1 py-0.5">LAZYPOCK_HOOKS_DIR</code></td
            >
            <td class="px-3 py-2">Directory for TypeScript hook files</td>
            <td class="px-3 py-2 font-mono text-xs">hooks</td>
          <td class="px-3 py-2">no</td>
          </tr>
          <tr>
            <td class="px-3 py-2"
              ><code class="doc-inline px-1 py-0.5">LAZYPOCK_LOG_LEVEL</code></td
            >
            <td class="px-3 py-2"
              ><code class="doc-inline px-1 py-0.5"
                >trace|debug|info|warn|error</code
              ></td
            >
            <td class="px-3 py-2 font-mono text-xs">info</td>
          <td class="px-3 py-2">no</td>
          </tr>
          <tr>
            <td class="px-3 py-2"
              ><code class="doc-inline px-1 py-0.5">LAZYPOCK_LOG_FORMAT</code></td
            >
            <td class="px-3 py-2"
              ><code class="doc-inline px-1 py-0.5">text</code> /
              <code class="doc-inline px-1 py-0.5">json</code></td
            >
            <td class="px-3 py-2 font-mono text-xs">text (json in production)</td>
          <td class="px-3 py-2">no</td>
          </tr>
          <tr>
            <td class="px-3 py-2"
              ><code class="doc-inline px-1 py-0.5">LAZYPOCK_POOL_SIZE</code></td
            >
            <td class="px-3 py-2"
              >Postgres pool per node (1-64); total connections = nodes ×
              (pool + 1)</td
            >
            <td class="px-3 py-2 font-mono text-xs">5</td>
          <td class="px-3 py-2">no</td>
          </tr>
          <tr>
            <td class="px-3 py-2"
              ><code class="doc-inline px-1 py-0.5"
                >LAZYPOCK_PG_PREPARED_STATEMENTS</code
              ></td
            >
            <td class="px-3 py-2">Use named prepared statements</td>
            <td class="px-3 py-2 font-mono text-xs">true</td>
          <td class="px-3 py-2">no</td>
          </tr>
          <tr>
            <td class="px-3 py-2"
              ><code class="doc-inline px-1 py-0.5"
                >LAZYPOCK_UPLOAD_MAX_BYTES</code
              ></td
            >
            <td class="px-3 py-2">Maximum accepted upload size, in bytes</td>
            <td class="px-3 py-2 font-mono text-xs">5242880 (5 MiB)</td>
          <td class="px-3 py-2">no</td>
          </tr>
          <tr>
            <td class="px-3 py-2"
              ><code class="doc-inline px-1 py-0.5">LAZYPOCK_PUBLIC_URL</code></td
            >
            <td class="px-3 py-2">Public base URL when it differs from the host</td>
            <td class="px-3 py-2 font-mono text-xs">—</td>
          <td class="px-3 py-2">no</td>
          </tr>
        </tbody>
      </table>
    </div>

    <h3 class="mt-6 text-lg font-semibold">Request handling</h3>
    <div class="mt-3 overflow-x-auto rounded-box border border-base-300">
      <table class="w-full text-sm">
        <thead class="bg-base-200 text-left">
          <tr>
            <th class="px-3 py-2 font-semibold">Variable</th>
            <th class="px-3 py-2 font-semibold">Description</th>
            <th class="px-3 py-2 font-semibold">Default / example</th>
            <th class="px-3 py-2 font-semibold">Required?</th>
          </tr>
        </thead>
        <tbody class="divide-y divide-base-300">
          <tr>
            <td class="px-3 py-2"
              ><code class="doc-inline px-1 py-0.5"
                >LAZYPOCK_TRUSTED_PROXIES</code
              ></td
            >
            <td class="px-3 py-2"
              >Peers whose forwarding headers are believed (address or CIDR, comma
              separated). Empty reads no header at all.</td
            >
            <td class="px-3 py-2 font-mono text-xs">—</td>
          <td class="px-3 py-2">no</td>
          </tr>
          <tr>
            <td class="px-3 py-2"
              ><code class="doc-inline px-1 py-0.5"
                >LAZYPOCK_RATE_LIMIT_AUTH</code
              ></td
            >
            <td class="px-3 py-2"
              >Requests/min per address that spend credentials; 0 disables</td
            >
            <td class="px-3 py-2 font-mono text-xs">20</td>
          <td class="px-3 py-2">no</td>
          </tr>
          <tr>
            <td class="px-3 py-2"
              ><code class="doc-inline px-1 py-0.5"
                >LAZYPOCK_RATE_LIMIT_API</code
              ></td
            >
            <td class="px-3 py-2">Requests/min per address for everything else</td>
            <td class="px-3 py-2 font-mono text-xs">600</td>
          <td class="px-3 py-2">no</td>
          </tr>
          <tr>
            <td class="px-3 py-2"
              ><code class="doc-inline px-1 py-0.5"
                >LAZYPOCK_RATE_LIMIT_ACCOUNT</code
              ></td
            >
            <td class="px-3 py-2"
              >Failed sign-ins per account before a 15-minute lockout. Only
              failures count; 0 disables it.</td
            >
            <td class="px-3 py-2 font-mono text-xs">10</td>
          <td class="px-3 py-2">no</td>
          </tr>
        </tbody>
      </table>
    </div>

    <h3 class="mt-6 text-lg font-semibold">Secrets</h3>
    <div class="mt-3 overflow-x-auto rounded-box border border-base-300">
      <table class="w-full text-sm">
        <thead class="bg-base-200 text-left">
          <tr>
            <th class="px-3 py-2 font-semibold">Variable</th>
            <th class="px-3 py-2 font-semibold">Description</th>
            <th class="px-3 py-2 font-semibold">Default / example</th>
            <th class="px-3 py-2 font-semibold">Required?</th>
          </tr>
        </thead>
        <tbody class="divide-y divide-base-300">
          <tr>
            <td class="px-3 py-2"
              ><code class="doc-inline px-1 py-0.5">LAZYPOCK_SECRET_KEY</code></td
            >
            <td class="px-3 py-2"
              >AES-256-GCM key for the secrets stored in
              <code class="doc-inline px-1 py-0.5">_settings</code>: the storage
              secret, and the access-token signing key when that is not supplied
              below. Required before a storage secret can be saved</td
            >
            <td class="px-3 py-2 font-mono text-xs">openssl rand -hex 32</td>
          <td class="px-3 py-2">no</td>
          </tr>
          <tr>
            <td class="px-3 py-2"
              ><code class="doc-inline px-1 py-0.5">LAZYPOCK_SIGNING_KEY</code></td
            >
            <td class="px-3 py-2"
              >Access-token signing key (base64), kept out of the database
              entirely. Without it the key is generated into
              <code class="doc-inline px-1 py-0.5">_settings</code>, encrypted
              with <code class="doc-inline px-1 py-0.5">LAZYPOCK_SECRET_KEY</code>
              when that is set</td
            >
            <td class="px-3 py-2 font-mono text-xs">openssl rand -base64 32</td>
          <td class="px-3 py-2">no</td>
          </tr>
          <tr>
            <td class="px-3 py-2"
              ><code class="doc-inline px-1 py-0.5"
                >LAZYPOCK_SUPERUSER_EMAIL</code
              ></td
            >
            <td class="px-3 py-2"
              >With the password below, creates the first superuser at boot —
              only while none exists. Also read by
              <code class="doc-inline px-1 py-0.5"
                >lazypock superuser create</code
              >. There is no default account and no default password</td
            >
            <td class="px-3 py-2 font-mono text-xs">you@example.com</td>
          <td class="px-3 py-2">no</td>
          </tr>
          <tr>
            <td class="px-3 py-2"
              ><code class="doc-inline px-1 py-0.5"
                >LAZYPOCK_SUPERUSER_PASSWORD</code
              ></td
            >
            <td class="px-3 py-2"
              >The first superuser's password. Set both of this pair or neither:
              half of one is a boot error</td
            >
            <td class="px-3 py-2 font-mono text-xs">—</td>
          <td class="px-3 py-2">no</td>
          </tr>
        </tbody>
      </table>
    </div>

    <h3 class="mt-6 text-lg font-semibold">File storage and images</h3>
    <div class="mt-3 overflow-x-auto rounded-box border border-base-300">
      <table class="w-full text-sm">
        <thead class="bg-base-200 text-left">
          <tr>
            <th class="px-3 py-2 font-semibold">Variable</th>
            <th class="px-3 py-2 font-semibold">Description</th>
            <th class="px-3 py-2 font-semibold">Default / example</th>
            <th class="px-3 py-2 font-semibold">Required?</th>
          </tr>
        </thead>
        <tbody class="divide-y divide-base-300">
          <tr>
            <td class="px-3 py-2"
              ><code class="doc-inline px-1 py-0.5"
                >LAZYPOCK_FILES_ADAPTER</code
              ></td
            >
            <td class="px-3 py-2"
              ><code class="doc-inline px-1 py-0.5">local</code> /
              <code class="doc-inline px-1 py-0.5">s3</code>; a saved Studio
              setting overrides it</td
            >
            <td class="px-3 py-2 font-mono text-xs">local</td>
          <td class="px-3 py-2">no</td>
          </tr>
          <tr>
            <td class="px-3 py-2"
              ><code class="doc-inline px-1 py-0.5">LAZYPOCK_FILES_DIR</code></td
            >
            <td class="px-3 py-2">Root directory for the local adapter</td>
            <td class="px-3 py-2 font-mono text-xs">data/files</td>
          <td class="px-3 py-2">no</td>
          </tr>
          <tr>
            <td class="px-3 py-2"
              ><code class="doc-inline px-1 py-0.5"
                >LAZYPOCK_FILES_PUBLIC_URL</code
              ></td
            >
            <td class="px-3 py-2">Public/CDN base URL for served objects</td>
            <td class="px-3 py-2 font-mono text-xs">—</td>
          <td class="px-3 py-2">no</td>
          </tr>
          <tr>
            <td class="px-3 py-2"
              ><code class="doc-inline px-1 py-0.5">LAZYPOCK_S3_ENDPOINT</code>,
              <code class="doc-inline px-1 py-0.5">_BUCKET</code>,
              <code class="doc-inline px-1 py-0.5">_REGION</code>,
              <code class="doc-inline px-1 py-0.5">_ACCESS_KEY</code>,
              <code class="doc-inline px-1 py-0.5">_SECRET_KEY</code></td
            >
            <td class="px-3 py-2"
              >S3/R2 backend. All required when the adapter is
              <code class="doc-inline px-1 py-0.5">s3</code>.</td
            >
            <td class="px-3 py-2 font-mono text-xs">region: auto</td>
          <td class="px-3 py-2">no <span class="whitespace-nowrap">(S3 only)</span></td>
          </tr>
          <tr>
            <td class="px-3 py-2"
              ><code class="doc-inline px-1 py-0.5">LAZYPOCK_PRESIGN_TTL</code></td
            >
            <td class="px-3 py-2"
              >Presigned URL lifetime, in seconds (1 s-7 days)</td
            >
            <td class="px-3 py-2 font-mono text-xs">900</td>
          <td class="px-3 py-2">no</td>
          </tr>
          <tr>
            <td class="px-3 py-2"
              ><code class="doc-inline px-1 py-0.5"
                >LAZYPOCK_IMAGE_SERVICE_URL</code
              ></td
            >
            <td class="px-3 py-2">Resize in this service instead of in-process</td>
            <td class="px-3 py-2 font-mono text-xs">—</td>
          <td class="px-3 py-2">no</td>
          </tr>
          <tr>
            <td class="px-3 py-2"
              ><code class="doc-inline px-1 py-0.5"
                >LAZYPOCK_IMAGE_SERVICE_TOKEN</code
              ></td
            >
            <td class="px-3 py-2">Bearer token sent to that service</td>
            <td class="px-3 py-2 font-mono text-xs">—</td>
          <td class="px-3 py-2">no</td>
          </tr>
        </tbody>
      </table>
    </div>

    <h3 class="mt-6 text-lg font-semibold">Background workers</h3>
    <div class="mt-3 overflow-x-auto rounded-box border border-base-300">
      <table class="w-full text-sm">
        <thead class="bg-base-200 text-left">
          <tr>
            <th class="px-3 py-2 font-semibold">Variable</th>
            <th class="px-3 py-2 font-semibold">Description</th>
            <th class="px-3 py-2 font-semibold">Default</th>
            <th class="px-3 py-2 font-semibold">Required?</th>
          </tr>
        </thead>
        <tbody class="divide-y divide-base-300">
          <tr>
            <td class="px-3 py-2"
              ><code class="doc-inline px-1 py-0.5"
                >LAZYPOCK_REFLECT_EXISTING</code
              ></td
            >
            <td class="px-3 py-2"
              >Adopt tables lazypock did not create as superuser-only collections
              (<code class="doc-inline px-1 py-0.5">on</code>/<code
                class="doc-inline px-1 py-0.5">off</code>)</td
            >
            <td class="px-3 py-2 font-mono text-xs">on</td>
          <td class="px-3 py-2">no</td>
          </tr>
          <tr>
            <td class="px-3 py-2"
              ><code class="doc-inline px-1 py-0.5"
                >LAZYPOCK_REQUIRE_RLS</code
              ></td
            >
            <td class="px-3 py-2"
              >Fail boot when the restricted roles cannot be switched to. Set
              <code class="doc-inline px-1 py-0.5">off</code> only for a
              superuser-only deployment, where every end-user request would fail
              anyway</td
            >
            <td class="px-3 py-2 font-mono text-xs">on</td>
          <td class="px-3 py-2">no</td>
          </tr>
          <tr>
            <td class="px-3 py-2"
              ><code class="doc-inline px-1 py-0.5"
                >LAZYPOCK_CRON_INTERVAL</code
              ></td
            >
            <td class="px-3 py-2"
              >Cron scheduler tick, in seconds; 0 disables it (loudly)</td
            >
            <td class="px-3 py-2 font-mono text-xs">30</td>
          <td class="px-3 py-2">no</td>
          </tr>
          <tr>
            <td class="px-3 py-2"
              ><code class="doc-inline px-1 py-0.5"
                >LAZYPOCK_JOB_INTERVAL</code
              ></td
            >
            <td class="px-3 py-2"
              >Job-queue worker tick, in seconds; 0 disables it (loudly)</td
            >
            <td class="px-3 py-2 font-mono text-xs">5</td>
          <td class="px-3 py-2">no</td>
          </tr>
          <tr>
            <td class="px-3 py-2"
              ><code class="doc-inline px-1 py-0.5">LAZYPOCK_JOB_LEASE</code></td
            >
            <td class="px-3 py-2"
              >How long a job claim is trusted before it is handed back, in
              seconds</td
            >
            <td class="px-3 py-2 font-mono text-xs">300</td>
          <td class="px-3 py-2">no</td>
          </tr>
          <tr>
            <td class="px-3 py-2"
              ><code class="doc-inline px-1 py-0.5"
                >LAZYPOCK_FILE_REAPER_INTERVAL</code
              ></td
            >
            <td class="px-3 py-2"
              >File-reaper tick, in seconds; 0 disables it (loudly)</td
            >
            <td class="px-3 py-2 font-mono text-xs">3600</td>
          <td class="px-3 py-2">no</td>
          </tr>
          <tr>
            <td class="px-3 py-2"
              ><code class="doc-inline px-1 py-0.5"
                >LAZYPOCK_IMPORT_CONFIRM_RECORDS</code
              ></td
            >
            <td class="px-3 py-2">Records above which an import must be confirmed</td>
            <td class="px-3 py-2 font-mono text-xs">10000</td>
          <td class="px-3 py-2">no</td>
          </tr>
        </tbody>
      </table>
    </div>

    <p class="mt-4 text-sm text-base-content/70">
      Two further variables are CLI/build-time and are not read by a running
      server: <code class="doc-inline px-1 py-0.5">LAZYPOCK_STUDIO_DIR</code>
      (a prebuilt Studio bundle to embed) and
      <code class="doc-inline px-1 py-0.5">LAZYPOCK_SUPERUSER_PASSWORD</code>
      (for <code class="doc-inline px-1 py-0.5">superuser create</code>).
    </p>
  </section>
</div>
