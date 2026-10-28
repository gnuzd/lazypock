<script lang="ts">
	import { client } from '$lib/client';
	import { onMount } from 'svelte';
	import Button from '$lib/components/Button.svelte';
	import Input from '$lib/components/Input.svelte';
	import { toast } from 'svelte-sonner';

	// The one "send a test email" control, shared by Mail Settings and the
	// template editor so both queue through the same path: a raw message when no
	// template is named, and `{ to, template, assigns }` when one is. The message
	// is queued, not delivered — a failure shows in the logs, not here.
	let {
		/** A template to send; omitted → a plain test message. */
		template = '',
		/** The template context. Ignored when `template` is empty. */
		assigns = {},
		/** Panel copy; defaults to a description of whichever mode is in use. */
		description = '',
		/** The input id, so two instances on one page stay labelled separately. */
		id = 'test-recipient'
	}: {
		template?: string;
		assigns?: Record<string, unknown>;
		description?: string;
		id?: string;
	} = $props();

	let recipient = $state('');
	let sending = $state(false);

	const copy = $derived(
		description ||
			(template
				? 'Queues this template to the address below, rendered with the sample values on this page — save first if the change is not stored yet.'
				: 'Uses the saved settings, so save first. The message is queued like any other — a delivery failure shows in the logs rather than here.')
	);

	onMount(async () => {
		try {
			const me = (await client.me()) as Record<string, unknown> | null;
			if (typeof me?.email === 'string') recipient = me.email;
		} catch {
			// no identity to prefill; the field stays empty
		}
	});

	async function sendTestEmail() {
		const to = recipient.trim();
		if (!to || sending) return;
		sending = true;
		try {
			await client.mail.send(
				template
					? { to, template, assigns }
					: {
							to,
							subject: 'Lazypock test email',
							text: 'This is a test email from your Lazypock server. If you can read it, SMTP is configured correctly.',
							html: '<p>This is a test email from your Lazypock server.</p><p>If you can read it, SMTP is configured correctly.</p>'
						}
			);
			toast.success('Test email queued — check the inbox in a moment.');
		} catch (err) {
			toast.error((err as { message?: string }).message || 'Could not queue the test email');
		} finally {
			sending = false;
		}
	}
</script>

<div class="rounded-box border border-base-300 bg-base-100 p-6">
	<div class="mb-4 text-sm text-base-content/60">
		<p>
			<strong class="text-base-content">Send a test email.</strong>
			{copy}
		</p>
	</div>
	<div class="flex flex-col gap-3 sm:flex-row sm:items-end">
		<div class="flex-1">
			<Input
				{id}
				label="Send to"
				type="email"
				placeholder="you@example.com"
				bind:value={recipient}
			/>
		</div>
		<Button
			class="btn-outline"
			loading={sending}
			disabled={sending || !recipient.trim()}
			onclick={sendTestEmail}
		>
			Send test email
		</Button>
	</div>
</div>
