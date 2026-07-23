<script lang="ts">
	let {
		children,
		onclick,
		type = 'button' as const,
		disabled = false,
		loading = false,
		class: className = ''
	}: {
		children?: import('svelte').Snippet;
		onclick?: (e: MouseEvent) => void;
		type?: 'button' | 'submit' | 'reset';
		disabled?: boolean;
		loading?: boolean;
		class?: string;
	} = $props();

	let cls = $derived('btn' + (className ? ' ' + className : ''));
</script>

<button
	{type}
	{disabled}
	{onclick}
	class={cls}
	class:loading
>
	{#if loading}
		<span class="btn-spinner"></span>
	{/if}
	{@render children?.()}
</button>

<style>
	@reference '$lib/styles/app.css';

	.btn-spinner {
		--size: 14px;
		width: var(--size);
		height: var(--size);
		flex-shrink: 0;
		border: 2px solid currentColor;
		border-top-color: transparent;
		border-radius: 50%;
		animation: btn-spin 0.6s infinite linear;
	}

	@keyframes btn-spin {
		from {
			transform: rotate(0deg);
		}
		to {
			transform: rotate(360deg);
		}
	}
</style>
