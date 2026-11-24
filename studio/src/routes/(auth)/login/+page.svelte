<script lang="ts">
	import { z } from 'zod';

	import Button from '#lib/components/ui/Button.svelte';
	import Input from '#lib/components/ui/Input.svelte';
	import { createForm } from '#lib/utils/form.svelte.ts';
	import { client } from '#lib/api/client.ts';
	import { toast } from 'svelte-sonner';
	import { ClientError } from 'lazypock-ts';
	import { goto } from '$app/navigation';

	const loginSchema = z.object({
		email: z.string().min(1, 'Email is required').email('Enter a valid email address'),
		password: z.string().min(1, 'Password is required')
	});

	const form = createForm(
		loginSchema,
		{ email: '', password: '' },
		{
			onSubmit: async ({ email, password }) => {
				try {
					await client.auth.login(email, password);
					goto('/collections?collection=users', { replace: true });
				} catch (err) {
					if (err instanceof ClientError) {
						console.log(err);
						toast.error(err.message);
					} else {
						console.log('Network error', err);
					}
				}
			}
		}
	);
</script>

<form class="flex w-90 flex-col gap-6" use:form.enhance novalidate>
	<div class="flex flex-col gap-2">
		<h1 class="text-2xl font-semibold">Welcome back</h1>
		<p class="text-sm text-muted">Sign in to manage your lazypock instance.</p>
	</div>

	<div class="flex flex-col gap-4">
		<Input
			label="Email"
			size="sm"
			bind:value={form.form.email}
			placeholder="admin@example.com"
			error={form.errors.email}
		/>
		<Input
			type="password"
			label="Password"
			size="sm"
			bind:value={form.form.password}
			placeholder="******"
			error={form.errors.password}
		/>
		<Button type="submit">Sign in</Button>
	</div>
</form>
