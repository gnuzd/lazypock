<script lang="ts">
	import { client } from '$lib/client';
	import { onMount } from 'svelte';
	import { slide } from 'svelte/transition';
	import Button from '$lib/components/Button.svelte';
	import Input from '$lib/components/Input.svelte';
	import Switch from '$lib/components/Switch.svelte';
	import '../settings.css';

	type StorageView = {
		backend: 'local' | 's3';
		endpoint: string;
		region: string;
		bucket: string;
		access_key_id: string;
		secret_access_key: string;
		prefix: string;
		public_base_url: string;
		force_path_style: boolean;
		presign_ttl: number;
		secret_set?: boolean;
		configured_from_env?: string[];
	};

	type TestStep = { step: string; ok: boolean; error: string | null };

	let loading = $state(true);
	let saving = $state(false);
	let testing = $state(false);
	let error = $state('');
	let notice = $state('');
	let secretSet = $state(false);
	let envLocked = $state<string[]>([]);
	let steps = $state<TestStep[]>([]);
	/** Separate boolean because `backend` is a string ('local' | 's3'). */
	let useS3 = $state(false);

	let form = $state<StorageView>({
		backend: 'local',
		endpoint: '',
		region: 'auto',
		bucket: '',
		access_key_id: '',
		secret_access_key: '',
		prefix: '',
		public_base_url: '',
		force_path_style: true,
		presign_ttl: 900
	});

	function locked(field: string): boolean {
		return envLocked.includes(field);
	}

	function apply(view: StorageView | null) {
		if (!view) return;
		useS3 = view.backend === 's3';

		form = {
			backend: view.backend ?? 'local',
			endpoint: view.endpoint ?? '',
			region: view.region ?? 'auto',
			bucket: view.bucket ?? '',
			access_key_id: view.access_key_id ?? '',
			// Never echo the secret back; blank means "keep the stored one".
			secret_access_key: '',
			prefix: view.prefix ?? '',
			public_base_url: view.public_base_url ?? '',
			force_path_style: view.force_path_style ?? true,
			presign_ttl: view.presign_ttl ?? 900
		};
		secretSet = view.secret_set ?? false;
		envLocked = view.configured_from_env ?? [];
	}

	onMount(async () => {
		try {
			apply(await client.http.get<StorageView>('/settings/storage'));
		} catch (e) {
			error = (e as Error).message || 'Could not load the storage settings';
		} finally {
			loading = false;
		}
	});

	async function save() {
		saving = true;
		error = '';
		notice = '';
		steps = [];

			try {
			const payload: Record<string, unknown> = {
				backend: useS3 ? 's3' : 'local',
				endpoint: form.endpoint,
				region: form.region,
				bucket: form.bucket,
				access_key_id: form.access_key_id,
				prefix: form.prefix,
				public_base_url: form.public_base_url,
				force_path_style: form.force_path_style,
				presign_ttl: form.presign_ttl
			};

			// Only send the secret when the user actually typed a new one.
			if (form.secret_access_key.trim()) {
				payload.secret_access_key = form.secret_access_key.trim();
			}

			apply(await client.http.patch<StorageView>('/settings/storage', payload));
			notice = 'Storage settings saved.';
		} catch (e) {
			error = (e as Error).message || 'Could not save the storage settings';
		} finally {
			saving = false;
		}
	}

	async function testConnection() {
		testing = true;
		error = '';
		steps = [];

		try {
			const res = (await client.http.post('/settings/storage/test', {})) as {
				ok: boolean;
				steps: TestStep[];
			} | null;
			steps = res?.steps ?? [];
		} catch (e) {
			// A failing step list comes back with a 422 and the details, so read them.
			const body = (e as { data?: { steps?: TestStep[]; message?: string } }).data;
			steps = body?.steps ?? [];
			error = body?.message ?? (e as Error).message ?? 'Connection test failed';
		} finally {
			testing = false;
		}
	}
</script>

<h2 class="mb-1 text-lg font-semibold">Files Storage</h2>
<p class="mb-4 text-sm text-base-content/60">
	Uploads are stored on the local disk by default. Connect an S3-compatible bucket (AWS S3,
	Cloudflare R2, MinIO) to keep them off the server — the secret key is encrypted at rest and never
	returned by the API.
</p>

