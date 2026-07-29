<script lang="ts">
	let {
		header,
		body,
		footer
	}: {
		header?: import('svelte').Snippet;
		body?: import('svelte').Snippet;
		footer?: import('svelte').Snippet;
	} = $props();
</script>

<aside
	class="flex w-(--sidebar-width) shrink-0 flex-col overflow-hidden border-r border-base-300 bg-base-100"
	style="--sidebar-width: 220px"
>
	{#if header}
		<div class="border-b border-base-300 p-2">
			{@render header()}
		</div>
	{/if}

	{#if body}
		<nav class="flex-1 overflow-y-auto py-1">
			{@render body()}
		</nav>
	{/if}

	{#if footer}
		<div class="border-t border-base-300 p-3">
			{@render footer()}
		</div>
	{/if}
</aside>
