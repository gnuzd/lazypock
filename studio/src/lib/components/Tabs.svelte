<script lang="ts">
	let {
		items = [],
		active = $bindable(''),
		class: className = ''
	}: {
		items: string[];
		active?: string;
		class?: string;
	} = $props();
</script>

<nav class="tabs-header {className}">
	{#each items as item (item)}
		<button class="tab-item" class:active={item === active} onclick={() => (active = item)}>
			{item}
		</button>
	{/each}
</nav>

<style>
	.tabs-header {
		display: flex;
		border-bottom: var(--border, 1px) solid var(--color-base-300);
		gap: 0;
	}

	.tab-item {
		flex: 1;
		padding: 8px 12px;
		font-size: var(--font-size-sm, 0.875rem);
		font-weight: 500;
		color: var(--color-base-content);
		opacity: 0.6;
		background: none;
		border: none;
		border-bottom: 2px solid transparent;
		cursor: pointer;
		transition:
			opacity var(--animation-speed-fast, 0.1s),
			background var(--animation-speed-fast, 0.1s),
			border-color var(--animation-speed-fast, 0.1s),
			color var(--animation-speed-fast, 0.1s);
		text-align: center;
	}

	.tab-item:hover {
		opacity: 1;
		background: var(--color-base-200);
	}

	.tab-item.active {
		opacity: 1;
		border-bottom-color: var(--color-primary);
		color: var(--color-primary);
	}
</style>
