<script lang="ts">
	import CodeBlock from '$lib/components/CodeBlock.svelte';
</script>

<svelte:head>
	<title>Lazypock-TS Docs</title>
</svelte:head>

<div class="prose-doc max-w-3xl">
	<!-- OVERVIEW -->
	<section id="overview" class="scroll-mt-20 mb-14">
		<div class="flex flex-wrap items-center gap-2">
			<span class="rounded-field bg-info/10 text-info px-2 py-0.5 text-xs font-medium">SDK · TypeScript</span>
			<span class="rounded-field bg-success/10 text-success px-2 py-0.5 text-xs font-medium">Backend, Studio &amp; SDK: stable</span>
		</div>
		<h1 class="mt-3 text-3xl font-bold tracking-tight">Lazypock — TypeScript SDK</h1>
		<p class="mt-3 text-base-content/80 leading-relaxed">
			TypeScript client library for <a class="text-primary underline" href="https://github.com/gnuzd/lazypock" target="_blank" rel="noreferrer">Lazypock</a>,
			an open-source, PocketBase-compatible backend. This guide covers both how to run a Lazypock
			<strong>server</strong> and how to use the <code class="doc-inline px-1.5 py-0.5 text-[13px]">lazypock</code>
			(package <code class="doc-inline px-1.5 py-0.5 text-[13px]">lazypock-ts</code>) client in your app.
		</p>
	</section>

	<!-- INSTALLATION -->
	<section id="installation" class="scroll-mt-20 mb-14">
		<h2 class="text-xl font-semibold border-b border-base-300 pb-2">Installation</h2>
		<CodeBlock lang="bash" code={`npm install lazypock`} />
		<p class="text-sm text-base-content/70">bun / pnpm / yarn work the same way.</p>
	</section>

	<!-- QUICK START -->
	<section id="quick-start" class="scroll-mt-20 mb-14">
		<h2 class="text-xl font-semibold border-b border-base-300 pb-2">Quick Start</h2>
		<CodeBlock
			lang="typescript" code={`import { LazypockClient } from 'lazypock';

const client = new LazypockClient({ baseUrl: 'http://localhost:4000/api' });

// Superuser login
await client.login('admin@example.com', 'password');

// Or auth collection login
await client.login('user@example.com', 'password', 'users');
// Or using the explicit method:
await client.authWithPassword('users', 'user@example.com', 'password');

// List records
const posts = await client.collection('posts').getList(1, 30);
// or fetch all pages:
const all = await client.collection('posts').getFullList();

// Create a record
const newPost = await client.collection('posts').create({ title: 'Hello', published: true });

// File upload
const file = await client.files.upload(fileInput.files[0]);

// Real-time subscriptions (PocketBase-style: callback-first)
client.collection('posts').subscribe((e) => console.log(e.action, e.record));`}
		/>
	</section>

	<!-- SERVER SETUP -->
	<section id="prerequisites" class="scroll-mt-20 mb-8">
		<h2 class="text-xl font-semibold border-b border-base-300 pb-2">Server Setup</h2>
		<p class="mt-3 text-base-content/80 leading-relaxed">
			Lazypock is a monorepo with three parts: the <strong>core</strong> backend (Elixir + Phoenix +
			PostgreSQL), the <strong>Studio</strong> admin UI (SvelteKit), and the <strong>TypeScript SDK</strong>
			(this package, kept in a separate repo). To use <code class="doc-inline px-1 py-0.5">lazypock-ts</code>
			against a real server, you first need a running backend.
		</p>

		<h3 class="mt-6 text-lg font-semibold">Prerequisites</h3>
		<ul class="mt-3 space-y-2 text-base-content/80 list-disc list-inside">
			<li><strong>Elixir 1.17+</strong> and <strong>Erlang/OTP 26+</strong></li>
			<li><strong>PostgreSQL 15+</strong></li>
			<li><strong>Node.js 20+</strong> (for the Studio admin UI and the SDK)</li>
			<li>
				<strong>ImageMagick 7+</strong> (<code class="doc-inline px-1 py-0.5">magick</code>/<code class="doc-inline px-1 py-0.5">convert</code>)
				— required for image thumbnails and on-demand scaling; uploads still work without it, thumbnails just
				won't be generated
			</li>
			<li><code class="doc-inline px-1 py-0.5">zig</code> and <code class="doc-inline px-1 py-0.5">xz</code> — only needed for Burrito production release builds</li>
		</ul>
	</section>

	<section id="docker-quickstart" class="scroll-mt-20 mb-10">
		<h3 class="text-lg font-semibold">Option A: Docker Compose (quickest)</h3>
		<p class="mt-2 text-base-content/80 leading-relaxed">
			No Elixir, Erlang, or source checkout needed — just Docker (for Postgres) and a prebuilt binary. Clone the
			repo and start Postgres with its
			<code class="doc-inline px-1 py-0.5">docker-compose.yml</code> (Postgres 16:
			<code class="doc-inline px-1 py-0.5">postgres/postgres@localhost:5432</code>, database
			<code class="doc-inline px-1 py-0.5">lazypock_dev</code>):
		</p>
		<CodeBlock lang="bash" code={`# 1. Start Postgres (the repo's docker-compose.yml runs Postgres 16:
#    postgres/postgres@localhost:5432, database lazypock_dev)
docker compose up -d`} />
		<p class="mt-3 text-base-content/80 leading-relaxed">
			2. Grab the prebuilt binary for your platform from
			<a class="text-primary underline" href="https://github.com/gnuzd/lazypock/releases" target="_blank" rel="noreferrer">Releases</a>
			(macOS arm64 + Linux x86_64; checksums included):
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
			<a class="text-primary underline" href="#run-backend">Option C</a> below.
		</p>
		<p class="mt-3 text-base-content/80 leading-relaxed">
			Point <code class="doc-inline px-1 py-0.5">lazypock-ts</code> at it right away:
		</p>
		<CodeBlock
			lang="typescript" code={`import { LazypockClient } from 'lazypock';

const client = new LazypockClient({ baseUrl: 'http://localhost:4000/api' });
await client.login('admin@lazypock.app', 'admin123');

// after creating a \`posts\` collection in the Studio
const posts = await client.collection('posts').getList(1, 30);
client.collection('posts').subscribe((e) => console.log(e.action, e.record));`}
		/>
	</section>


	<section id="download-binary" class="scroll-mt-20 mb-10">
		<h3 class="text-lg font-semibold">Option B: Download a prebuilt binary</h3>
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
export PHX_SERVER=true
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

	<section id="run-backend" class="scroll-mt-20 mb-10">
		<h3 class="text-lg font-semibold">Option C: Manual setup — 1. Run the backend (Phoenix)</h3>
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
	</section>

	<section id="run-studio" class="scroll-mt-20 mb-10">
		<h3 class="text-lg font-semibold">2. Run Studio (Admin UI)</h3>
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
	</section>

	<section id="run-sdk" class="scroll-mt-20 mb-10">
		<h3 class="text-lg font-semibold">3. Install the TypeScript SDK</h3>
		<p class="mt-2 text-base-content/80 leading-relaxed">In your own app:</p>
		<CodeBlock lang="bash" code={`npm install lazypock`} />
		<p class="mt-3 text-base-content/80 leading-relaxed">Or, to build the SDK from source:</p>
		<CodeBlock
			lang="bash" code={`git clone git@github.com:gnuzd/lazypock-ts.git
cd lazypock-ts
npm install
npm run build`}
		/>
	</section>

	<section id="first-time-setup" class="scroll-mt-20 mb-10">
		<h3 class="text-lg font-semibold">First-time setup</h3>
		<ol class="mt-3 list-decimal list-inside space-y-1 text-base-content/80">
			<li>Open Studio at <code class="doc-inline px-1 py-0.5">http://localhost:5173/_/</code> (or <code class="doc-inline px-1 py-0.5">/_</code> if served directly from Phoenix)</li>
			<li>You'll be redirected to the login page</li>
			<li>Click <strong>Setup</strong> to create the first superuser account</li>
			<li>Log in and start creating collections — every collection you create in Studio gets an instant REST API + realtime channel + rules, ready to call from <code class="doc-inline px-1 py-0.5">lazypock-ts</code></li>
		</ol>
	</section>

	<section id="production" class="scroll-mt-20 mb-10">
		<h3 class="text-lg font-semibold">Production release (single binary)</h3>
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
export PHX_SERVER=true
export SECRET_KEY_BASE="$(mix phx.gen.secret)"
export PHX_HOST="localhost"
LAZYPOCK_SUPERUSER_EMAIL=admin@example.com LAZYPOCK_SUPERUSER_PASSWORD=changeme \\
  ./core/burrito_out/lazypock_macos_silicon`}
		/>
		<p class="mt-3 text-sm text-base-content/70">
			The release runs with <code class="doc-inline px-1 py-0.5">RUNTIME_CONFIG=false</code>, so config is baked
			in at build time; environment variables are still read at boot via the Elixir config provider.
		</p>
	</section>

	<section id="env-vars" class="scroll-mt-20 mb-14">
		<h3 class="text-lg font-semibold">Environment variables</h3>
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
						<td class="px-3 py-2"><code class="doc-inline px-1 py-0.5">PHX_SERVER</code></td>
						<td class="px-3 py-2">Enable the HTTP server (set to <code class="doc-inline px-1 py-0.5">true</code>)</td>
						<td class="px-3 py-2 font-mono text-xs">true</td>
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

	<!-- TYPE SAFETY -->
	<section id="type-safety" class="scroll-mt-20 mb-14">
		<h2 class="text-xl font-semibold border-b border-base-300 pb-2">Type Safety</h2>
		<p class="mt-3 text-base-content/80 leading-relaxed">
			The SDK offers three levels of type safety — pick what fits your project.
		</p>

		<h3 id="codegen" class="scroll-mt-20 mt-8 text-lg font-semibold">1. Fully typed via codegen (recommended)</h3>
		<p class="mt-2 text-base-content/80 leading-relaxed">
			Connect to your API once and generate a typed client — every collection becomes an interface with the
			exact field types from your schema (selects become string unions, relations become record IDs, etc.).
		</p>
		<CodeBlock
			lang="bash" code={`# In your app, after installing lazypock:
