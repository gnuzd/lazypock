<script lang="ts">
	let {
		show = $bindable(false),
		title = '',
		children
	}: {
		show?: boolean;
		title?: string;
		children?: import('svelte').Snippet;
	} = $props();

	function close() {
		show = false;
	}
</script>

<svelte:body
	onkeydown={(e) => {
		if (e.key === 'Escape' && show) close();
	}}
/>

{#if show}
	<!-- svelte-ignore a11y_click_events_have_key_events a11y_interactive_supports_focus -->
	<div class="modal-overlay" onclick={close} role="dialog">
		<!-- svelte-ignore a11y_click_events_have_key_events -->
		<div class="modal" onclick={(e) => e.stopPropagation()}>
			{#if title}
				<div class="modal-header">
					<h2 class="modal-title">{title}</h2>
					<button class="modal-close" onclick={close}>&times;</button>
				</div>
			{/if}
			<div class="modal-body">
				{@render children?.()}
			</div>
		</div>
	</div>
{/if}

<style>
	.modal-overlay {
		position: fixed;
		z-index: 1000;
		inset: 0;
		display: flex;
		align-items: center;
		justify-content: center;
		background: var(
			--modal-overlay,
			color-mix(in srgb, var(--color-neutral, #000), transparent 50%)
		);
		padding: 20px;
	}

	.modal {
		display: flex;
		flex-direction: column;
		width: 100%;
		max-width: 540px;
		max-height: 85vh;
		border: 0;
		outline: 0;
		margin: 0;
		word-break: break-word;
		color: var(--color-base-content);
		background: var(--color-base-100);
		border-radius: var(--radius-box, 8px);
		box-shadow:
			0 25px 50px -12px rgba(0, 0, 0, 0.25),
			0 0 0 1px color-mix(in srgb, var(--color-base-content) 10%, transparent);
		overflow: hidden;
	}

	.modal-header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 16px 20px;
		border-bottom: var(--border, 1px) solid var(--color-base-300);
		flex-shrink: 0;
	}

	.modal-title {
		margin: 0;
		font-size: var(--font-size-base, 0.9375rem);
		font-weight: 600;
		line-height: 1;
	}

	.modal-close {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		width: 28px;
		height: 28px;
		border: 0;
		outline: 0;
		background: none;
		cursor: pointer;
		font-size: 1.4rem;
		line-height: 1;
		color: var(--color-base-hint);
		border-radius: var(--radius-field, 6px);
		transition: background var(--animation-speed, 0.2s);
		flex-shrink: 0;
		padding: 0;
	}

	.modal-close:hover {
		background: var(--color-base-300);
		color: var(--color-base-content);
	}

	.modal-body {
		padding: 20px;
		overflow-y: auto;
		flex: 1;
	}
</style>
