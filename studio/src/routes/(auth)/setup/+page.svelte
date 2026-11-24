<script lang="ts">
	import { onMount } from 'svelte';
	import { z } from 'zod';
	import { goto } from '$app/navigation';
	import { toast } from 'svelte-sonner';

	import Button from '#lib/components/ui/Button.svelte';
	import Input from '#lib/components/ui/Input.svelte';
	import { createForm } from '#lib/utils/form.svelte.ts';

	const setupSchema = z
		.object({
			email: z.string().min(1, 'Email is required').email('Enter a valid email address'),
			password: z.string().min(1, 'Password is required'),
			confirm: z.string().min(1, 'Confirm your password')
		})
		.refine((data) => data.confirm === data.password, {
			message: "Passwords don't match",
			path: ['confirm']
		});

	const form = createForm(
		setupSchema,
		{ email: '', password: '', confirm: '' },
		{
			onSubmit: async (values) => {}
		}
	);

	const canSubmit = $derived(
		!form.submitting &&
			form.form.email.trim().length > 0 &&
			form.form.password.length > 0 &&
			form.form.confirm.length > 0
	);
</script>

<div class="flex flex-col gap-6">
	<!-- Header -->
	<div class="flex flex-col gap-2">
		<h1 class="text-2xl font-semibold">Create a superuser</h1>
		<p class="text-sm text-muted">
			The first account owns this instance. You can add more accounts later.
		</p>
	</div>

	<!-- Already complete -->
	<!-- {#if complete} -->
	<!-- 	<p class="text-sm text-base-content/70"> -->
	<!-- 		Setup is already complete. Sign in with the existing superuser. -->
	<!-- 	</p> -->
	<!-- {/if} -->

	<form class="flex flex-col gap-4" use:form.enhance novalidate>
		<Input
			label="Email"
			size="sm"
			bind:value={form.form.email}
			placeholder="admin@example.com"
			error={form.errors.email}
		/>
		<Input
			label="Password"
			size="sm"
			type="password"
			bind:value={form.form.password}
			placeholder="******"
			error={form.errors.password}
		/>
		<Input
			label="Confirm password"
			size="sm"
			type="password"
			bind:value={form.form.confirm}
			placeholder="Repeat the password"
			error={form.errors.confirm}
		/>
		<Button type="submit" loading={form.submitting} disabled={!canSubmit}>Create superuser</Button>
	</form>
</div>
