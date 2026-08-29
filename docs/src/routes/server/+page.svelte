<script lang="ts">
	import CodeBlock from '$lib/components/CodeBlock.svelte';
</script>

<svelte:head>
	<title>Server Guide — Lazypock Docs</title>
</svelte:head>

<div class="prose-doc max-w-3xl">
	<!-- INTRO -->
	<section class="scroll-mt-20 mb-14">
		<div class="flex flex-wrap items-center gap-2">
			<span class="rounded-field bg-info/10 text-info px-2 py-0.5 text-xs font-medium">Server · Elixir + Phoenix + PostgreSQL</span>
		</div>
		<h1 class="mt-3 text-3xl font-bold tracking-tight">Server Guide</h1>
		<p class="mt-3 text-base-content/80 leading-relaxed">
			How to run and operate a Lazypock server. The repo is a monorepo with two parts: the
			<strong>core</strong> backend (Elixir + Phoenix + PostgreSQL) and the <strong>Studio</strong>
			admin UI (SvelteKit, served by the backend at <code class="doc-inline px-1 py-0.5">/_/</code>).
			Your app talks to the server through one of the
			<a class="text-primary underline" href="/sdk">SDKs</a> — no backend code required.
		</p>

		<h2 class="mt-8 text-xl font-semibold border-b border-base-300 pb-2">Prerequisites</h2>
		<ul class="mt-3 space-y-2 text-base-content/80 list-disc list-inside">
			<li><strong>Elixir 1.17+</strong> and <strong>Erlang/OTP 26+</strong> (only for running from source)</li>
			<li><strong>PostgreSQL 15+</strong></li>
			<li><strong>Node.js 20+</strong> (only for the Studio dev server and the SDKs)</li>
			<li>
				<strong>ImageMagick 7+</strong> (<code class="doc-inline px-1 py-0.5">magick</code>/<code class="doc-inline px-1 py-0.5">convert</code>)
				— required for image thumbnails and on-demand scaling; uploads still work without it, thumbnails just
				won't be generated
			</li>
			<li><code class="doc-inline px-1 py-0.5">zig</code> and <code class="doc-inline px-1 py-0.5">xz</code> — only needed for Burrito production release builds</li>
		</ul>
		<p class="mt-3 text-sm text-base-content/70">
			The Docker quick start below skips the Elixir/Node prerequisites entirely — just Docker and a prebuilt binary.
		</p>
	</section>

	<!-- QUICK START (DOCKER) -->
	<section id="quick-start" class="scroll-mt-20 mb-10">
		<h2 class="text-xl font-semibold border-b border-base-300 pb-2">Quick start — Docker Compose</h2>
		<p class="mt-3 text-base-content/80 leading-relaxed">
			No Elixir, Erlang, or source checkout needed — just Docker (for Postgres) and a prebuilt binary. Clone the
			repo and start Postgres with its <code class="doc-inline px-1 py-0.5">docker-compose.yml</code> (Postgres 16:
			<code class="doc-inline px-1 py-0.5">postgres/postgres@localhost:5432</code>, database
			<code class="doc-inline px-1 py-0.5">lazypock_dev</code>):
		</p>
		<CodeBlock lang="bash" code={`# 1. Start Postgres (the repo's docker-compose.yml runs Postgres 16:
#    postgres/postgres@localhost:5432, database lazypock_dev)
docker compose up -d`} />
		<p class="mt-3 text-base-content/80 leading-relaxed">
			2. Grab the prebuilt binary for your platform from
			<a class="text-primary underline" href="https://github.com/gnuzd/lazypock/releases" target="_blank" rel="noreferrer">Releases</a>
			(macOS arm64 + Linux x86_64; checksums included) — or see
			<a class="text-primary underline" href="#binary">Option B</a> below:
		</p>
		<CodeBlock lang="bash" code={`# 2. Grab the prebuilt binary for your platform from Releases:
#    https://github.com/gnuzd/lazypock/releases
#    (macOS arm64 + Linux x86_64; checksums included)`} />
		<p class="mt-3 text-base-content/80 leading-relaxed">
			3. Run it — the superuser is auto-created on first boot:
		</p>
		<CodeBlock
			lang="bash" code={`DATABASE_URL="ecto://postgres:postgres@localhost:5432/lazypock_dev" \\
SECRET_KEY_BASE="$(openssl rand -base64 48)" \\
LAZYPOCK_SUPERUSER_EMAIL=admin@lazypock.app \\
LAZYPOCK_SUPERUSER_PASSWORD=admin123 \\
  ./lazypock`}
		/>
		<div class="rounded-box border border-base-300 bg-base-200/60 p-4 my-4 text-sm leading-relaxed">
			<ul class="space-y-1.5 list-disc list-inside">
				<li>Server + Studio admin UI: <code class="doc-inline px-1 py-0.5">http://localhost:4000</code> (login at <code class="doc-inline px-1 py-0.5">/_/</code> with the superuser above)</li>
				<li>REST API: <code class="doc-inline px-1 py-0.5">http://localhost:4000/api/...</code></li>
			</ul>
		</div>
		<p class="mt-3 text-base-content/80 leading-relaxed">
			To reset everything (including the database):
		</p>
		<CodeBlock lang="bash" code={`docker compose down -v`} />
		<p class="mt-3 text-base-content/80 leading-relaxed">
			Prefer no Docker at all? Any PostgreSQL 15+ works — just point
			<code class="doc-inline px-1 py-0.5">DATABASE_URL</code> at it. Or run from source — see
			<a class="text-primary underline" href="#manual">Manual setup</a> below.
		</p>
		<p class="mt-3 text-base-content/80 leading-relaxed">
			Ready to connect an app? Follow the
			<a class="text-primary underline" href="/sdk/typescript/quick-start">TypeScript SDK quick start</a>.
		</p>
	</section>

	<!-- PREBUILT BINARY -->
	<section id="binary" class="scroll-mt-20 mb-10">
		<h2 class="text-xl font-semibold border-b border-base-300 pb-2">Prebuilt binary</h2>
		<p class="mt-2 text-base-content/80 leading-relaxed">
			No Docker, no Elixir toolchain — grab a prebuilt single-binary release (built with Burrito) straight
			from GitHub Releases and run it directly:
		</p>
		<CodeBlock
			lang="bash" code={`# Download the latest release for your platform from:
# https://github.com/gnuzd/lazypock/releases/latest

chmod +x lazypock_macos_silicon   # or the binary matching your OS/arch

export DATABASE_URL="ecto://postgres:postgres@localhost:5432/lazypock"
export SECRET_KEY_BASE="$(openssl rand -base64 48)"
LAZYPOCK_SUPERUSER_EMAIL=admin@example.com LAZYPOCK_SUPERUSER_PASSWORD=changeme \\
  ./lazypock_macos_silicon`}
		/>
		<p class="mt-3 text-sm text-base-content/70">
			You still need a reachable PostgreSQL 15+ instance (e.g. via
			<code class="doc-inline px-1 py-0.5">docker run -p 5432:5432 postgres:16-alpine</code>). The binary
			handles migrations, seeding, and serving the Studio UI on its own — see
			<a class="text-primary underline" href="https://github.com/gnuzd/lazypock/releases" target="_blank" rel="noreferrer">Releases</a>
			for available platforms and checksums.
		</p>
	</section>

	<!-- MANUAL -->
	<section id="manual" class="scroll-mt-20 mb-10">
		<h2 class="text-xl font-semibold border-b border-base-300 pb-2">Manual setup (from source)</h2>

		<h3 class="mt-6 text-lg font-semibold">1. Run the backend (Phoenix)</h3>
		<CodeBlock
			lang="bash" code={`git clone git@github.com:gnuzd/lazypock.git
cd lazypock/core

export DATABASE_URL="ecto://postgres:postgres@localhost:5432/lazypock_dev"
mix setup          # install deps, create DB, run migrations, seed
mix phx.server      # starts Phoenix on http://localhost:4000`}
		/>
		<p class="mt-2 text-sm text-base-content/70">
			The REST API and realtime channels are served at <code class="doc-inline px-1 py-0.5">http://localhost:4000</code>.
		</p>

		<h3 class="mt-6 text-lg font-semibold">2. Run Studio (Admin UI)</h3>
		<CodeBlock
			lang="bash" code={`cd lazypock/studio

npm install
npm run dev          # starts Vite dev server on http://localhost:5173`}
		/>
		<p class="mt-2 text-base-content/80 leading-relaxed">
			The SvelteKit dev server proxies <code class="doc-inline px-1 py-0.5">/api</code> requests to the Phoenix
			backend on port 4000. Studio itself is served at
			<code class="doc-inline px-1 py-0.5">http://localhost:5173/_/</code>.
		</p>

		<p class="mt-4 text-base-content/80 leading-relaxed">
			Then connect an app with one of the
			<a class="text-primary underline" href="/sdk">SDKs</a> — see the
			<a class="text-primary underline" href="/sdk/typescript/install">TypeScript SDK install</a>.
		</p>
	</section>

	<!-- FIRST-TIME SETUP -->
	<section id="first-time" class="scroll-mt-20 mb-10">
		<h2 class="text-xl font-semibold border-b border-base-300 pb-2">First-time setup</h2>
		<ol class="mt-3 list-decimal list-inside space-y-1 text-base-content/80">
			<li>Open Studio at <code class="doc-inline px-1 py-0.5">http://localhost:5173/_/</code> (or <code class="doc-inline px-1 py-0.5">/_</code> if served directly from Phoenix)</li>
			<li>You'll be redirected to the login page</li>
			<li>Click <strong>Setup</strong> to create the first superuser account</li>
			<li>Log in and start creating collections — every collection you create in Studio gets an instant REST API + realtime channel + rules, ready to call from any SDK</li>
		</ol>
	</section>

	<!-- PRODUCTION -->
	<section id="production" class="scroll-mt-20 mb-10">
		<h2 class="text-xl font-semibold border-b border-base-300 pb-2">Production release (single binary)</h2>
		<p class="mt-2 text-base-content/80 leading-relaxed">
			Lazypock ships as a single binary via <a class="text-primary underline" href="https://github.com/burrito-elixir/burrito" target="_blank" rel="noreferrer">Burrito</a>:
		</p>
		<CodeBlock
			lang="bash" code={`cd core
MIX_ENV=prod mix release
# Binary: core/burrito_out/lazypock_macos_silicon`}
		/>
		<p class="mt-3 text-base-content/80 leading-relaxed">Minimal production run example:</p>
		<CodeBlock
			lang="bash" code={`export DATABASE_URL="ecto://postgres:postgres@localhost:5432/lazypock"
export SECRET_KEY_BASE="$(mix phx.gen.secret)"
export PHX_HOST="localhost"
LAZYPOCK_SUPERUSER_EMAIL=admin@example.com LAZYPOCK_SUPERUSER_PASSWORD=changeme \\
  ./core/burrito_out/lazypock_macos_silicon`}
		/>
		<p class="mt-3 text-sm text-base-content/70">
			The HTTP server is <strong>always started</strong> — no <code class="doc-inline px-1 py-0.5">PHX_SERVER</code>
			needed; just run the binary (or <code class="doc-inline px-1 py-0.5">bin/lazypock start</code>).
		</p>
		<p class="mt-3 text-sm text-base-content/70">
			The release runs with <code class="doc-inline px-1 py-0.5">RUNTIME_CONFIG=false</code>, so config is baked
			in at build time; environment variables are still read at boot via the Elixir config provider.
		</p>
	</section>

	<!-- ENV VARS -->
	<section id="env-vars" class="scroll-mt-20 mb-14">
		<h2 class="text-xl font-semibold border-b border-base-300 pb-2">Environment variables</h2>
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
						<td class="px-3 py-2"><code class="doc-inline px-1 py-0.5">DATABASE_URL</code></td>
						<td class="px-3 py-2">PostgreSQL connection string</td>
						<td class="px-3 py-2 font-mono text-xs">ecto://postgres:postgres@localhost:5432/lazypock_dev</td>
					</tr>
					<tr>
						<td class="px-3 py-2"><code class="doc-inline px-1 py-0.5">SECRET_KEY_BASE</code></td>
						<td class="px-3 py-2">Secret for signing cookies</td>
						<td class="px-3 py-2 font-mono text-xs">mix phx.gen.secret</td>
					</tr>
					<tr>
						<td class="px-3 py-2"><code class="doc-inline px-1 py-0.5">PHX_HOST</code></td>
						<td class="px-3 py-2">Public hostname (optional, defaults to <code class="doc-inline px-1 py-0.5">example.com</code>)</td>
						<td class="px-3 py-2 font-mono text-xs">localhost</td>
					</tr>
					<tr>
						<td class="px-3 py-2"><code class="doc-inline px-1 py-0.5">PORT</code></td>
						<td class="px-3 py-2">HTTP port (optional, defaults to <code class="doc-inline px-1 py-0.5">4000</code>)</td>
						<td class="px-3 py-2 font-mono text-xs">4000</td>
					</tr>
					<tr>
						<td class="px-3 py-2"><code class="doc-inline px-1 py-0.5">POOL_SIZE</code></td>
						<td class="px-3 py-2">DB connection pool size (optional, defaults to <code class="doc-inline px-1 py-0.5">10</code>)</td>
						<td class="px-3 py-2 font-mono text-xs">10</td>
					</tr>
					<tr>
						<td class="px-3 py-2"><code class="doc-inline px-1 py-0.5">LAZYPOCK_SUPERUSER_EMAIL</code></td>
						<td class="px-3 py-2">Auto-create superuser on boot</td>
						<td class="px-3 py-2 font-mono text-xs">admin@lazypock.app</td>
					</tr>
					<tr>
						<td class="px-3 py-2"><code class="doc-inline px-1 py-0.5">LAZYPOCK_SUPERUSER_PASSWORD</code></td>
						<td class="px-3 py-2">Auto-create superuser on boot</td>
						<td class="px-3 py-2 font-mono text-xs">your-password</td>
					</tr>
					<tr>
						<td class="px-3 py-2"><code class="doc-inline px-1 py-0.5">LAZYPOCK_THUMBNAILS</code></td>
						<td class="px-3 py-2">Set to <code class="doc-inline px-1 py-0.5">0</code> to disable thumbnail/scaling generation</td>
						<td class="px-3 py-2 font-mono text-xs">0</td>
					</tr>
					<tr>
						<td class="px-3 py-2"><code class="doc-inline px-1 py-0.5">LAZYPOCK_DATA_DIR</code></td>
						<td class="px-3 py-2">Base data dir for migrations/hooks/seeds (default: <code class="doc-inline px-1 py-0.5">~/.lazypock</code>)</td>
						<td class="px-3 py-2 font-mono text-xs">/data/lazypock</td>
					</tr>
					<tr>
						<td class="px-3 py-2"><code class="doc-inline px-1 py-0.5">LAZYPOCK_MIGRATIONS_DIR</code></td>
						<td class="px-3 py-2">Directory for migrations (default: <code class="doc-inline px-1 py-0.5">~/.lazypock/migrations</code>)</td>
						<td class="px-3 py-2 font-mono text-xs">/data/lazypock/migrations</td>
					</tr>
					<tr>
						<td class="px-3 py-2"><code class="doc-inline px-1 py-0.5">LAZYPOCK_AUTOMIGRATE</code></td>
						<td class="px-3 py-2">Set to <code class="doc-inline px-1 py-0.5">0</code> to disable auto-migrate on boot (then use <code class="doc-inline px-1 py-0.5">lazypock migrate</code>)</td>
						<td class="px-3 py-2 font-mono text-xs">0</td>
					</tr>
					<tr>
						<td class="px-3 py-2"><code class="doc-inline px-1 py-0.5">LAZYPOCK_AUTOSEED</code></td>
						<td class="px-3 py-2">Set to <code class="doc-inline px-1 py-0.5">0</code> to disable boot-time seeding</td>
						<td class="px-3 py-2 font-mono text-xs">0</td>
					</tr>
					<tr>
						<td class="px-3 py-2"><code class="doc-inline px-1 py-0.5">LAZYPOCK_HOOKS_DIR</code></td>
						<td class="px-3 py-2">Directory for user hooks (default: <code class="doc-inline px-1 py-0.5">~/.lazypock/hooks</code>)</td>
						<td class="px-3 py-2 font-mono text-xs">/data/lazypock/hooks</td>
					</tr>
					<tr>
						<td class="px-3 py-2"><code class="doc-inline px-1 py-0.5">LAZYPOCK_SEEDS_FILE</code></td>
						<td class="px-3 py-2">Seed file path (default: <code class="doc-inline px-1 py-0.5">~/.lazypock/seeds.exs</code>)</td>
						<td class="px-3 py-2 font-mono text-xs">/data/lazypock/seeds.exs</td>
					</tr>
				</tbody>
			</table>
		</div>
		<p class="mt-3 text-base-content/80 leading-relaxed">
			Migrations and hooks live in <strong>user-writable directories on disk</strong>
			(<code class="doc-inline px-1 py-0.5">~/.lazypock/migrations</code>,
			<code class="doc-inline px-1 py-0.5">~/.lazypock/hooks</code>) rather than inside the binary — bundled
			defaults are copied there on first boot and applied automatically, and you can drop in new
			<code class="doc-inline px-1 py-0.5">.exs</code> migration files or Elixir hook modules after a release
			without rebuilding. See <code class="doc-inline px-1 py-0.5">lazypock migrate</code> /
			<code class="doc-inline px-1 py-0.5">lazypock migrations</code> /
			<code class="doc-inline px-1 py-0.5">lazypock seed</code> in the
			<a class="text-primary underline" href="https://github.com/gnuzd/lazypock#migrations-pocketbase-style" target="_blank" rel="noreferrer">lazypock README</a>
			for the full CLI.
		</p>
	</section>
</div>
