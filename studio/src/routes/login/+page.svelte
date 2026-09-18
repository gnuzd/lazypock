<script lang="ts">
	import { onMount } from 'svelte';
	import { toast } from 'svelte-sonner';

	import { base } from '$app/paths';
	import { browser } from '$app/environment';

	import { client } from '$lib/client';
	import { createForm } from '$lib/createForm.svelte';
	import { loginSchema, setupSchema } from '$lib/validation';

	import Input from '$lib/components/Input.svelte';
	import Button from '$lib/components/Button.svelte';
	import { goto } from '$app/navigation';

	let loginForm = createForm(loginSchema, { email: '', password: '' });
	let setupForm = createForm(setupSchema, { email: '', password: '', confirmPassword: '' });
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
			if (browser) goto(base + '/collections?collection=users', { replaceState: true });
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
			if (browser) goto(base + '/collections?collection=' + name, { replaceState: true });
		} catch (err) {
			toast.error((err as { message?: string }).message || 'Setup failed');
		}
	}
</script>

<div class="flex min-h-screen items-center justify-center">
	{#if checking}
		<div class="text-sm text-base-content/40">Checking...</div>
	{:else if needsSetup}
		<form class="flex w-full max-w-md flex-col gap-3 p-7.5" use:setupForm.enhance={handleSetup}>
			<div class="mb-3 text-center">
				<h1 class="mt-2.5 text-[22px] font-semibold">Lazypock Setup</h1>
				<p class="mt-1 text-sm text-base-content/60">Create the first superuser account</p>
			</div>

			<Input
				id="setup-email"
				type="email"
				label="Email"
				placeholder="admin@example.com"
				bind:value={setupForm.form.email}
				error={setupForm.errors.email}
				required
			/>

			<Input
				id="setup-password"
				type="password"
				label="Password"
				placeholder="password"
				bind:value={setupForm.form.password}
				error={setupForm.errors.password}
				required
			/>

			<Input
				id="setup-confirm"
				type="password"
				label="Confirm Password"
				placeholder="password"
				bind:value={setupForm.form.confirmPassword}
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
		<form class="flex w-full max-w-md flex-col gap-3 p-7.5" use:loginForm.enhance={handleLogin}>
			<div class="mb-3 text-center">
				<h1 class="mt-2.5 text-[22px] font-semibold">Lazypock</h1>
			</div>

			<Input
				id="email"
				type="email"
				label="Email"
				placeholder="superuser@example.com"
				bind:value={loginForm.form.email}
				error={loginForm.errors.email}
				required
			/>

			<Input
				id="password"
				type="password"
				label="Password"
				placeholder="password"
				bind:value={loginForm.form.password}
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
