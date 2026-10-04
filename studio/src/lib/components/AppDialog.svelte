<script lang="ts">
	import Button from '$lib/components/Button.svelte';
	import Modal from '$lib/components/Modal.svelte';
	import { currentDialog, settleDialog } from '$lib/dialog.svelte';

	let request = $derived(currentDialog()?.request ?? null);
	let inputEl = $state<HTMLInputElement | null>(null);

	// Focus the prompt input once it exists (not `autofocus`, which trips a11y).
	$effect(() => {
		if (request?.kind === 'prompt' && inputEl) inputEl.focus();
	});

	function cancel() {
		settleDialog(request?.kind === 'confirm' ? false : null);
	}

	function accept() {
		settleDialog(request?.kind === 'prompt' ? (inputEl?.value ?? '') : true);
	}
</script>

{#if request}
	<!-- Keyed on the request so each dialog mounts fresh (and a prompt starts
	     with the right value) without syncing local state. -->
	{#key request}
		<Modal show={true} size="sm" title={request.title ?? ''} onDismiss={cancel}>
			<p class="dialog-message">{request.message}</p>

			{#if request.details && request.details.length > 0}
				<ul class="dialog-details">
					{#each request.details as detail (detail)}
						<li><code>{detail}</code></li>
					{/each}
				</ul>
			{/if}

			{#if request.kind === 'prompt'}
				<input
					class="dialog-input"
					bind:this={inputEl}
					value={request.value ?? ''}
					placeholder={request.placeholder ?? ''}
					onkeydown={(e) => {
						if (e.key === 'Enter') accept();
					}}
				/>
			{/if}

			<div class="dialog-actions">
				{#if request.kind !== 'alert'}
					<Button class="btn-sm" onclick={cancel}>{request.cancelLabel ?? 'Cancel'}</Button>
				{/if}
				<Button
					class={request.variant === 'error' ? 'btn-error btn-sm' : 'btn-primary btn-sm'}
					onclick={accept}
				>
					{request.confirmLabel ?? (request.kind === 'alert' ? 'OK' : 'Confirm')}
				</Button>
			</div>
		</Modal>
	{/key}
{/if}

<style>
	.dialog-message {
		margin: 0 0 10px;
		font-size: 0.875rem;
	}

	.dialog-details {
		display: flex;
		flex-direction: column;
		gap: 2px;
		margin: 0 0 12px;
		padding-left: 1.1rem;
		font-size: 0.75rem;
		color: color-mix(in oklab, var(--color-base-content) 70%, transparent);
	}

	.dialog-input {
		width: 100%;
		margin-bottom: 12px;
		padding: 6px 10px;
		font-size: 0.875rem;
		color: var(--color-base-content);
		background: var(--color-base-100);
		border: 1px solid var(--color-base-300);
		border-radius: var(--radius-field, 6px);
	}

	.dialog-actions {
		display: flex;
		justify-content: flex-end;
		gap: 8px;
	}
</style>
