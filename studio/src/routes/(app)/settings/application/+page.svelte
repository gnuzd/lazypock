<script lang="ts">
	import { client } from '$lib/client';
	import { onMount } from 'svelte';
	import Button from '$lib/components/Button.svelte';
	import Input from '$lib/components/Input.svelte';

	let appName = $state('');
	let appSaving = $state(false);
	let appSaved = $state(false);
	let corsOrigins = $state('');
	let corsSaving = $state(false);
	let corsSaved = $state(false);

	onMount(async () => {
		try {
			const res = (await client.http.get('/settings')) as Record<string, unknown> | null;
			if (!res) return;
			appName = (res.app_name as string) ?? '';
			const origins = res.cors_origins;
			corsOrigins = Array.isArray(origins) ? origins.join(', ') : ((origins as string) ?? '');
		} catch {
			// not configured
		}
	});

	async function saveApp() {
		appSaving = true;
		appSaved = false;
		try {
			await client.http.patch('/settings', { app_name: appName || null });
			appSaved = true;
			setTimeout(() => (appSaved = false), 2000);
		} catch {
			// ignore
		} finally {
			appSaving = false;
		}
	}

	async function saveCors() {
		corsSaving = true;
		corsSaved = false;
		try {
			const list = corsOrigins
				.split(',')
				.map((s) => s.trim())
				.filter((s) => s.length > 0);
			await client.http.patch('/settings', { cors_origins: list });
			// Force the CORS cache to refresh immediately.
			await client.http.post('/settings/refresh-cors', {}).catch(() => {});
			corsSaved = true;
			setTimeout(() => (corsSaved = false), 2000);
		} catch {
			// ignore
		} finally {
			corsSaving = false;
		}
	}
</script>

<h2 class="mb-4 text-lg font-semibold">Application Settings</h2>
<div class="rounded-box border border-base-300 bg-base-100 p-6">
	<Input
		label="App Name"
		placeholder="Lazypock"
		bind:value={appName}
		help="Displayed in the admin UI header."
	/>
	<div class="mt-4 flex items-center gap-3">
		<Button class="btn-primary" loading={appSaving} disabled={appSaving} onclick={saveApp}
			>Save</Button
		>
		{#if appSaved}<span class="text-xs text-success">Saved!</span>{/if}
	</div>
</div>

<div class="mt-6 rounded-box border border-base-300 bg-base-100 p-6">
	<h3 class="mb-2 text-base font-semibold">Allowed Origins (CORS)</h3>
	<p class="mb-3 text-xs text-base-content/70">
		Comma-separated origins allowed to call the API and use the realtime socket. The app's own
		origin and <code>LAZYPOCK_CORS_ORIGINS</code> env are always included. Changes apply without a restart.
	</p>
	<Input
		label="Origins"
		placeholder="http://localhost:1420, https://app.example.com"
		bind:value={corsOrigins}
		help="Example: http://localhost:1420"
	/>
	<div class="mt-4 flex items-center gap-3">
		<Button class="btn-primary" loading={corsSaving} disabled={corsSaving} onclick={saveCors}
			>Save</Button
		>
		{#if corsSaved}<span class="text-xs text-success">Saved!</span>{/if}
	</div>
</div>
