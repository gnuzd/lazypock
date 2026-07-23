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

<aside class="w-(--sidebar-width) flex flex-col border-r border-base-300 bg-base-100 shrink-0 overflow-hidden">
	{#if header}
		<div class="p-2 border-b border-base-300">
			{@render header()}
		</div>
	{/if}

	{#if body}
		<nav class="flex-1 overflow-y-auto py-1">
			{@render body()}
		</nav>
	{/if}

	{#if footer}
		<div class="p-3 border-t border-base-300">
			{@render footer()}
		</div>
	{/if}
</aside>