npx lazypock-gen \\
  --url http://localhost:4000/api \\
  --email admin@example.com \\
  --password your-password
# writes ./lazypock.types.ts`}
		/>
		<div class="rounded-box border border-base-300 bg-base-200/60 p-4 my-4 text-sm leading-relaxed">
			<p>
				<code class="doc-inline px-1.5 py-0.5">lazypock-gen</code> remains as a deprecated alias for
				backwards compatibility — the canonical command is now simply
				<code class="doc-inline px-1.5 py-0.5">lazypock</code>:
			</p>
			<CodeBlock
				lang="bash" code={`npx lazypock --url http://localhost:4000/api --email admin@example.com --password your-password`}
			/>
			<p class="mt-3">
				<strong>Use an API key instead of a password</strong> (recommended). Generate one from the Studio
				<em>Settings → API Keys</em> dashboard, then:
			</p>
			<CodeBlock
				lang="bash" code={`npx lazypock --url http://localhost:4000/api --apikey lazypock_xxxxxxxx
# or via env: LAZYPOCK_URL=... LAZYPOCK_API_KEY=... npx lazypock`}
			/>
			<p class="mt-3">
				API keys are stored as a SHA-256 hash (raw value shown once at generation) and are scoped to
				collection listing — ideal for codegen (they can <code class="doc-inline px-1.5 py-0.5">GET /collections</code>
				without a login round-trip, and cannot read or mutate your records).
			</p>
		</div>

		<p class="mt-4 text-base-content/80 leading-relaxed">Then in your app:</p>
		<CodeBlock
			lang="typescript" code={`import { createClient } from './lazypock.types';

const client = createClient({ baseUrl: 'http://localhost:4000/api' });
await client.login('admin@example.com', 'password');

// Collection access is fully type-checked:
const post = await client.collection('posts').getOne('abc123');
// post.title — string, post.published — boolean, …

await client.collection('posts').create({ title: 'x' });      // ✓
await client.collection('posts').create({ nope: 1 });          // ✗ compile error`}
		/>
		<div class="rounded-box border border-base-300 bg-base-200/60 p-4 my-4 text-sm leading-relaxed">
			<p>
				<strong>Dynamic collection names are fully supported.</strong> The typed client accepts any runtime
				string for <code class="doc-inline px-1.5 py-0.5">collection(name)</code> and still returns the
				typed service for known collection names. So route params and dynamic lookups work naturally:
			</p>
			<CodeBlock
				lang="typescript" code={`function load(name: string) {
  return client.collection(name).getList(); // ✓ works for any string
}`}
			/>
		</div>

		<h3 id="hand-written-generics" class="scroll-mt-20 mt-8 text-lg font-semibold">2. Hand-written generics (no codegen)</h3>
		<p class="mt-2 text-base-content/80 leading-relaxed">
			Pass a record interface to <code class="doc-inline px-1.5 py-0.5">collection&lt;T&gt;()</code> or use
			<code class="doc-inline px-1.5 py-0.5">.typed&lt;T&gt;()</code>:
		</p>
		<CodeBlock
			lang="typescript" code={`interface Post {
  id: string;
  title: string;
  published: boolean;
}

const postsSvc = client.collection('posts').typed<Post>();
const post = await postsSvc.getOne('abc123'); // post.title: string

await postsSvc.create({ title: 'Hi', published: true }); // ✓
await postsSvc.create({ title: 'Hi', nope: 1 });          // ✗ compile error`}
		/>

		<h3 id="runtime-schema" class="scroll-mt-20 mt-8 text-lg font-semibold">3. Runtime schema types (experimental)</h3>
		<p class="mt-2 text-base-content/80 leading-relaxed">Fetch schemas at runtime and let the client derive field types:</p>
		<CodeBlock
			lang="typescript" code={`const res = await fetch('http://localhost:4000/api/collections', {
  headers: { Authorization: 'Bearer ' + token },
});
const { items } = await res.json(); // CollectionSchema[]

const client = new LazypockClient({
  baseUrl: 'http://localhost:4000/api',
  types: { schemas: items },
});

const code = client.generateTypes(); // string — write to lazypock.types.ts`}
		/>
		<p class="mt-2 text-sm text-base-content/70">
			The codegen CLI emits a <code class="doc-inline px-1 py-0.5">lazypockSchema</code> snapshot next to the
			types, and the generated <code class="doc-inline px-1 py-0.5">createClient()</code> wires it in
			automatically — so the schema-driven behaviour below (hidden-field exclusion, query validation) works
			out of the box.
		</p>

		<h3 id="cli-reference" class="scroll-mt-20 mt-8 text-lg font-semibold">CLI reference</h3>
		<CodeBlock
			lang="bash" code={`lazypock [options]

Options:
  --url <url>        API base URL (or LAZYPOCK_URL)
  --apikey <key>    API key (or LAZYPOCK_API_KEY) — recommended, no login round-trip
  --api-key <key>   Deprecated alias for --apikey
  --email <email>    Superuser email (or LAZYPOCK_EMAIL)
  --password <pw>    Superuser password (or LAZYPOCK_PASSWORD)
  --output <file>   Output file (default: lazypock.types.ts)
  --out <file>      Deprecated alias for --output
  --package <name>   Package name to import (default: lazypock)
  --skip-system      Skip system collections`}
		/>
		<p class="mt-2 text-base-content/80 leading-relaxed">You must provide credentials one of two ways (or via the matching env vars):</p>
		<ol class="list-decimal list-inside mt-2 space-y-1 text-base-content/80">
			<li><code class="doc-inline px-1 py-0.5">--apikey</code> / <code class="doc-inline px-1 py-0.5">LAZYPOCK_API_KEY</code> — scoped to collection listing, no login.</li>
			<li><code class="doc-inline px-1 py-0.5">--email</code> + <code class="doc-inline px-1 py-0.5">--password</code> / matching env vars — superuser login.</li>
		</ol>
	</section>

	<!-- SELECT -->
	<section id="select" class="scroll-mt-20 mb-14">
		<h2 class="text-xl font-semibold border-b border-base-300 pb-2">Field projection (select) &amp; query suggestions</h2>
		<h3 class="mt-4 text-lg font-semibold">select(...) — pick the fields you want</h3>
		<p class="mt-2 text-base-content/80 leading-relaxed">
			<code class="doc-inline px-1 py-0.5">select()</code> projects list/read responses to the given fields
			(PocketBase <code class="doc-inline px-1 py-0.5">fields</code> param). Field names are
			<strong>type-checked</strong> when the service is typed:
		</p>
		<CodeBlock
			lang="typescript" code={`const t = await client.collection('posts').select('id', 'title').getList();
// GET /api/posts?fields=id,title

await client.collection('posts').select('id', 'title').getOne('abc123'); // same`}
		/>
		<ul class="mt-3 space-y-2 text-base-content/80 list-disc list-inside">
			<li><code class="doc-inline px-1 py-0.5">select('*')</code> (or no <code class="doc-inline px-1 py-0.5">select()</code> call) — request all visible fields; hidden fields are excluded automatically when a schema is available.</li>
			<li><code class="doc-inline px-1 py-0.5">select()</code> with no arguments resets back to the default.</li>
			<li><code class="doc-inline px-1 py-0.5">select()</code> returns a <strong>derived service</strong> — the original is untouched, so you can keep one default service and project per-request.</li>
			<li>Passing an explicit <code class="doc-inline px-1 py-0.5">fields</code> option overrides the <code class="doc-inline px-1 py-0.5">select()</code> preset.</li>
		</ul>
		<p class="mt-3 text-base-content/80 leading-relaxed">
			When a schema is known (via <code class="doc-inline px-1 py-0.5">types.schemas</code> or codegen), hidden
			fields are <strong>not returned by the server</strong>: every read sends
			<code class="doc-inline px-1 py-0.5">fields=&lt;visible fields&gt;</code> by default, and selecting an
			unknown field logs a warning.
		</p>

		<h3 id="filter-sort-expand" class="scroll-mt-20 mt-8 text-lg font-semibold">filter / sort / expand — type-checked suggestions</h3>
		<p class="mt-2 text-base-content/80 leading-relaxed">
			With a typed service, the query options validate field names (and filter operators) at compile time —
			your editor suggests valid fields as you type:
		</p>
		<CodeBlock
			lang="typescript" code={`await postsSvc.getList(1, 20, { sort: '-title' });        // ✓ suggests title/published/…
await postsSvc.getList(1, 20, { sort: '-nope' });         // ✗ compile error

await postsSvc.getList(1, 20, {
  filter: "title ~ 'x' && published = true", // ✓ field + operator checked
});
await postsSvc.getList(1, 20, { filter: 'nope = 1' });    // ✗ compile error

await postsSvc.getList(1, 20, { expand: 'author' });      // ✓ field suggested
await postsSvc.getOne('abc', { expand: 'author' });`}
		/>
		<ul class="mt-3 space-y-2 text-base-content/80 list-disc list-inside">
			<li><code class="doc-inline px-1 py-0.5">filter</code> — <code class="doc-inline px-1 py-0.5">field op value</code> clauses with <code class="doc-inline px-1 py-0.5">= != ~ !~ &gt; &gt;= &lt; &lt;=</code> operators; <code class="doc-inline px-1 py-0.5">&amp;&amp;</code>, <code class="doc-inline px-1 py-0.5">||</code>, <code class="doc-inline px-1 py-0.5">!</code>, and parentheses are allowed after the first clause.</li>
			<li><code class="doc-inline px-1 py-0.5">sort</code> — <code class="doc-inline px-1 py-0.5">field</code>, <code class="doc-inline px-1 py-0.5">-field</code> (desc), <code class="doc-inline px-1 py-0.5">+field</code>, or comma-separated.</li>
			<li><code class="doc-inline px-1 py-0.5">expand</code> — comma-separated relation field names; non-relation fields warn at runtime when a schema is available.</li>
			<li>The <strong>untyped</strong> client (<code class="doc-inline px-1 py-0.5">client.collection('posts')</code> without <code class="doc-inline px-1 py-0.5">typed&lt;T&gt;()</code>) still accepts any string — suggestions kick in once the service is typed.</li>
		</ul>
	</section>

	<!-- API REFERENCE -->
	<section id="client" class="scroll-mt-20 mb-8">
		<h2 class="text-xl font-semibold border-b border-base-300 pb-2">API Reference</h2>
		<h3 class="mt-4 text-lg font-semibold">LazypockClient</h3>
		<p class="mt-2 text-base-content/80 leading-relaxed">The main client class.</p>
	</section>

	<section id="constructor-options" class="scroll-mt-20 mb-10">
		<h3 class="text-lg font-semibold">Constructor Options</h3>
		<div class="mt-3 overflow-x-auto rounded-box border border-base-300">
			<table class="w-full text-sm">
				<thead class="bg-base-200 text-left">
					<tr>
						<th class="px-3 py-2 font-semibold">Option</th>
						<th class="px-3 py-2 font-semibold">Type</th>
						<th class="px-3 py-2 font-semibold">Default</th>
						<th class="px-3 py-2 font-semibold">Description</th>
					</tr>
				</thead>
				<tbody class="divide-y divide-base-300">
					<tr>
						<td class="px-3 py-2"><code class="doc-inline px-1 py-0.5">baseUrl</code></td>
						<td class="px-3 py-2 font-mono text-xs">string</td>
						<td class="px-3 py-2 text-base-content/60">required</td>
						<td class="px-3 py-2">API base URL (e.g. <code class="doc-inline px-1 py-0.5">http://localhost:4000/api</code>)</td>
					</tr>
					<tr>
						<td class="px-3 py-2"><code class="doc-inline px-1 py-0.5">storage</code></td>
						<td class="px-3 py-2 font-mono text-xs">StorageAdapter</td>
						<td class="px-3 py-2 text-base-content/60">memoryStorage</td>
						<td class="px-3 py-2">Custom storage adapter for token persistence</td>
					</tr>
					<tr>
						<td class="px-3 py-2"><code class="doc-inline px-1 py-0.5">authStore</code></td>
						<td class="px-3 py-2 font-mono text-xs">AuthStore</td>
						<td class="px-3 py-2 text-base-content/60">auto-created</td>
						<td class="px-3 py-2">Explicit auth store instance</td>
					</tr>
					<tr>
						<td class="px-3 py-2"><code class="doc-inline px-1 py-0.5">realtime</code></td>
						<td class="px-3 py-2 font-mono text-xs">RealtimeService</td>
						<td class="px-3 py-2 text-base-content/60">auto-created</td>
						<td class="px-3 py-2">Real-time service for WebSocket subscriptions</td>
					</tr>
				</tbody>
			</table>
		</div>
	</section>

	<section id="auth-methods" class="scroll-mt-20 mb-10">
		<h3 class="text-lg font-semibold">Authentication Methods</h3>
		<ul class="mt-3 space-y-2 text-base-content/80 list-disc list-inside">
			<li><code class="doc-inline px-1 py-0.5">login(email, password, collection?)</code> — Login as superuser or auth collection user</li>
			<li><code class="doc-inline px-1 py-0.5">authWithPassword(collection, identity, password, options?)</code> — Auth collection login</li>
			<li><code class="doc-inline px-1 py-0.5">authRefresh(collection, options?)</code> — Refresh auth token</li>
			<li><code class="doc-inline px-1 py-0.5">checkSuperuser()</code> — Check if any superuser exists</li>
			<li><code class="doc-inline px-1 py-0.5">setup(email, password)</code> — Create initial superuser</li>
			<li><code class="doc-inline px-1 py-0.5">logout()</code> — Clear auth state</li>
			<li><code class="doc-inline px-1 py-0.5">me(options?)</code> — Get current superuser profile</li>
		</ul>

		<h4 class="mt-6 text-sm font-semibold uppercase tracking-wide text-base-content/50">Auto-Cancellation Methods</h4>
		<ul class="mt-3 space-y-2 text-base-content/80 list-disc list-inside">
			<li><code class="doc-inline px-1 py-0.5">autoCancellation(enable)</code> — Globally enable/disable auto-cancellation of duplicated pending requests</li>
			<li><code class="doc-inline px-1 py-0.5">cancelRequest(requestKey)</code> — Abort a single pending request by key (default <code class="doc-inline px-1 py-0.5">HTTP_METHOD + path</code>)</li>
			<li><code class="doc-inline px-1 py-0.5">cancelAllRequests()</code> — Abort all pending requests</li>
		</ul>
	</section>

	<section id="collections-service" class="scroll-mt-20 mb-10">
		<h3 class="text-lg font-semibold">Collections Service (<code class="doc-inline px-1 py-0.5 text-base font-normal">client.collections</code>)</h3>
		<p class="mt-2 text-base-content/80 leading-relaxed">PocketBase-style service for the collections themselves (admin):</p>
		<ul class="mt-3 space-y-2 text-base-content/80 list-disc list-inside">
			<li><code class="doc-inline px-1 py-0.5">collections.getList(params?)</code> — Paginated list of collections</li>
			<li><code class="doc-inline px-1 py-0.5">collections.getFullList(options?)</code> — Fetch all collections (auto-paginates)</li>
			<li><code class="doc-inline px-1 py-0.5">collections.getOne(id, options?)</code> — Get collection by ID/name</li>
			<li><code class="doc-inline px-1 py-0.5">collections.create(data, options?)</code> — Create collection</li>
			<li><code class="doc-inline px-1 py-0.5">collections.update(id, data, options?)</code> — Update collection</li>
			<li><code class="doc-inline px-1 py-0.5">collections.delete(id, options?)</code> — Delete collection</li>
			<li><code class="doc-inline px-1 py-0.5">collections.subscribe(cb)</code> — Subscribe to collection create/update/delete events (returns unsubscribe fn)</li>
			<li><code class="doc-inline px-1 py-0.5">collections.unsubscribe()</code> — Unsubscribe from registry events</li>
		</ul>
	</section>

	<section id="file-operations" class="scroll-mt-20 mb-10">
		<h3 class="text-lg font-semibold">File Operations</h3>
		<ul class="mt-3 space-y-2 text-base-content/80 list-disc list-inside">
			<li><code class="doc-inline px-1 py-0.5">files.upload(file, filename?, options?, meta?)</code> — Upload a file</li>
			<li><code class="doc-inline px-1 py-0.5">files.getUrl(fileId)</code> — Get file metadata</li>
			<li><code class="doc-inline px-1 py-0.5">files.delete(fileId, options?)</code> — Delete a file</li>
			<li><code class="doc-inline px-1 py-0.5">getFileUrl(baseUrl, fileId)</code> — Construct a file URL from base URL and file ID (utility)</li>
		</ul>
	</section>

	<section id="realtime-api" class="scroll-mt-20 mb-10">
		<h3 class="text-lg font-semibold">Realtime</h3>
		<ul class="mt-3 space-y-2 text-base-content/80 list-disc list-inside">
			<li><code class="doc-inline px-1 py-0.5">realtime.connect(opts)</code> — Connect to WebSocket</li>
			<li><code class="doc-inline px-1 py-0.5">realtime.disconnect()</code> — Disconnect</li>
			<li><code class="doc-inline px-1 py-0.5">realtime.subscribe(topic, callback)</code> — Low-level subscribe (topic like <code class="doc-inline px-1 py-0.5">collection:posts</code>)</li>
			<li><code class="doc-inline px-1 py-0.5">realtime.unsubscribe(topic, callback?)</code> — Low-level unsubscribe</li>
			<li><code class="doc-inline px-1 py-0.5">collection(name).subscribe(callback, recordId?)</code> — Subscribe to record changes; callback receives <code class="doc-inline px-1 py-0.5">{'{'} action, record {'}'}</code>; returns unsubscribe fn</li>
			<li><code class="doc-inline px-1 py-0.5">collection(name).unsubscribe(recordId?)</code> — Unsubscribe from record changes</li>
		</ul>
	</section>

	<section id="collection-service" class="scroll-mt-20 mb-10">
		<h3 class="text-lg font-semibold">CollectionService</h3>
		<p class="mt-2 text-base-content/80 leading-relaxed">Returned by <code class="doc-inline px-1 py-0.5">client.collection(name)</code>.</p>
		<ul class="mt-3 space-y-2 text-base-content/80 list-disc list-inside">
			<li><code class="doc-inline px-1 py-0.5">select(...fields)</code> — Project reads to the given fields (see Field projection); <code class="doc-inline px-1 py-0.5">select('*')</code> restores the all-visible default</li>
			<li><code class="doc-inline px-1 py-0.5">getList(page, perPage, options?)</code> — Paginated list of records (typed <code class="doc-inline px-1 py-0.5">filter</code>/<code class="doc-inline px-1 py-0.5">sort</code>/<code class="doc-inline px-1 py-0.5">expand</code>/<code class="doc-inline px-1 py-0.5">fields</code>)</li>
			<li><code class="doc-inline px-1 py-0.5">getFullList(options?)</code> — Fetch all records (auto-paginates)</li>
			<li><code class="doc-inline px-1 py-0.5">getFirstListItem(filter, options?)</code> — Fetch first record matching filter</li>
			<li><code class="doc-inline px-1 py-0.5">getOne(id, options?)</code> — Get record by ID</li>
			<li><code class="doc-inline px-1 py-0.5">create(data, options?)</code> — Create record</li>
			<li><code class="doc-inline px-1 py-0.5">update(id, data, options?)</code> — Update record</li>
			<li><code class="doc-inline px-1 py-0.5">delete(id, options?)</code> — Delete record</li>
			<li><code class="doc-inline px-1 py-0.5">subscribe(callback, recordId?)</code> — Subscribe to record changes (PocketBase-style)</li>
			<li><code class="doc-inline px-1 py-0.5">unsubscribe(recordId?)</code> — Unsubscribe</li>
			<li><code class="doc-inline px-1 py-0.5">authWithPassword(identity, password, options?)</code> — Login to this auth collection</li>
			<li><code class="doc-inline px-1 py-0.5">authRefresh(options?)</code> — Refresh token for this auth collection</li>
			<li><code class="doc-inline px-1 py-0.5">authMethods(options?)</code> — Get available auth methods</li>
		</ul>
	</section>

	<section id="auth-store" class="scroll-mt-20 mb-10">
		<h3 class="text-lg font-semibold">AuthStore</h3>
		<p class="mt-2 text-base-content/80 leading-relaxed">Handles token persistence and auto-refresh.</p>
		<ul class="mt-3 space-y-2 text-base-content/80 list-disc list-inside">
			<li><code class="doc-inline px-1 py-0.5">token</code> — Current JWT token</li>
			<li><code class="doc-inline px-1 py-0.5">model</code> — Current auth model (user record or null)</li>
			<li><code class="doc-inline px-1 py-0.5">isValid</code> — Whether a token exists</li>
			<li><code class="doc-inline px-1 py-0.5">isExpired</code> — Whether the current token has expired (with 30s buffer)</li>
			<li><code class="doc-inline px-1 py-0.5">collectionName</code> — Name of the auth collection used for token refresh</li>
			<li><code class="doc-inline px-1 py-0.5">set(token, model)</code> — Update token and model</li>
			<li><code class="doc-inline px-1 py-0.5">setCollectionName(name)</code> — Set the auth collection name for token refresh</li>
			<li><code class="doc-inline px-1 py-0.5">clear()</code> — Clear all auth state</li>
			<li><code class="doc-inline px-1 py-0.5">onChange(callback)</code> — Listen for auth changes (returns unsubscribe function)</li>
			<li><code class="doc-inline px-1 py-0.5">init()</code> — Restore persisted auth from storage</li>
		</ul>
	</section>

	<section id="types" class="scroll-mt-20 mb-14">
		<h3 class="text-lg font-semibold">Types</h3>
		<CodeBlock
			lang="typescript" code={`interface ApiRecord {
  id: string;
  collectionId: string;
  collectionName: string;
  created: string;
  updated: string;
  [key: string]: unknown;
}

interface ListResult<T> {
  page: number;
  perPage: number;
  totalItems: number;
  totalPages: number;
  items: T[];
}

interface AuthModel {
  id: string;
  [key: string]: unknown;
}

interface FileRecord {
  id: string;
  filename: string;
  mimeType: string;
  size: number;
  url: string;
  [key: string]: unknown;
}

interface RequestOptions {
  signal?: AbortSignal;
  fetch?: typeof fetch;
  headers?: Record<string, string>;
}`}
		/>
	</section>

	<!-- AUTO CANCELLATION -->
	<section id="auto-cancellation" class="scroll-mt-20 mb-14">
		<h2 class="text-xl font-semibold border-b border-base-300 pb-2">Auto Cancellation</h2>
		<p class="mt-3 text-base-content/80 leading-relaxed">
			The SDK auto-cancels duplicated pending requests for you (PocketBase-compatible behaviour). When a new
			request is issued with the same request key as a still-pending request, the previous one is aborted —
			only the last request executes:
		</p>
		<CodeBlock
			lang="typescript" code={`// Only the last call will execute; the first two are auto-cancelled
await client.collection('posts').getList(1, 20); // cancelled
await client.collection('posts').getList(2, 20); // cancelled
await client.collection('posts').getList(3, 20); // executed`}
		/>
		<p class="mt-3 text-base-content/80 leading-relaxed">
			By default the request key is <code class="doc-inline px-1 py-0.5">HTTP_METHOD + path</code> (e.g.
			<code class="doc-inline px-1 py-0.5">"GET /api/posts?page=1"</code>), so duplicate calls with identical
			URLs cancel each other. Cancelled requests reject with an
			<code class="doc-inline px-1 py-0.5">ApiError</code> whose <code class="doc-inline px-1 py-0.5">isAbort</code>
			is <code class="doc-inline px-1 py-0.5">true</code>:
		</p>
		<CodeBlock
			lang="typescript" code={`try {
  await client.collection('posts').getList(1, 20);
} catch (err) {
  if (err instanceof ApiError && err.isAbort) {
    // superseded by a newer request — safe to ignore
  }
}`}
		/>

		<h3 class="mt-6 text-lg font-semibold">Per-request control</h3>
		<p class="mt-2 text-base-content/80 leading-relaxed">
			Pass <code class="doc-inline px-1 py-0.5">requestKey</code> in the request options to customize the key,
			or disable auto-cancellation for a specific request:
		</p>
		<CodeBlock
			lang="typescript" code={`await client.collection('posts').getList(1, 20, { requestKey: 'my-list' }); // cancelled
await client.collection('posts').getList(1, 20, { requestKey: 'my-list' }); // executed

await client.collection('posts').getList(1, 20, { requestKey: null });   // executed
await client.collection('posts').getList(1, 20, { requestKey: null });   // executed`}
		/>

		<h3 class="mt-6 text-lg font-semibold">Global control</h3>
		<CodeBlock
			lang="typescript" code={`// Disable auto-cancellation globally
client.autoCancellation(false);

// Manually cancel pending requests
client.cancelRequest('GET /api/posts?page=1');
client.cancelAllRequests();`}
		/>

		<h3 class="mt-6 text-lg font-semibold">Single-flight dedup (getFullList)</h3>
		<p class="mt-2 text-base-content/80 leading-relaxed">
			<code class="doc-inline px-1 py-0.5">getFullList()</code> (and
			<code class="doc-inline px-1 py-0.5">collections.getFullList()</code>) are <strong>single-flight</strong>:
			concurrent calls with the same effective options share one in-flight request instead of firing
			duplicates. This means the common pattern below results in <strong>one</strong> network request, and
			<strong>both</strong> callers resolve with the same data — no abort rejection:
		</p>
		<CodeBlock
			lang="typescript" code={`const [a, b] = await Promise.all([
  client.collection('posts').getFullList(),
  client.collection('posts').getFullList(),
]);
// one GET fired; a === b`}
		/>
		<p class="mt-3 text-base-content/80 leading-relaxed">
			Calls with <strong>different</strong> options (e.g. different <code class="doc-inline px-1 py-0.5">sort</code>/<code class="doc-inline px-1 py-0.5">filter</code>)
			are still distinct requests. Multi-page fetches continue to work normally — each page request is unique
			(page number is part of the URL), so pages never cancel each other.
		</p>
		<p class="mt-3 text-base-content/80 leading-relaxed">
			The underlying <code class="doc-inline px-1 py-0.5">singleFlight</code> option is also available on any
			request when you want to coalesce concurrent identical calls yourself:
		</p>
		<CodeBlock lang="typescript" code={`await client.collection('posts').getList(1, 20, { singleFlight: true });`} />
	</section>

	<!-- ERROR HANDLING -->
	<section id="error-handling" class="scroll-mt-20 mb-14">
		<h2 class="text-xl font-semibold border-b border-base-300 pb-2">Error Handling</h2>
		<p class="mt-3 text-base-content/80 leading-relaxed">
			The SDK throws <code class="doc-inline px-1 py-0.5">ApiError</code> on non-2xx responses:
		</p>
		<CodeBlock
			lang="typescript" code={`import { LazypockClient, ApiError } from 'lazypock';

try {
  await client.collection('posts').create({ title: 'My Post' });
} catch (err) {
  if (err instanceof ApiError) {
    console.log(err.status);    // HTTP status code
    console.log(err.message);   // Error message
    console.log(err.data);      // Full response data
  }
}`}
		/>
	</section>

	<!-- CONFIGURATION -->
	<section id="configuration" class="scroll-mt-20 mb-14">
		<h2 class="text-xl font-semibold border-b border-base-300 pb-2">Configuration</h2>

		<h3 class="mt-4 text-lg font-semibold">Storage Adapter</h3>
		<p class="mt-2 text-base-content/80 leading-relaxed">
			By default, the SDK uses <code class="doc-inline px-1 py-0.5">localStorage</code> for token persistence.
			You can provide a custom adapter:
		</p>
		<CodeBlock
			lang="typescript" code={`import { LazypockClient, AuthStore } from 'lazypock';

const customStorage = {
  get: async (key) => await AsyncStorage.getItem(key),
  set: async (key, value) => await AsyncStorage.setItem(key, value),
  remove: async (key) => await AsyncStorage.removeItem(key),
};

const client = new LazypockClient({
  baseUrl: 'http://localhost:4000/api',
  storage: customStorage,
});`}
		/>

		<h3 class="mt-6 text-lg font-semibold">Auto Token Refresh</h3>
		<p class="mt-2 text-base-content/80 leading-relaxed">
			The SDK automatically refreshes expired auth tokens. When a token expires, the next API call triggers a
			transparent refresh via the <code class="doc-inline px-1 py-0.5">auth-refresh</code> endpoint. No
			manual intervention needed.
		</p>
	</section>

	<!-- REALTIME SUBSCRIPTIONS -->
	<section id="realtime-subscriptions" class="scroll-mt-20 mb-14">
		<h2 class="text-xl font-semibold border-b border-base-300 pb-2">Real-time Subscriptions</h2>
		<CodeBlock
			lang="typescript" code={`// Subscribe to all changes in a collection (PocketBase-style: callback-first)
const off = client.collection('posts').subscribe((event) => {
  console.log(event.action); // 'create' | 'update' | 'delete'
  console.log(event.record);
});

// Subscribe to a specific record only
client.collection('posts').subscribe((event) => { /* ... */ }, 'abc123');

// Unsubscribe
client.collection('posts').unsubscribe();

// ...or call the returned unsubscribe function for one-shot listeners:
off();`}
		/>

		<h3 class="mt-6 text-lg font-semibold">Anonymous / rule-based realtime</h3>
		<p class="mt-2 text-base-content/80 leading-relaxed">
			Realtime subscriptions honor your API <strong>and list rules</strong> — matching PocketBase behavior.
			This means <strong>non-logged-in users can subscribe</strong> to collections whose list rules are public
			(empty <code class="doc-inline px-1 py-0.5">""</code> string) or anon-friendly
			(<code class="doc-inline px-1 py-0.5">@request.auth.*</code> filters). The SDK auto-connects the
			WebSocket on first use, so no token is required to receive public change events:
		</p>
		<CodeBlock
			lang="typescript" code={`// Works without logging in, as long as the collection's list rule allows it
const off = client.collection('public_feed').subscribe((e) => {
  console.log(e.action, e.record);
});`}
		/>
	</section>

	<!-- LICENSE -->
	<section id="license" class="scroll-mt-20 mb-24">
		<h2 class="text-xl font-semibold border-b border-base-300 pb-2">License</h2>
		<p class="mt-3 text-base-content/80 leading-relaxed">
			<a class="text-primary underline" href="https://github.com/gnuzd/lazypock-ts/blob/main/LICENSE" target="_blank" rel="noreferrer">MIT</a>
			© 2024-2025 Chris Nguyen (gnuzd)
		</p>
		<div class="mt-8 flex flex-wrap gap-3">
			<a
				class="rounded-field bg-primary text-primary-content px-4 py-2 text-sm font-medium hover:opacity-90"
				href="https://github.com/gnuzd/lazypock-ts"
				target="_blank"
				rel="noreferrer">View on GitHub — lazypock-ts</a
			>
			<a
				class="rounded-field border border-base-300 px-4 py-2 text-sm font-medium hover:bg-base-200"
				href="https://github.com/gnuzd/lazypock"
				target="_blank"
				rel="noreferrer">Backend — lazypock</a
			>
		</div>
	</section>
</div>
