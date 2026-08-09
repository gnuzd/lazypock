<script lang="ts">
	import { client } from '$lib/client';
	import Input from '$lib/components/Input.svelte';
	import Button from '$lib/components/Button.svelte';
	import { createForm } from '$lib/createForm.svelte';

	import { toast } from 'svelte-sonner';
	import { base } from '$app/paths';
	import { browser } from '$app/environment';
	import { loginSchema, setupSchema } from '$lib/validation';
	import { onMount } from 'svelte';

	let loginForm = $state(createForm(loginSchema, { email: '', password: '' }));
	let setupForm = $state(
		createForm(setupSchema, { email: '', password: '', confirmPassword: '' })
	);
	let checking = $state(true);
	let needsSetup = $state(false);

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

	async function handleLogin(data: { email: string; password: string }) {
		try {
			await client.login(data.email, data.password);
			await client.me();
			if (browser) window.location.href = base + '/collections?collection=users';
		} catch (err) {
			toast.error((err as { message?: string }).message || 'Login failed');
		}
	}

	async function handleSetup(data: { email: string; password: string; confirmPassword: string }) {
		try {
			await client.setup(data.email, data.password);
			await client.login(data.email, data.password);
			await client.me();
			const res = await client.collections.getList({ page: 1, perPage: 200 });
			const name = res?.items?.[0]?.name ?? '';
			if (browser) window.location.href = base + '/collections?collection=' + name;
		} catch (err) {
			toast.error((err as { message?: string }).message || 'Setup failed');
		}
	}
</script>

<div class="flex min-h-screen items-center justify-center">
	{#if checking}
		<div class="text-sm text-base-content/40">Checking...</div>
	{:else if needsSetup}
		<form
			class="flex w-full max-w-md flex-col gap-3 p-[30px]"
			onsubmit={(e) => setupForm.handleSubmit(e, handleSetup)}
		>
			<div class="mb-3 text-center">
				<h1 class="mt-2.5 text-[22px] font-semibold">Lazypock Setup</h1>
				<p class="mt-1 text-sm text-base-content/60">Create the first superuser account</p>
			</div>

			<Input
				id="setup-email"
				type="email"
				label="Email"
				placeholder="admin@example.com"
				bind:value={setupForm.values.email}
				error={setupForm.errors.email}
				required
			/>

			<Input
				id="setup-password"
				type="password"
				label="Password"
				placeholder="password"
				bind:value={setupForm.values.password}
				error={setupForm.errors.password}
				required
			/>

			<Input
				id="setup-confirm"
				type="password"
				label="Confirm Password"
				placeholder="password"
				bind:value={setupForm.values.confirmPassword}
				error={setupForm.errors.confirmPassword}
				required
			/>

			<Button
				type="submit"
				class="btn-primary btn-md btn-full"
				loading={setupForm.submitting}
				disabled={setupForm.submitting}
			>
				{setupForm.submitting ? 'Creating...' : 'Create Superuser'}
			</Button>
		</form>
	{:else}
		<form
			class="flex w-full max-w-md flex-col gap-3 p-[30px]"
			onsubmit={(e) => loginForm.handleSubmit(e, handleLogin)}
		>
			<div class="mb-3 text-center">
				<h1 class="mt-2.5 text-[22px] font-semibold">Lazypock</h1>
			</div>

			<Input
				id="email"
				type="email"
				label="Email"
				placeholder="superuser@example.com"
				bind:value={loginForm.values.email}
				error={loginForm.errors.email}
				required
			/>

			<Input
				id="password"
				type="password"
				label="Password"
				placeholder="password"
				bind:value={loginForm.values.password}
				error={loginForm.errors.password}
				required
			/>

			<Button
				type="submit"
				class="btn-primary btn-md btn-full"
				loading={loginForm.submitting}
				disabled={loginForm.submitting}
			>
				{loginForm.submitting ? 'Signing in...' : 'Sign in'}
			</Button>
		</form>
	{/if}
</div>
