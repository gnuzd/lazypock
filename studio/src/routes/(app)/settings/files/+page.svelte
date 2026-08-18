<script lang="ts">
	import { client } from '$lib/client';
	import { onMount } from 'svelte';
	import { slide } from 'svelte/transition';
	import { z } from 'zod';
	import Button from '$lib/components/Button.svelte';
	import Input from '$lib/components/Input.svelte';
	import { createForm } from '$lib/createForm.svelte';
	import '../settings.css';

	const storageSchema = z
		.object({
			storageEnabled: z.boolean(),
			s3Bucket: z.string(),
			s3Region: z.string(),
			s3AccessKey: z.string(),
			s3SecretKey: z.string(),
			s3Endpoint: z.string()
		})
		.superRefine((data, ctx) => {
			if (data.storageEnabled && !data.s3Bucket.trim()) {
				ctx.addIssue({ code: 'custom', path: ['s3Bucket'], message: 'Bucket is required' });
			}
		});

	let storageForm = $state(
		createForm(storageSchema, {
			storageEnabled: false,
			s3Bucket: '',
			s3Region: 'us-east-1',
			s3AccessKey: '',
			s3SecretKey: '',
			s3Endpoint: ''
		})
	);

	onMount(async () => {
		try {
			const res = (await client.http.get('/settings')) as Record<string, unknown> | null;
			if (!res) return;
			const v = storageForm.values;
			v.storageEnabled = (res.s3_enabled as boolean) ?? false;
			const s3 = (res.s3 as Record<string, string>) ?? {};
			v.s3Bucket = s3.bucket ?? '';
			v.s3Region = s3.region ?? 'us-east-1';
			v.s3AccessKey = s3.access_key ?? '';
			v.s3SecretKey = s3.secret_key ?? '';
			v.s3Endpoint = s3.endpoint ?? '';
		} catch {
			// not configured
		}
	});

	async function saveStorage(data: z.infer<typeof storageSchema>) {
		try {
			await client.http.patch('/settings', {
				s3_enabled: data.storageEnabled,
				s3: data.storageEnabled
					? {
							bucket: data.s3Bucket,
							region: data.s3Region,
							access_key: data.s3AccessKey,
							secret_key: data.s3SecretKey,
							endpoint: data.s3Endpoint
						}
					: null
			});
		} catch {
			// ignore
		}
	}
</script>

<h2 class="mb-4 text-lg font-semibold">Files Storage</h2>
<form
	class="rounded-box border border-base-300 bg-base-100 p-6"
	onsubmit={(e) => storageForm.handleSubmit(e, saveStorage)}
>
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
			<input
				id="storage-enabled"
				type="checkbox"
				bind:checked={storageForm.values.storageEnabled}
			/>
			<span class="switch-slider"></span>
		</label>
	</div>

	{#if storageForm.values.storageEnabled}
		<div transition:slide={{ duration: 150 }}>
			<div class="flex flex-col gap-3">
				<div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
					<Input
						label="S3 Bucket"
						placeholder="my-bucket"
						bind:value={storageForm.values.s3Bucket}
						error={storageForm.errors.s3Bucket}
						required
					/>
					<Input label="Region" placeholder="us-east-1" bind:value={storageForm.values.s3Region} />
				</div>
				<div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
					<Input label="Access Key" bind:value={storageForm.values.s3AccessKey} />
					<Input
						label="Secret Key"
						type="password"
						autocomplete="new-password"
						bind:value={storageForm.values.s3SecretKey}
					/>
				</div>
				<Input
					label="Endpoint (optional)"
					placeholder="https://s3.amazonaws.com"
					bind:value={storageForm.values.s3Endpoint}
				/>
			</div>
		</div>
	{/if}

	<div class="flex items-center justify-end gap-3">
		<Button class="btn-primary" loading={storageForm.submitting} type="submit">Save changes</Button>
	</div>
</form>
