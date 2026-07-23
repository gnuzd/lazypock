<script lang="ts">
	import { client } from '$lib/client';
	import Input from '$lib/components/Input.svelte';
	import Button from '$lib/components/Button.svelte';

	import { toast } from 'svelte-sonner';
	import { base } from '$app/paths';
	import { browser } from '$app/environment';

	let email = $state('');
	let password = $state('');
	let loading = $state(false);

	async function handleSubmit(e: Event) {
		e.preventDefault();
		loading = true;

		try {
			await client.login(email, password);
			await client.me();
			const result = await client.listCollections('page=1&perPage=200');
			const name = result?.items?.[0]?.name ?? '';
			if (browser) window.location.href = base + '/collections?collection=' + name;
		} catch (err) {
			toast.error((err as { message?: string }).message || 'Login failed');
		} finally {
			loading = false;
		}
	}
</script>

<div class="flex items-center justify-center min-h-screen">
	<form class="w-full max-w-md p-[30px] flex flex-col gap-3" onsubmit={handleSubmit}>
		<div class="text-center mb-3">
			<h1 class="text-[22px] font-semibold mt-2.5">Lazypock</h1>
		</div>

		<Input
			id="email"
			type="email"
			label="Email"
			placeholder="superuser@example.com"
			bind:value={email}
			required
		/>

		<Input
			id="password"
			type="password"
			label="Password"
			placeholder="password"
			bind:value={password}
			required
		/>

		<Button type="submit" class="btn-primary btn-md btn-full" {loading} disabled={loading}>
			{loading ? 'Signing in...' : 'Sign in'}
		</Button>
	</form>
</div>
