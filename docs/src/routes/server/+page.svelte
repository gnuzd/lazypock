<script lang="ts">
  import CodeBlock from "$lib/components/CodeBlock.svelte";
  import Seo from '$lib/components/Seo.svelte';

  const title = 'Server Guide — Lazypock Docs';
  const description =
    'Run and operate a Lazypock server: Docker image or prebuilt binary, first-time setup, filters, production checklist, ImageMagick and every environment variable.';
</script>

<Seo {title} {description} />

<div class="prose-doc max-w-3xl">
  <!-- INTRO -->
  <section class="scroll-mt-20 mb-14">
    <div class="flex flex-wrap items-center gap-2">
      <span
        class="rounded-field bg-info/10 text-info px-2 py-0.5 text-xs font-medium"
        >Server · Elixir + Phoenix + PostgreSQL</span
      >
    </div>
    <h1 class="mt-3 text-3xl font-bold tracking-tight">Server Guide</h1>
    <p class="mt-3 text-base-content/80 leading-relaxed">
      How to run and operate a Lazypock server. The repo is a monorepo with two
      parts: the
      <strong>core</strong> backend (Elixir + Phoenix + PostgreSQL) and the
      <strong>Studio</strong>
      admin UI (SvelteKit, served by the backend at
      <code class="doc-inline px-1 py-0.5">/_/</code>). Your app talks to the
      server through one of the
      <a class="text-primary underline" href="/sdk">SDKs</a> — no backend code required.
    </p>

    <h2 class="mt-8 text-xl font-semibold border-b border-base-300 pb-2">
      Prerequisites
    </h2>
    <ul class="mt-3 space-y-2 text-base-content/80 list-disc list-inside">
      <li>
        <strong>Elixir 1.17+</strong> and <strong>Erlang/OTP 26+</strong> (only for
        running from source)
      </li>
      <li><strong>PostgreSQL 15+</strong></li>
      <li>
        <strong>Node.js 20+</strong> (only for the Studio dev server and the SDKs)
      </li>
      <li>
        <strong>ImageMagick 6 or 7</strong> — required for image thumbnails,
        presets and on-demand scaling. It is a <em>host</em> dependency when you
        run the binary or from source (the Docker image bundles it); see
        <a class="text-primary underline" href="#images">Image processing</a>.
        Uploads still work without it, but no resized variants are produced.
      </li>
      <li>
        <code class="doc-inline px-1 py-0.5">zig</code> and
        <code class="doc-inline px-1 py-0.5">xz</code> — only needed for Burrito
        production release builds
      </li>
    </ul>
    <p class="mt-3 text-sm text-base-content/70">
      The Docker quick start below skips the Elixir/Node prerequisites entirely
      — just Docker and the published image.
    </p>
  </section>

  <!-- QUICK START (DOCKER) -->
  <section id="quick-start" class="scroll-mt-20 mb-10">
    <h2 class="text-xl font-semibold border-b border-base-300 pb-2">
      Quick start — Docker image
    </h2>
    <p class="mt-3 text-base-content/80 leading-relaxed">
      No Elixir, Erlang, or source checkout needed — just Docker and the
      published image (<code class="doc-inline px-1 py-0.5">gnuzd/lazypock</code
      >, built from the released Linux binary). Save this as
      <code class="doc-inline px-1 py-0.5">compose.yml</code> and run
      <code class="doc-inline px-1 py-0.5">docker compose up -d</code>:
    </p>
    <CodeBlock
      lang="yaml"
      code={`services:
  lazypock:
    image: gnuzd/lazypock:latest
    ports: ["4000:4000"]
    environment:
      DATABASE_URL: ecto://postgres:postgres@postgres:5432/lazypock
      # generate one with: openssl rand -base64 48
      SECRET_KEY_BASE: change-me
      LAZYPOCK_SUPERUSER_EMAIL: admin@lazypock.app
      LAZYPOCK_SUPERUSER_PASSWORD: admin123
    volumes: ["uploads:/data/uploads"]
  postgres:
    image: postgres:16-alpine
    environment:
      POSTGRES_PASSWORD: postgres
    volumes: ["pgdata:/var/lib/postgresql/data"]
volumes:
  uploads:
  pgdata:`}
    />
    <p class="mt-3 text-base-content/80 leading-relaxed">
      Already run a PostgreSQL 15+? One container is enough — just point
      <code class="doc-inline px-1 py-0.5">DATABASE_URL</code> at it:
    </p>
    <CodeBlock
      lang="bash"
      code={`docker run -d -p 4000:4000 \\
  -e DATABASE_URL="ecto://user:pass@host:5432/db" \\
  -e SECRET_KEY_BASE="$(openssl rand -base64 48)" \\
  -v lazypock-uploads:/data/uploads \\
  gnuzd/lazypock:latest`}
    />
    <div
      class="rounded-box border border-base-300 bg-base-200/60 p-4 my-4 text-sm leading-relaxed"
    >
      <ul class="space-y-1.5 list-disc list-inside">
        <li>
          Server + Studio admin UI: <code class="doc-inline px-1 py-0.5"
            >http://localhost:4000</code
          >
          (login at <code class="doc-inline px-1 py-0.5">/_/</code> — the superuser is
          created on first boot from <code class="doc-inline px-1 py-0.5"
            >LAZYPOCK_SUPERUSER_EMAIL</code
          >/<code class="doc-inline px-1 py-0.5">LAZYPOCK_SUPERUSER_PASSWORD</code>)
        </li>
        <li>
          REST API: <code class="doc-inline px-1 py-0.5"
            >http://localhost:4000/api/...</code
          >
        </li>
        <li>
          The image bundles ImageMagick, so thumbnails and on-demand scaling work
          out of the box.
        </li>
        <li>
          Pin a version (<code class="doc-inline px-1 py-0.5"
            >gnuzd/lazypock:0.20.0</code
          >) rather than <code class="doc-inline px-1 py-0.5">latest</code> in
          production.
        </li>
      </ul>
    </div>
    <p class="mt-3 text-base-content/80 leading-relaxed">
      To reset everything (including the database):
    </p>
    <CodeBlock lang="bash" code={`docker compose down -v`} />
    <p class="mt-3 text-base-content/80 leading-relaxed">
      Prefer a single binary over a container? See
      <a class="text-primary underline" href="#binary">Prebuilt binary</a> below.
    </p>
    <p class="mt-3 text-base-content/80 leading-relaxed">
      Prefer no Docker at all? Any PostgreSQL 15+ works — just point
      <code class="doc-inline px-1 py-0.5">DATABASE_URL</code> at it. Or run
      from source — see
      <a class="text-primary underline" href="#manual">Manual setup</a> below.
    </p>
    <p class="mt-3 text-base-content/80 leading-relaxed">
      Ready to connect an app? Follow the
      <a class="text-primary underline" href="/sdk/typescript"
        >TypeScript SDK quick start</a
      >.
    </p>
  </section>

  <!-- PREBUILT BINARY -->
  <section id="binary" class="scroll-mt-20 mb-10">
    <h2 class="text-xl font-semibold border-b border-base-300 pb-2">
      Prebuilt binary
    </h2>
    <p class="mt-2 text-base-content/80 leading-relaxed">
      No Docker, no Elixir toolchain — grab a prebuilt single-binary release
      (built with Burrito) straight from GitHub Releases and run it directly:
    </p>
    <CodeBlock
      lang="bash"
      code={`# Download the latest release for your platform from:
# https://github.com/gnuzd/lazypock/releases/latest

chmod +x lazypock_macos_silicon   # or the binary matching your OS/arch

export DATABASE_URL="ecto://postgres:postgres@localhost:5432/lazypock"
export SECRET_KEY_BASE="$(openssl rand -base64 48)"
LAZYPOCK_SUPERUSER_EMAIL=admin@example.com LAZYPOCK_SUPERUSER_PASSWORD=changeme \\
  ./lazypock_macos_silicon`}
    />
    <p class="mt-3 text-sm text-base-content/70">
      You still need a reachable PostgreSQL 15+ instance (e.g. via
      <code class="doc-inline px-1 py-0.5"
        >docker run -p 5432:5432 postgres:16-alpine</code
      >). The binary handles migrations, seeding, and serving the Studio UI on
      its own — see
      <a
        class="text-primary underline"
        href="https://github.com/gnuzd/lazypock/releases"
        target="_blank"
        rel="noreferrer">Releases</a
      >
      for available platforms and checksums.
    </p>
    <p class="mt-3 text-sm text-base-content/70">
      The binary bundles the app, <strong>not its system dependencies</strong>.
      In particular <strong>ImageMagick is not included</strong>: install it on
      the host (or in your container image) if you want thumbnails and image
      scaling — see <a class="text-primary underline" href="#images"
        >Image processing</a
      >.
    </p>
  </section>

  <!-- MANUAL -->
  <section id="manual" class="scroll-mt-20 mb-10">
    <h2 class="text-xl font-semibold border-b border-base-300 pb-2">
      Manual setup (from source)
    </h2>

    <h3 class="mt-6 text-lg font-semibold">1. Run the backend (Phoenix)</h3>
    <CodeBlock
      lang="bash"
      code={`git clone git@github.com:gnuzd/lazypock.git
cd lazypock/core

export DATABASE_URL="ecto://postgres:postgres@localhost:5432/lazypock_dev"
mix setup          # install deps, create DB, run migrations, seed
mix phx.server      # starts Phoenix on http://localhost:4000`}
    />
    <p class="mt-2 text-sm text-base-content/70">
      The REST API and realtime channels are served at <code
        class="doc-inline px-1 py-0.5">http://localhost:4000</code
      >.
    </p>

    <h3 class="mt-6 text-lg font-semibold">2. Run Studio (Admin UI)</h3>
    <CodeBlock
      lang="bash"
      code={`cd lazypock/studio

npm install
npm run dev          # starts Vite dev server on http://localhost:5173`}
    />
    <p class="mt-2 text-base-content/80 leading-relaxed">
      The SvelteKit dev server proxies <code class="doc-inline px-1 py-0.5"
        >/api</code
      >
      requests to the Phoenix backend on port 4000. Studio itself is served at
      <code class="doc-inline px-1 py-0.5">http://localhost:5173/_/</code>.
    </p>

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
    <ol class="mt-3 list-decimal list-inside space-y-1 text-base-content/80">
      <li>
        Open Studio at <code class="doc-inline px-1 py-0.5"
          >http://localhost:5173/_/</code
        >
        (or <code class="doc-inline px-1 py-0.5">/_</code> if served directly from
        Phoenix)
      </li>
      <li>You'll be redirected to the login page</li>
      <li>
        Click <strong>Setup</strong> to create the first superuser account
      </li>
      <li>
        Log in and start creating collections — every collection you create in
        Studio gets an instant REST API + realtime channel + rules, ready to
        call from any SDK
      </li>
    </ol>
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
      <code class="doc-inline px-1 py-0.5">DATABASE_URL</code> to your
      database:
    </p>
    <CodeBlock
      lang="bash"
      code={`export DATABASE_URL="ecto://user:password@db-host:5432/my_existing_db"
export SECRET_KEY_BASE="$(openssl rand -base64 48)"
LAZYPOCK_SUPERUSER_EMAIL=admin@example.com LAZYPOCK_SUPERUSER_PASSWORD=changeme \
  ./lazypock_macos_silicon`}
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
        <code class="doc-inline px-1 py-0.5">_request_logs</code>, …) plus
        <code class="doc-inline px-1 py-0.5">schema_migrations</code>. These
        are namespaced and never touch your tables.
      </li>
      <li>
        <strong>Auto-registration</strong>: every public table that isn't
        already a LazyPock collection becomes a <code
          class="doc-inline px-1 py-0.5">base</code> collection, with columns
        inferred from the Postgres schema — so your existing tables appear in
        the Studio sidebar and are served through the dynamic
        <code class="doc-inline px-1 py-0.5">/api/:collection</code> routes
        immediately.
      </li>
      <li>
        <strong>Shape reconciliation</strong> so CRUD works out of the box:
        Ecto <code class="doc-inline px-1 py-0.5">timestamps()</code>
        columns are normalized (<code
          class="doc-inline px-1 py-0.5">inserted_at</code> → <code
          class="doc-inline px-1 py-0.5">created_at</code>, a missing
        <code class="doc-inline px-1 py-0.5">updated_at</code> is added), and
        foreign-key columns become <strong>relation fields</strong> pointing
        at the referenced table — so the Studio shows a relation dropdown and
        the API supports <code class="doc-inline px-1 py-0.5">expand</code>
        on them.
      </li>
    </ol>
    <p class="mt-3 text-sm text-base-content/70">
      Internal <code class="doc-inline px-1 py-0.5">_</code>-prefixed tables
      and <code class="doc-inline px-1 py-0.5">schema_migrations</code> are
      never registered. Set
      <code class="doc-inline px-1 py-0.5">LAZYPOCK_AUTOMIGRATE=0</code> if
      you'd rather run migrations manually with
      <code class="doc-inline px-1 py-0.5">lazypock migrate</code> — note
      that auto-registration runs as part of every migrate, so tables added
      later by other tools are picked up on the next
      <code class="doc-inline px-1 py-0.5">lazypock migrate</code> (or
      restart). Log in to the Studio with the superuser created on first boot
      (or via the env vars above) and your data is ready to browse, query,
      and edit.
    </p>
  </section>

  <!-- PRODUCTION -->
  <section id="production" class="scroll-mt-20 mb-10">
    <h2 class="text-xl font-semibold border-b border-base-300 pb-2">
      Production release (single binary)
    </h2>
    <p class="mt-2 text-base-content/80 leading-relaxed">
      Lazypock ships as a single binary via <a
        class="text-primary underline"
        href="https://github.com/burrito-elixir/burrito"
        target="_blank"
        rel="noreferrer">Burrito</a
      >:
    </p>
    <CodeBlock
      lang="bash"
      code={`cd core
MIX_ENV=prod mix release
# Binary: core/burrito_out/lazypock_macos_silicon`}
    />
    <p class="mt-3 text-base-content/80 leading-relaxed">
      Minimal production run example:
    </p>
    <CodeBlock
      lang="bash"
      code={`export DATABASE_URL="ecto://postgres:postgres@localhost:5432/lazypock"
export SECRET_KEY_BASE="$(mix phx.gen.secret)"
export PHX_HOST="localhost"
LAZYPOCK_SUPERUSER_EMAIL=admin@example.com LAZYPOCK_SUPERUSER_PASSWORD=changeme \\
  ./core/burrito_out/lazypock_macos_silicon`}
    />
    <p class="mt-3 text-sm text-base-content/70">
      The HTTP server is <strong>always started</strong> — no
      <code class="doc-inline px-1 py-0.5">PHX_SERVER</code>
      needed; just run the binary (or
      <code class="doc-inline px-1 py-0.5">bin/lazypock start</code>).
    </p>
    <p class="mt-3 text-sm text-base-content/70">
      The release runs with <code class="doc-inline px-1 py-0.5"
        >RUNTIME_CONFIG=false</code
      >, so config is baked in at build time; environment variables are still
      read at boot via the Elixir config provider.
    </p>
  </section>

  <!-- IMAGE PROCESSING -->
  <section id="images" class="scroll-mt-20 mb-10">
    <h2 class="text-xl font-semibold border-b border-base-300 pb-2">
      Image processing (ImageMagick)
    </h2>
    <p class="mt-2 text-base-content/80 leading-relaxed">
      Every resize goes through the <strong>ImageMagick CLI</strong>: upload
      thumbnails, the named presets (<code class="doc-inline px-1 py-0.5">thumb</code>,
      <code class="doc-inline px-1 py-0.5">small</code>,
      <code class="doc-inline px-1 py-0.5">content</code>) and on-demand
      <code class="doc-inline px-1 py-0.5">GET /api/files/:id/scale/:preset</code>
      requests. It is a <strong>host dependency</strong> — a prebuilt binary
      does not bundle it, and an OS upgrade does not remove the need for it.
    </p>

    <h3 class="mt-5 font-semibold">Install</h3>
    <CodeBlock
      lang="bash"
      code={`# Debian / Ubuntu — ImageMagick 6 (identify + convert)
sudo apt install imagemagick

# RHEL / Fedora
sudo dnf install ImageMagick

# Alpine (containers)
apk add imagemagick

# macOS — ImageMagick 7 (magick)
brew install imagemagick`}
    />

    <h3 class="mt-5 font-semibold">Verify</h3>
    <CodeBlock
      lang="bash"
      code={`identify -version      # ImageMagick 6 and 7
magick -version        # ImageMagick 7 only

# the two binaries LazyPock shells out to:
command -v identify    # reads dimensions
command -v convert || command -v magick   # performs the resize`}
    />
    <p class="mt-3 text-sm text-base-content/70">
      Both <strong>ImageMagick 6</strong> (what Debian/Ubuntu ship, providing
      <code class="doc-inline px-1 py-0.5">identify</code> and
      <code class="doc-inline px-1 py-0.5">convert</code>) and
      <strong>ImageMagick 7</strong>
      (<code class="doc-inline px-1 py-0.5">magick</code>) are supported.
    </p>

    <h3 class="mt-5 font-semibold">If it is missing</h3>
    <ul class="mt-2 space-y-2 text-base-content/80 list-disc list-inside">
      <li>Uploads still succeed, and originals are served unchanged.</li>
      <li>
        No thumbnails or presets are generated:
        <code class="doc-inline px-1 py-0.5">GET /api/files/:id/thumbs/:size</code>
        answers <code class="doc-inline px-1 py-0.5">404</code> and
        <code class="doc-inline px-1 py-0.5">/scale/…</code> answers
        <code class="doc-inline px-1 py-0.5">400</code>.
      </li>
      <li>A one-time warning is logged on the first upload.</li>
      <li>
        Set
        <code class="doc-inline px-1 py-0.5">LAZYPOCK_THUMBNAILS=0</code> to
        disable resizing deliberately (and silence the warning).
      </li>
    </ul>

    <h3 class="mt-5 font-semibold">Resource limits</h3>
    <div class="mt-3 overflow-x-auto rounded-box border border-base-300">
      <table class="w-full text-sm">
        <thead class="bg-base-200">
          <tr>
            <th class="px-3 py-2 text-left">Variable</th>
            <th class="px-3 py-2 text-left">Purpose</th>
            <th class="px-3 py-2 text-left">Default</th>
          </tr>
        </thead>
        <tbody class="divide-y divide-base-300">
          <tr>
            <td class="px-3 py-2"
              ><code class="doc-inline px-1 py-0.5">LAZYPOCK_IMAGE_CONCURRENCY</code></td
            >
            <td class="px-3 py-2"
              >Resizes running at once per instance — requests wait briefly, then
              get a <code class="doc-inline px-1 py-0.5">503</code></td
            >
            <td class="px-3 py-2 font-mono text-xs">1</td>
          </tr>
          <tr>
            <td class="px-3 py-2"
              ><code class="doc-inline px-1 py-0.5"
                >LAZYPOCK_MAGICK_MEMORY_LIMIT</code
              ></td
            >
            <td class="px-3 py-2">Per-process ImageMagick memory cap</td>
            <td class="px-3 py-2 font-mono text-xs">256MiB</td>
          </tr>
          <tr>
            <td class="px-3 py-2"
              ><code class="doc-inline px-1 py-0.5">LAZYPOCK_VARIANT_CACHE_MAX</code></td
            >
            <td class="px-3 py-2">Local variant cache cap, oldest evicted first</td>
            <td class="px-3 py-2 font-mono text-xs">5GB</td>
          </tr>
          <tr>
            <td class="px-3 py-2"
              ><code class="doc-inline px-1 py-0.5">LAZYPOCK_IMAGE_ENGINE</code></td
            >
            <td class="px-3 py-2">Image engine (an unavailable engine fails loudly)</td>
            <td class="px-3 py-2 font-mono text-xs">magick</td>
          </tr>
        </tbody>
      </table>
    </div>
    <p class="mt-3 text-sm text-base-content/70">
      Which variants exist, their sizes, and whether they are generated during
      upload (eager) or on first request (lazy) is configured with
      <code class="doc-inline px-1 py-0.5">image.presets</code> — see
      <a class="text-primary underline" href="/files">File storage &amp; images</a>.
    </p>
  </section>

  <!-- ENV VARS -->
  <section id="env-vars" class="scroll-mt-20 mb-14">
    <h2 class="text-xl font-semibold border-b border-base-300 pb-2">
      Environment variables
    </h2>
    <div class="mt-3 overflow-x-auto rounded-box border border-base-300">
      <table class="w-full text-sm">
        <thead class="bg-base-200 text-left">
          <tr>
            <th class="px-3 py-2 font-semibold">Variable</th>
            <th class="px-3 py-2 font-semibold">Description</th>
            <th class="px-3 py-2 font-semibold">Example</th>
          </tr>
        </thead>
        <tbody class="divide-y divide-base-300">
          <tr>
            <td class="px-3 py-2"
              ><code class="doc-inline px-1 py-0.5">DATABASE_URL</code></td
            >
            <td class="px-3 py-2">PostgreSQL connection string</td>
            <td class="px-3 py-2 font-mono text-xs"
              >ecto://postgres:postgres@localhost:5432/lazypock_dev</td
            >
          </tr>
          <tr>
            <td class="px-3 py-2"
              ><code class="doc-inline px-1 py-0.5">SECRET_KEY_BASE</code></td
            >
            <td class="px-3 py-2">Secret for signing cookies</td>
            <td class="px-3 py-2 font-mono text-xs">mix phx.gen.secret</td>
          </tr>
          <tr>
            <td class="px-3 py-2"
              ><code class="doc-inline px-1 py-0.5">PHX_HOST</code></td
            >
            <td class="px-3 py-2"
              >Public hostname (optional, defaults to <code
                class="doc-inline px-1 py-0.5">example.com</code
              >)</td
            >
            <td class="px-3 py-2 font-mono text-xs">localhost</td>
          </tr>
          <tr>
            <td class="px-3 py-2"
              ><code class="doc-inline px-1 py-0.5">PORT</code></td
            >
            <td class="px-3 py-2"
              >HTTP port (optional, defaults to <code
                class="doc-inline px-1 py-0.5">4000</code
              >)</td
            >
            <td class="px-3 py-2 font-mono text-xs">4000</td>
          </tr>
          <tr>
            <td class="px-3 py-2"
              ><code class="doc-inline px-1 py-0.5">POOL_SIZE</code></td
            >
            <td class="px-3 py-2"
              >DB connection pool size (optional, defaults to <code
                class="doc-inline px-1 py-0.5">10</code
              >)</td
            >
            <td class="px-3 py-2 font-mono text-xs">10</td>
          </tr>
          <tr>
            <td class="px-3 py-2"
              ><code class="doc-inline px-1 py-0.5"
                >LAZYPOCK_SUPERUSER_EMAIL</code
              ></td
            >
            <td class="px-3 py-2">Auto-create superuser on boot</td>
            <td class="px-3 py-2 font-mono text-xs">admin@lazypock.app</td>
          </tr>
          <tr>
            <td class="px-3 py-2"
              ><code class="doc-inline px-1 py-0.5"
                >LAZYPOCK_SUPERUSER_PASSWORD</code
              ></td
            >
            <td class="px-3 py-2">Auto-create superuser on boot</td>
            <td class="px-3 py-2 font-mono text-xs">your-password</td>
          </tr>
          <tr>
            <td class="px-3 py-2"
              ><code class="doc-inline px-1 py-0.5">LAZYPOCK_THUMBNAILS</code
              ></td
            >
            <td class="px-3 py-2"
              >Set to <code class="doc-inline px-1 py-0.5">0</code> to disable thumbnail/scaling
              generation</td
            >
            <td class="px-3 py-2 font-mono text-xs">0</td>
          </tr>
          <tr>
            <td class="px-3 py-2"
              ><code class="doc-inline px-1 py-0.5">LAZYPOCK_IMAGE_CONCURRENCY</code></td
            >
            <td class="px-3 py-2"
              >Concurrent image resizes per instance (requests wait, then get
              <code class="doc-inline px-1 py-0.5">503</code>)</td
            >
            <td class="px-3 py-2 font-mono text-xs">1</td>
          </tr>
          <tr>
            <td class="px-3 py-2"
              ><code class="doc-inline px-1 py-0.5">LAZYPOCK_UPLOAD_MAX_MB</code></td
            >
            <td class="px-3 py-2"
              >Request-body cap for
              <code class="doc-inline px-1 py-0.5">POST /api/files</code> (other routes stay at
              8 MB)</td
            >
            <td class="px-3 py-2 font-mono text-xs">20</td>
          </tr>
          <tr>
            <td class="px-3 py-2"
              ><code class="doc-inline px-1 py-0.5">LAZYPOCK_MAGICK_MEMORY_LIMIT</code></td
            >
            <td class="px-3 py-2">Per-process ImageMagick memory cap</td>
            <td class="px-3 py-2 font-mono text-xs">256MiB</td>
          </tr>
          <tr>
            <td class="px-3 py-2"
              ><code class="doc-inline px-1 py-0.5">LAZYPOCK_VARIANT_CACHE_MAX</code></td
            >
            <td class="px-3 py-2"
              >Local variant cache cap — oldest files are evicted first</td
            >
            <td class="px-3 py-2 font-mono text-xs">5GB</td>
          </tr>
          <tr>
            <td class="px-3 py-2"
              ><code class="doc-inline px-1 py-0.5">LAZYPOCK_S3_*</code></td
            >
            <td class="px-3 py-2"
              >S3/R2 backend: <code class="doc-inline px-1 py-0.5">ENDPOINT</code>,
              <code class="doc-inline px-1 py-0.5">BUCKET</code>,
              <code class="doc-inline px-1 py-0.5">REGION</code>,
              <code class="doc-inline px-1 py-0.5">ACCESS_KEY</code>,
              <code class="doc-inline px-1 py-0.5">SECRET</code>,
              <code class="doc-inline px-1 py-0.5">PREFIX</code>,
              <code class="doc-inline px-1 py-0.5">PUBLIC_URL</code> — override and lock the matching
              Studio fields. See <a href="/files">File storage &amp; images</a></td
            >
            <td class="px-3 py-2 font-mono text-xs">—</td>
          </tr>
          <tr>
            <td class="px-3 py-2"
              ><code class="doc-inline px-1 py-0.5">LAZYPOCK_DATA_DIR</code></td
            >
            <td class="px-3 py-2"
              >Base data dir for migrations/hooks/seeds (default: <code
                class="doc-inline px-1 py-0.5">~/.lazypock</code
              >)</td
            >
            <td class="px-3 py-2 font-mono text-xs">/data/lazypock</td>
          </tr>
          <tr>
            <td class="px-3 py-2"
              ><code class="doc-inline px-1 py-0.5"
                >LAZYPOCK_MIGRATIONS_DIR</code
              ></td
            >
            <td class="px-3 py-2"
              >Directory for migrations (default: <code
                class="doc-inline px-1 py-0.5">~/.lazypock/migrations</code
              >)</td
            >
            <td class="px-3 py-2 font-mono text-xs"
              >/data/lazypock/migrations</td
            >
          </tr>
          <tr>
            <td class="px-3 py-2"
              ><code class="doc-inline px-1 py-0.5">LAZYPOCK_AUTOMIGRATE</code
              ></td
            >
            <td class="px-3 py-2"
              >Set to <code class="doc-inline px-1 py-0.5">0</code> to disable
              auto-migrate on boot (then use
              <code class="doc-inline px-1 py-0.5">lazypock migrate</code>)</td
            >
            <td class="px-3 py-2 font-mono text-xs">0</td>
          </tr>
          <tr>
            <td class="px-3 py-2"
              ><code class="doc-inline px-1 py-0.5">LAZYPOCK_AUTOSEED</code></td
            >
            <td class="px-3 py-2"
              >Set to <code class="doc-inline px-1 py-0.5">0</code> to disable boot-time
              seeding</td
            >
            <td class="px-3 py-2 font-mono text-xs">0</td>
          </tr>
          <tr>
            <td class="px-3 py-2"
              ><code class="doc-inline px-1 py-0.5">LAZYPOCK_HOOKS_DIR</code
              ></td
            >
            <td class="px-3 py-2"
              >Directory for user hooks (default: <code
                class="doc-inline px-1 py-0.5">~/.lazypock/hooks</code
              >)</td
            >
            <td class="px-3 py-2 font-mono text-xs">/data/lazypock/hooks</td>
          </tr>
          <tr>
            <td class="px-3 py-2"
              ><code class="doc-inline px-1 py-0.5">LAZYPOCK_SEEDS_FILE</code
              ></td
            >
            <td class="px-3 py-2"
              >Seed file path (default: <code class="doc-inline px-1 py-0.5"
                >~/.lazypock/seeds.exs</code
              >)</td
            >
            <td class="px-3 py-2 font-mono text-xs">/data/lazypock/seeds.exs</td
            >
          </tr>
          <tr>
            <td class="px-3 py-2"
              ><code class="doc-inline px-1 py-0.5">LAZYPOCK_AUTH_TOKEN_TTL</code
              ></td
            >
            <td class="px-3 py-2"
              >Access-token lifetime in seconds (optional, defaults to <code
                class="doc-inline px-1 py-0.5">604800</code
              > = 7 days)</td
            >
            <td class="px-3 py-2 font-mono text-xs">3600</td>
          </tr>
          <tr>
            <td class="px-3 py-2"
              ><code class="doc-inline px-1 py-0.5"
                >LAZYPOCK_AUTH_TOKEN_SECRET</code
              ></td
            >
            <td class="px-3 py-2"
              >Dedicated signing secret for auth tokens (optional; defaults to
              <code class="doc-inline px-1 py-0.5">SECRET_KEY_BASE</code>)</td
            >
            <td class="px-3 py-2 font-mono text-xs">openssl rand -base64 48</td>
          </tr>
        </tbody>
      </table>
    </div>
    <p class="mt-3 text-base-content/80 leading-relaxed">
      Migrations and hooks live in <strong
        >user-writable directories on disk</strong
      >
      (<code class="doc-inline px-1 py-0.5">~/.lazypock/migrations</code>,
      <code class="doc-inline px-1 py-0.5">~/.lazypock/hooks</code>) rather than
      inside the binary — bundled defaults are copied there on first boot and
      applied automatically, and you can drop in new
      <code class="doc-inline px-1 py-0.5">.exs</code> migration files or Elixir
      hook modules after a release without rebuilding. See
      <code class="doc-inline px-1 py-0.5">lazypock migrate</code>
      /
      <code class="doc-inline px-1 py-0.5">lazypock migrations</code> /
      <code class="doc-inline px-1 py-0.5">lazypock seed</code> in the
      <a
        class="text-primary underline"
        href="https://github.com/gnuzd/lazypock#migrations-pocketbase-style"
        target="_blank"
        rel="noreferrer">lazypock README</a
      >
      for the full CLI.
    </p>
  </section>
</div>
