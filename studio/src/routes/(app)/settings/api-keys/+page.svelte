<script lang="ts">
	import { client } from '$lib/client';
	import { onMount } from 'svelte';
	import Button from '$lib/components/Button.svelte';
	import '../settings.css';

	let hasApiKey = $state(false);
	let freshlyGenerated = $state('');
	let busy = $state(false);

	onMount(async () => {
		try {
			const res = (await client.http.get('/settings/api-key')) as {
				has_api_key?: boolean;
			} | null;
			if (!res) return;
			hasApiKey = res.has_api_key ?? false;
		} catch {
			// not configured / unauth
		}
	});

	async function generate() {
		busy = true;
		try {
			const res = (await client.http.post('/settings/api-key', {})) as { api_key?: string } | null;
			if (res?.api_key) {
				freshlyGenerated = res.api_key;
				hasApiKey = true;
				navigator.clipboard?.writeText(res.api_key).catch(() => {});
			}
		} catch {
			// ignore
		} finally {
			busy = false;
		}
	}

	async function revoke() {
		busy = true;
		try {
			await client.http.delete('/settings/api-key');
			hasApiKey = false;
			freshlyGenerated = '';
		} catch {
			// ignore
		} finally {
			busy = false;
		}
	}
</script>

<h2 class="mb-4 text-lg font-semibold">API Keys</h2>
<div class="rounded-box border border-base-300 bg-base-100 p-6">
	<div class="mb-4 text-sm text-base-content/60">
		<p>
			API keys let you authenticate programmatic access to Lazypock without a browser session — for
			example running the TypeScript codegen CLI (<code>npx lazypock --url ... --apikey ...</code
			>).
		</p>
		<p class="mt-1">
			Keys are stored as a SHA-256 hash (the raw key is shown only once, when generated) and are
			scoped to listing your collection schemas so the codegen CLI can fetch them without a login.
			Treat them like passwords.
		</p>
	</div>

	{#if freshlyGenerated}
		<div class="mb-4 rounded border border-success/40 bg-success/10 p-4 text-sm">
			<div class="mb-2 font-semibold text-success">
				Your new API key — copy it now, it won't be shown again.
			</div>
			<div class="flex items-center gap-2">
				<code class="rounded bg-base-200 px-3 py-2 break-all select-all">{freshlyGenerated}</code>
				<Button
					class="btn-outline btn-sm"
					onclick={() => navigator.clipboard?.writeText(freshlyGenerated)}
				>
					Copy
				</Button>
			</div>
		</div>
	{:else if hasApiKey}
		<div
			class="mb-4 flex items-center justify-between rounded border border-base-300 bg-base-200/50 p-4"
		>
			<div>
				<div class="text-xs text-base-content/50">A current key is configured</div>
				<div class="text-sm text-base-content/60">
					Raw key is not stored — regenerate to replace it.
				</div>
			</div>
			<Button class="btn-outline btn-sm" onclick={generate}>Regenerate</Button>
		</div>
	{:else}
		<div class="mb-4 text-sm text-base-content/50">No API key generated yet.</div>
	{/if}

	<div class="mt-4 flex items-center gap-3">
		{#if !hasApiKey || freshlyGenerated}
			<Button class="btn-primary" loading={busy} onclick={generate}>Generate API key</Button>
		{/if}
		{#if hasApiKey && !freshlyGenerated}
			<Button class="btn-error btn-outline" loading={busy} onclick={revoke}>Revoke</Button>
		{/if}
	</div>
</div>