{#if loading}
	<p class="text-sm text-base-content/60">Loading…</p>
{:else}
	<form
		class="rounded-box border border-base-300 bg-base-100 p-6"
		onsubmit={(e) => {
			e.preventDefault();
			void save();
		}}
	>
		<Switch class="mb-4" bind:checked={useS3}>Use S3-compatible storage</Switch>

		{#if useS3}
			<div transition:slide={{ duration: 150 }}>
				<div class="mb-4 grid grid-cols-1 gap-3 sm:grid-cols-2">
					<Input
						label="Endpoint"
						placeholder="https://<account>.r2.cloudflarestorage.com"
						bind:value={form.endpoint}
						disabled={locked('endpoint')}
						help={locked('endpoint') ? 'Set by LAZYPOCK_S3_ENDPOINT' : ''}
						required
					/>
					<Input
						label="Bucket"
						placeholder="my-bucket"
						bind:value={form.bucket}
						disabled={locked('bucket')}
						help={locked('bucket') ? 'Set by LAZYPOCK_S3_BUCKET' : ''}
						required
					/>
				</div>

				<div class="mb-4 grid grid-cols-1 gap-3 sm:grid-cols-2">
					<Input
						label="Region"
						placeholder="auto"
						bind:value={form.region}
						disabled={locked('region')}
						help={locked('region') ? 'Set by LAZYPOCK_S3_REGION' : ''}
					/>
					<Input
						label="Key prefix"
						placeholder="lazypock/my-app/"
						bind:value={form.prefix}
						disabled={locked('prefix')}
						help="Objects are stored under <prefix><file id>/"
					/>
				</div>

				<div class="mb-4 grid grid-cols-1 gap-3 sm:grid-cols-2">
					<Input
						label="Access key id"
						bind:value={form.access_key_id}
						disabled={locked('access_key_id')}
						help={locked('access_key_id') ? 'Set by LAZYPOCK_S3_ACCESS_KEY' : ''}
						required
					/>
					<Input
						label="Secret access key"
						type="password"
						autocomplete="new-password"
						placeholder={secretSet ? '•••••••• (unchanged)' : ''}
						bind:value={form.secret_access_key}
						disabled={locked('secret_access_key')}
						help={locked('secret_access_key')
							? 'Set by LAZYPOCK_S3_SECRET'
							: secretSet
								? 'Leave blank to keep the stored secret'
								: ''}
						required={!secretSet}
					/>
				</div>

				<div class="mb-4 grid grid-cols-1 gap-3 sm:grid-cols-2">
					<Input
						label="Public base URL (optional)"
						placeholder="https://cdn.example.com"
						bind:value={form.public_base_url}
						disabled={locked('public_base_url')}
						help="Served URLs point here (CDN) instead of through the app"
					/>
					<Input
						label="Presign TTL (seconds)"
						type="number"
						value={String(form.presign_ttl)}
						oninput={(e) => (form.presign_ttl = Number((e.target as HTMLInputElement).value) || 900)}
						help="Lifetime of direct-upload and protected-file URLs"
					/>
				</div>

				<Switch bind:checked={form.force_path_style}>Force path style</Switch>
				<p class="mt-1 text-xs text-base-content/50">
					Leave on for R2 and MinIO; disable for virtual-hosted AWS S3.
				</p>
			</div>
		{/if}

		{#if error}
			<p class="mt-4 text-sm text-error">{error}</p>
		{/if}
		{#if notice}
			<p class="mt-4 text-sm text-success">{notice}</p>
		{/if}

		{#if steps.length > 0}
			<ul class="mt-4 flex flex-col gap-1 text-sm">
				{#each steps as step (step.step)}
					<li class:opacity-60={!step.ok}>
						<span class={step.ok ? 'text-success' : 'text-error'}>{step.ok ? '✓' : '✗'}</span>
						<strong class="ml-1 capitalize">{step.step}</strong>
						{#if step.error}<span class="ml-1 text-base-content/60">{step.error}</span>{/if}
					</li>
				{/each}
			</ul>
		{/if}

		<div class="mt-4 flex items-center justify-end gap-3">
			{#if useS3}
				<Button class="btn-sm" loading={testing} onclick={() => void testConnection()}>
					Test connection
				</Button>
			{/if}
			<Button class="btn-primary" loading={saving} type="submit">Save changes</Button>
		</div>
	</form>
{/if}
