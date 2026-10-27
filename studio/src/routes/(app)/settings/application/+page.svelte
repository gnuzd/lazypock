<script lang="ts">
	import { client } from '$lib/client';
	import { onMount } from 'svelte';
	import { z } from 'zod';
	import Button from '$lib/components/Button.svelte';
	import Input from '$lib/components/Input.svelte';
	import { createForm } from '$lib/createForm.svelte';
	import Tabs from '$lib/components/Tabs.svelte';
	import Select from '$lib/components/Select.svelte';
	import { toast } from 'svelte-sonner';

	const appSchema = z.object({ appName: z.string().trim() });
	const corsSchema = z.object({ origins: z.string().trim() });
	const realtimeSchema = z.object({ realtime: z.boolean() });
	/** The shapes the stored switch can be in: a boolean, or `{ enabled }`. */
	const realtimeSettingSchema = z.union([
		z.boolean(),
		z.object({ enabled: z.boolean().optional() })
	]);

	/**
	 * Read the instance-wide switch out of the stored setting.
	 *
	 * Unset means on — what every collection did before the switch existed — and
	 * the server also accepts `{ enabled: … }`, so all three shapes are read here.
	 */
	function realtimeFromSetting(setting: unknown): boolean {
		const parsed = realtimeSettingSchema.safeParse(setting);
		if (!parsed.success) return true;
		return typeof parsed.data === 'boolean' ? parsed.data : (parsed.data.enabled ?? true);
	}

	let appForm = $state(createForm(appSchema, { appName: '' }));
	let corsForm = $state(createForm(corsSchema, { origins: '' }));
	let realtimeForm = $state(createForm(realtimeSchema, { realtime: true }));
	let appSaved = $state(false);
	let corsSaved = $state(false);
	let realtimeSaved = $state(false);
	let activeTab = $state('General');

	onMount(async () => {
		try {
			const res = (await client.http.get('/settings')) as Record<string, unknown> | null;
			if (!res) return;
			appForm.form.appName = (res.app_name as string) ?? '';
			const origins = res.cors_origins;
			corsForm.form.origins = Array.isArray(origins)
				? origins.join(', ')
				: ((origins as string) ?? '');
			// Unset means on: that is what every collection did before the switch existed.
			realtimeForm.form.realtime = realtimeFromSetting(res.realtime);
		} catch {
			// not configured
		}
	});

	async function saveApp(data: { appName: string }) {
		try {
			await client.http.patch('/settings', { app_name: data.appName || null });
			toast.success('Saved');
		} catch {
			toast.error("Can't save. Please try again later.");
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

	async function saveRealtime(data: { realtime: boolean }) {
		realtimeSaved = false;
		try {
			await client.http.patch('/settings', { realtime: data.realtime });
			realtimeSaved = true;
			setTimeout(() => (realtimeSaved = false), 2000);
		} catch {
			toast.error("Can't save. Please try again later.");
		}
	}
</script>

<h2 class="mb-4 text-lg font-semibold">Application Settings</h2>

<Tabs bind:active={activeTab} items={['General', 'Cors', 'Realtime']} />

{#if activeTab === 'General'}
	<form
		class="mt-6 rounded-box border border-base-300 bg-base-100 p-6"
		use:appForm.enhance={saveApp}
	>
		<Input
			label="App Name"
			placeholder="Lazypock"
			bind:value={appForm.form.appName}
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
{:else if activeTab === 'Cors'}
	<form
		class="mt-6 rounded-box border border-base-300 bg-base-100 p-6"
		use:corsForm.enhance={saveCors}
	>
		<h3 class="mb-2 text-base font-semibold">Allowed Origins (CORS)</h3>
		<p class="mb-3 text-xs text-base-content/70">
			Comma-separated origins allowed to call the API and use the realtime socket. The app's own
			origin and <code>LAZYPOCK_CORS_ORIGINS</code> env are always included. Changes apply without a restart.
		</p>
		<Input
			label="Origins"
			placeholder="http://localhost:1420, https://app.example.com"
			bind:value={corsForm.form.origins}
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
{:else}
	<form
		class="mt-6 rounded-box border border-base-300 bg-base-100 p-6"
		use:realtimeForm.enhance={saveRealtime}
	>
		<h3 class="mb-2 text-base font-semibold">Realtime change events</h3>
		<p class="mb-4 text-xs text-base-content/70">
			Every collection publishes its writes as realtime events. Turning this off <strong
				>drops the change-event trigger on every collection</strong
			>, so no write produces an event — including writes made directly in the database, which never
			go through the server — and no write pays for one. It takes effect immediately, for existing
			collections as well as new ones. A single collection can opt out in its own settings.
		</p>
		<div class="field max-w-xs">
			<span class="field-label">Publish change events</span>
			<Select
				variant="plain"
				triggerClass="w-full py-2.5"
				options={[
					{ value: true, label: 'On (default)' },
					{ value: false, label: 'Off' }
				]}
				bind:value={realtimeForm.form.realtime}
			/>
		</div>
		<div class="mt-4 flex items-center gap-3">
			<Button
				class="btn-primary"
				loading={realtimeForm.submitting}
				disabled={realtimeForm.submitting}
				type="submit">Save</Button
			>
			{#if realtimeSaved}<span class="text-xs text-success">Saved!</span>{/if}
		</div>
	</form>
{/if}
