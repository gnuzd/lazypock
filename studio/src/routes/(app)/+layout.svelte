<script lang="ts">
	import Rail from './Rail.svelte';

	const { children } = $props();
</script>

<div class="flex h-dvh w-full">
	<Rail />
	{@render children()}
</div>
