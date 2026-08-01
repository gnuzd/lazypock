<script lang="ts">
	import { client } from '$lib/client';
	import { onMount } from 'svelte';
	import Button from '$lib/components/Button.svelte';
	import Input from '$lib/components/Input.svelte';

	let appName = $state('');
	let appSaving = $state(false);
	let appSaved = $state(false);

	onMount(async () => {
		try {
			const res = (await client.http.get('/settings')) as Record<string, unknown> | null;
			if (!res) return;
			appName = (res.app_name as string) ?? '';
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
