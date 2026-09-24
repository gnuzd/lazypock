<script lang="ts">
	import type { Snippet } from 'svelte';

	/**
	 * Settings switch toggle.
	 *
	 * `variant="field"` (default) is the tinted row with the label on the left
	 * and the switch on the right; `variant="inline"` puts the switch first with
	 * an optional description under the label (used when the setting needs
	 * explaining).
	 */
	const defaultId = $props.id();

	let {
		checked = $bindable(false),
		disabled = false,
		variant = 'field',
		description = '',
		id = defaultId,
		class: className = '',
		children
	}: {
		checked?: boolean;
		disabled?: boolean;
		variant?: 'field' | 'inline';
		/** Optional helper text, rendered under the label (inline variant). */
		description?: string;
		id?: string;
		class?: string;
		children?: Snippet;
	} = $props();
</script>

{#snippet control()}
	<label class="switch">
		<input {id} type="checkbox" bind:checked {disabled} />
		<span class="switch-slider"></span>
	</label>
{/snippet}

{#if variant === 'inline'}
	<div class="inline-switch {className}">
		{@render control()}
		<div>
			<label class="switch-label" for={id}>
				<span class="txt">{@render children?.()}</span>
			</label>
			{#if description}
				<p class="text-xs text-base-content/50">{description}</p>
			{/if}
		</div>
	</div>
{:else}
	<div class="switch-field {className}">
		<label class="switch-label" for={id}>
			<span class="txt">{@render children?.()}</span>
		</label>
		{@render control()}
	</div>
{/if}

<style>
	.switch-field {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 12px;
		padding: 12px;
		border-radius: var(--radius-field);
		background: color-mix(in oklab, var(--color-base-content) 8%, var(--color-base-100));
		transition: background var(--animation-speed, 0.2s);
	}

	.switch-field:focus-within {
		background: color-mix(in oklab, var(--color-base-content) 12%, var(--color-base-100));
	}

	.inline-switch {
		display: flex;
		align-items: flex-start;
		gap: 8px;
	}

	.switch-label {
		flex: 1;
		cursor: pointer;
		font-size: 0.9375rem;
		color: var(--color-base-content);
	}

	.switch-label .txt {
		opacity: 0.85;
	}

	.switch {
		position: relative;
		display: inline-block;
		width: 44px;
		height: 24px;
		flex-shrink: 0;
	}

	.switch input {
		opacity: 0;
		width: 0;
		height: 0;
		position: absolute;
	}

	.switch-slider {
		position: absolute;
		cursor: pointer;
		inset: 0;
		background: color-mix(in oklab, var(--color-base-content) 25%, transparent);
		border-radius: 24px;
		transition: 0.2s;
	}

	.switch-slider::before {
		content: '';
		position: absolute;
		height: 18px;
		width: 18px;
		left: 3px;
		bottom: 3px;
		background: white;
		border-radius: 50%;
		transition: 0.2s;
	}

	.switch input:checked + .switch-slider {
		background: var(--color-primary);
	}

	.switch input:checked + .switch-slider::before {
		transform: translateX(20px);
	}

	.switch input:disabled + .switch-slider {
		opacity: 0.4;
		cursor: not-allowed;
	}
</style>
