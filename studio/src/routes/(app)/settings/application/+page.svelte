<script lang="ts">
	import { onMount } from 'svelte';
	import { toast } from 'svelte-sonner';

	import { z } from 'zod';
	import { Info, Save, Shield, SlidersHorizontal, Zap } from '@lucide/svelte';

	import { client } from '$lib/client';
	import { createForm } from '$lib/createForm.svelte';
	import Button from '$lib/components/Button.svelte';
	import Input from '$lib/components/Input.svelte';
	import Select from '$lib/components/Select.svelte';
	import SettingsCard from '$lib/components/SettingsCard.svelte';
	import Tabs from '$lib/components/Tabs.svelte';

	import '../settings.css';

	const schema = z.object({
		appName: z.string().trim(),
		origins: z.string().trim(),
		realtime: z.boolean()
	});

	/** The shapes the stored switch can be in: a boolean, or `{ enabled }`. */
	const realtimeSettingSchema = z.union([
		z.boolean(),
		z.object({ enabled: z.boolean().optional() })
	]);

	/**
	 * Read the instance-wide switch out of the stored setting.
	 *
	 * Unset means on — what every collection did before the switch existed — and
	 * the server also accepts `{ enabled: ... }`, so all three shapes are read here.
	 */
	function realtimeFromSetting(setting: unknown): boolean {
		const parsed = realtimeSettingSchema.safeParse(setting);
		if (!parsed.success) return true;
		return typeof parsed.data === 'boolean' ? parsed.data : (parsed.data.enabled ?? true);
	}

	let form = $state(createForm(schema, { appName: '', origins: '', realtime: true }));
	let saving = $state(false);

	onMount(async () => {
		try {
			const res = (await client.http.get('/settings')) as Record<string, unknown> | null;
			if (!res) return;
			form.form.appName = (res.app_name as string) ?? '';
			const origins = res.cors_origins;
			form.form.origins = Array.isArray(origins) ? origins.join(', ') : ((origins as string) ?? '');
			form.form.realtime = realtimeFromSetting(res.realtime);
		} catch {
			// not configured
		}
	});

	async function save() {
		if (!form.validateAll()) return;
		saving = true;
		try {
			const { appName, origins, realtime } = form.form;
			await client.http.patch('/settings', {
				app_name: appName || null,
				cors_origins: origins
					.split(',')
					.map((s) => s.trim())
					.filter((s) => s.length > 0),
				realtime
			});
			// Force the CORS cache to refresh immediately.
			await client.http.post('/settings/refresh-cors', {}).catch(() => {});
			toast.success('Saved');
		} catch {
			toast.error("Can't save. Please try again later.");
		} finally {
			saving = false;
		}
	}
</script>

<div class="flex flex-col gap-4">
	<header class="flex flex-wrap items-start justify-between gap-3 border-b border-base-300 pb-4">
		<div>
			<h1 class="text-xl font-semibold">System Settings</h1>
			<p class="mt-1 text-sm text-base-content/50">
				Application-wide configuration for this instance.
			</p>
		</div>
		<Button
			class="btn-primary btn-sm gap-1.5"
			loading={saving}
			disabled={saving}
			onclick={() => void save()}
		>
			<Save size={15} />
			Save changes
		</Button>
	</header>

	<Tabs
		active="application"
		class="mb-3"
		items={[{ icon: SlidersHorizontal, label: 'Application Settings', key: 'application' }]}
	/>

	<section class="flex flex-col gap-4">
		<div class="grid gap-4 xl:grid-cols-3">
			<SettingsCard title="General Info" badge="Basic" icon={Info}>
				<Input
					label="App Name"
					placeholder="Lazypock"
					bind:value={form.form.appName}
					help="Displayed in the admin UI header."
				/>
			</SettingsCard>

			<SettingsCard
				title="Allowed Origins (CORS)"
				badge="Security"
				badgeVariant="warning"
				icon={Shield}
			>
				<div class="field">
					<label class="field-label" for="cors-origins">Origins</label>
					<textarea
						id="cors-origins"
						rows={3}
						placeholder="http://localhost:1420, https://example.com"
						class="w-full resize-y border-0 bg-transparent px-3 pb-2.5 text-sm leading-6 text-base-content outline-none placeholder:text-base-content/35"
						bind:value={form.form.origins}></textarea>
				</div>
				<p class="text-xs text-base-content/50">
					Comma-separated list of allowed origins. Include the scheme and the port, no path.
				</p>
			</SettingsCard>

			<SettingsCard title="Realtime Events" badge="Pubsub" badgeVariant="info" icon={Zap}>
				<div class="field">
					<span class="field-label">Publish Change Events</span>
					<Select
						variant="plain"
						triggerClass="w-full py-2.5"
						options={[
							{ value: true, label: 'On (data, auth)' },
							{ value: false, label: 'Off' }
						]}
						bind:value={form.form.realtime}
					/>
				</div>
				<p class="text-xs text-base-content/50">
					Broadcast database changes over SSE. Turning it off drops the change-event trigger on
					every collection.
				</p>
			</SettingsCard>
		</div>
	</section>
</div>
