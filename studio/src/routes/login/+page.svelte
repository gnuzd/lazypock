<script lang="ts">
	import { client } from '$lib/client';
	import Input from '$lib/components/Input.svelte';
	import Button from '$lib/components/Button.svelte';

	import { toast } from 'svelte-sonner';
	import { base } from '$app/paths';
	import { browser } from '$app/environment';
	import { loginSchema, setupSchema } from '$lib/validation';
	import { onMount } from 'svelte';

	let formData = $state({ email: '', password: '' });
	let setupData = $state({ email: '', password: '', confirmPassword: '' });
	let loading = $state(false);
	let checking = $state(true);
	let needsSetup = $state(false);
	let fieldErrors = $state<Record<string, string>>({});

	onMount(async () => {
		try {
			const res = await client.checkSuperuser();
			needsSetup = res?.has_superuser === false;
		} catch {
			// If the check fails, assume login mode
		} finally {
			checking = false;
		}
	});

	async function handleLogin(e: Event) {
		e.preventDefault();

		const result = loginSchema.safeParse(formData);
		if (!result.success) {
			const errs: Record<string, string> = {};
			for (const issue of result.error.issues) {
				const fieldName = issue.path.join('.');
				if (!errs[fieldName]) {
					errs[fieldName] = issue.message;
				}
			}
			fieldErrors = errs;
			return;
		}

		fieldErrors = {};
		loading = true;

		try {
			await client.login(result.data.email, result.data.password);
			await client.me();
			if (browser) window.location.href = base + '/collections?collection=users';
		} catch (err) {
			toast.error((err as { message?: string }).message || 'Login failed');
		} finally {
			loading = false;
		}
	}

	async function handleSetup(e: Event) {
		e.preventDefault();

		const result = setupSchema.safeParse(setupData);
		if (!result.success) {
			const errs: Record<string, string> = {};
			for (const issue of result.error.issues) {
				const fieldName = issue.path.join('.');
				if (!errs[fieldName]) {
					errs[fieldName] = issue.message;
				}
			}
			fieldErrors = errs;
			return;
		}

		fieldErrors = {};
		loading = true;

		try {
			await client.setup(result.data.email, result.data.password);
			await client.login(result.data.email, result.data.password);
			await client.me();
			const res = await client.collections.getList({ page: 1, perPage: 200 });
			const name = res?.items?.[0]?.name ?? '';
			if (browser) window.location.href = base + '/collections?collection=' + name;
		} catch (err) {
			toast.error((err as { message?: string }).message || 'Setup failed');
		} finally {
			loading = false;
		}
	}
</script>

<div class="flex min-h-screen items-center justify-center">
	{#if checking}
		<div class="text-sm text-base-content/40">Checking...</div>
	{:else if needsSetup}
		<form class="flex w-full max-w-md flex-col gap-3 p-[30px]" onsubmit={handleSetup}>
			<div class="mb-3 text-center">
				<h1 class="mt-2.5 text-[22px] font-semibold">Lazypock Setup</h1>
				<p class="mt-1 text-sm text-base-content/60">Create the first superuser account</p>
			</div>

			<Input
				id="setup-email"
				type="email"
				label="Email"
				placeholder="admin@example.com"
				bind:value={setupData.email}
				error={fieldErrors.email}
				required
			/>

			<Input
				id="setup-password"
				type="password"
				label="Password"
				placeholder="password"
				bind:value={setupData.password}
				error={fieldErrors.password}
				required
			/>

			<Input
				id="setup-confirm"
				type="password"
				label="Confirm Password"
				placeholder="password"
				bind:value={setupData.confirmPassword}
				error={fieldErrors.confirmPassword}
				required
			/>

			<Button type="submit" class="btn-primary btn-md btn-full" {loading} disabled={loading}>
				{loading ? 'Creating...' : 'Create Superuser'}
			</Button>
		</form>
	{:else}
		<form class="flex w-full max-w-md flex-col gap-3 p-[30px]" onsubmit={handleLogin}>
			<div class="mb-3 text-center">
				<h1 class="mt-2.5 text-[22px] font-semibold">Lazypock</h1>
			</div>

			<Input
				id="email"
				type="email"
				label="Email"
				placeholder="superuser@example.com"
				bind:value={formData.email}
				error={fieldErrors.email}
				required
			/>

			<Input
				id="password"
				type="password"
				label="Password"
				placeholder="password"
				bind:value={formData.password}
				error={fieldErrors.password}
				required
			/>

			<Button type="submit" class="btn-primary btn-md btn-full" {loading} disabled={loading}>
				{loading ? 'Signing in...' : 'Sign in'}
			</Button>
		</form>
	{/if}
</div>
