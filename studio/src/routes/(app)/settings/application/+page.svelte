<script lang="ts">
	import { client } from '$lib/client';
	import { onMount } from 'svelte';
	import { z } from 'zod';
	import Button from '$lib/components/Button.svelte';
	import Input from '$lib/components/Input.svelte';
	import { createForm } from '$lib/createForm.svelte';

	const appSchema = z.object({ appName: z.string().trim() });
	const corsSchema = z.object({ origins: z.string().trim() });

	let appForm = $state(createForm(appSchema, { appName: '' }));
	let corsForm = $state(createForm(corsSchema, { origins: '' }));
	let appSaved = $state(false);
	let corsSaved = $state(false);

	onMount(async () => {
		try {
			const res = (await client.http.get('/settings')) as Record<string, unknown> | null;
			if (!res) return;
			appForm.values.appName = (res.app_name as string) ?? '';
			const origins = res.cors_origins;
			corsForm.values.origins = Array.isArray(origins)
				? origins.join(', ')
				: ((origins as string) ?? '');
		} catch {
			// not configured
		}
	});

	async function saveApp(data: { appName: string }) {
		appSaved = false;
		try {
			await client.http.patch('/settings', { app_name: data.appName || null });
			appSaved = true;
			setTimeout(() => (appSaved = false), 2000);
		} catch {
			// ignore
		}
	}

	async function saveCors(data: { origins: string }) {
		corsSaved = false;
		try {
			const list = data.origins
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
		}
	}
</script>

<h2 class="mb-4 text-lg font-semibold">Application Settings</h2>
<form
	class="rounded-box border border-base-300 bg-base-100 p-6"
	onsubmit={(e) => appForm.handleSubmit(e, saveApp)}
>
	<Input
		label="App Name"
		placeholder="Lazypock"
		bind:value={appForm.values.appName}
		help="Displayed in the admin UI header."
	/>
	<div class="mt-4 flex items-center gap-3">
		<Button
			class="btn-primary"
			loading={appForm.submitting}
			disabled={appForm.submitting}
			type="submit">Save</Button
		>
		{#if appSaved}<span class="text-xs text-success">Saved!</span>{/if}
	</div>
</form>

<form
	class="mt-6 rounded-box border border-base-300 bg-base-100 p-6"
	onsubmit={(e) => corsForm.handleSubmit(e, saveCors)}
>
	<h3 class="mb-2 text-base font-semibold">Allowed Origins (CORS)</h3>
	<p class="mb-3 text-xs text-base-content/70">
		Comma-separated origins allowed to call the API and use the realtime socket. The app's own
		origin and <code>LAZYPOCK_CORS_ORIGINS</code> env are always included. Changes apply without a restart.
	</p>
	<Input
		label="Origins"
		placeholder="http://localhost:1420, https://app.example.com"
		bind:value={corsForm.values.origins}
		help="Example: http://localhost:1420"
		error={corsForm.errors.origins}
	/>
	<div class="mt-4 flex items-center gap-3">
		<Button
			class="btn-primary"
			loading={corsForm.submitting}
			disabled={corsForm.submitting}
			type="submit">Save</Button
		>
		{#if corsSaved}<span class="text-xs text-success">Saved!</span>{/if}
	</div>
</form>
