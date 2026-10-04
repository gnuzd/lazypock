<script lang="ts">
	import { quintInOut } from 'svelte/easing';
	import { fade } from 'svelte/transition';

	let {
		show = $bindable(false),
		title = '',
		size = 'md' as 'sm' | 'md' | 'lg' | 'xl',
		bodyClass = '',
		onDismiss,
		children
	}: {
		show?: boolean;
		title?: string;
		/** Width preset — `xl` for media/library views that need room. */
		size?: 'sm' | 'md' | 'lg' | 'xl';
		/** Extra classes for the body (e.g. `flex min-h-0 flex-col` so inner areas scroll). */
		bodyClass?: string;
		/**
		 * Called when the dialog closes *itself* (overlay click, Escape, ✕) so
		 * callers can react — e.g. settling a pending promise. Not called when a
		 * caller sets `show` to false.
		 */
		onDismiss?: () => void;
		children?: import('svelte').Snippet;
	} = $props();

	function close() {
		show = false;
		onDismiss?.();
	}
</script>

<svelte:body
	onkeydown={(e) => {
		if (e.key === 'Escape' && show) close();
	}}
/>

{#if show}
	<!-- svelte-ignore a11y_click_events_have_key_events a11y_interactive_supports_focus -->
	<div
		class="modal-overlay"
		transition:fade={{ duration: 200, easing: quintInOut }}
		onclick={close}
		role="dialog"
	>
		<!-- svelte-ignore a11y_click_events_have_key_events -->
		<div class="modal size-{size}" onclick={(e) => e.stopPropagation()}>
			{#if title}
				<div class="modal-header">
					<h2 class="modal-title">{title}</h2>
					<button class="modal-close" onclick={close}>&times;</button>
				</div>
			{/if}
			<div class="modal-body {bodyClass}">
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
		max-width: var(--modal-max-width, 540px);
		max-height: var(--modal-max-height, 85vh);
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

	/* Width/height presets. `xl` is the media/library size: wide enough for the
	   thumbnail grid and tall enough that its own area scrolls inside. */
	.modal.size-sm {
		--modal-max-width: 420px;
	}

	.modal.size-lg {
		--modal-max-width: 820px;
	}

	.modal.size-xl {
		--modal-max-width: min(1100px, 94vw);
		--modal-max-height: 92vh;
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
		min-height: 0;
	}
</style>
