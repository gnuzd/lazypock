<script lang="ts">
	import { client } from '$lib/client';
	import { onMount } from 'svelte';
	import { slide } from 'svelte/transition';
	import Button from '$lib/components/Button.svelte';
	import Input from '$lib/components/Input.svelte';
	import '../settings.css';

	let storageEnabled = $state(false);
	let s3Bucket = $state('');
	let s3Region = $state('us-east-1');
	let s3AccessKey = $state('');
	let s3SecretKey = $state('');
	let s3Endpoint = $state('');
	let storageSaving = $state(false);

	onMount(async () => {
		try {
			const res = (await client.http.get('/settings')) as Record<string, unknown> | null;
			if (!res) return;
			storageEnabled = (res.s3_enabled as boolean) ?? false;
			const s3 = (res.s3 as Record<string, string>) ?? {};
			s3Bucket = s3.bucket ?? '';
			s3Region = s3.region ?? 'us-east-1';
			s3AccessKey = s3.access_key ?? '';
			s3SecretKey = s3.secret_key ?? '';
			s3Endpoint = s3.endpoint ?? '';
		} catch {
			// not configured
		}
	});

	async function saveStorage() {
		storageSaving = true;
		try {
			await client.http.patch('/settings', {
				s3_enabled: storageEnabled,
				s3: storageEnabled
					? {
							bucket: s3Bucket,
							region: s3Region,
							access_key: s3AccessKey,
							secret_key: s3SecretKey,
							endpoint: s3Endpoint
						}
					: null
			});
		} catch {
			// ignore
		} finally {
			storageSaving = false;
		}
	}
</script>

<h2 class="mb-4 text-lg font-semibold">Files Storage</h2>
<div class="rounded-box border border-base-300 bg-base-100 p-6">
	<div class="mb-4 text-sm text-base-content/60">
		<p>By default Lazypock uses the local file system to store uploaded files.</p>
		<p>If you have limited disk space, you could optionally connect to an S3 compatible storage.</p>
	</div>

	<!-- S3 toggle -->
	<div class="switch-field mb-4">
		<label class="switch-label" for="storage-enabled">
			<span class="txt">Use S3 storage</span>
		</label>
		<label class="switch">
			<input id="storage-enabled" type="checkbox" bind:checked={storageEnabled} />
			<span class="switch-slider"></span>
		</label>
	</div>

	{#if storageEnabled}
		<div transition:slide={{ duration: 150 }}>
			<div class="flex flex-col gap-3">
				<div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
					<Input label="S3 Bucket" placeholder="my-bucket" bind:value={s3Bucket} required />
					<Input label="Region" placeholder="us-east-1" bind:value={s3Region} />
				</div>
				<div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
					<Input label="Access Key" bind:value={s3AccessKey} />
					<Input label="Secret Key" type="password" bind:value={s3SecretKey} />
				</div>
				<Input
					label="Endpoint (optional)"
					placeholder="https://s3.amazonaws.com"
					bind:value={s3Endpoint}
				/>
			</div>
		</div>
	{/if}

	<div class="flex items-center justify-end gap-3">
		<Button class="btn-primary" loading={storageSaving} onclick={saveStorage}>Save changes</Button>
	</div>
</div>
