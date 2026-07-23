<script lang="ts">
	import { fly } from 'svelte/transition';

	let {
		show = $bindable(false),
		class: className = '',
		trigger,
		children,
	}: {
		show: boolean;
		class?: string;
		trigger?: import('svelte').Snippet;
		children?: import('svelte').Snippet;
	} = $props();

	let wrapper: HTMLDivElement | undefined = $state();

	$effect(() => {
		if (show) {
			function handle(e: MouseEvent) {
				if (wrapper && !wrapper.contains(e.target as Node)) {
					show = false;
				}
			}
			document.addEventListener('mousedown', handle);
			return () => document.removeEventListener('mousedown', handle);
		}
	});
</script>

<div bind:this={wrapper} class="relative {className}">
	<div onclick={() => show = !show} role="button" tabindex="-1">
		{@render trigger?.()}
	</div>

	{#if show}
		<div
			class="absolute z-50 mt-1 bg-base-100 border-2 border-primary rounded-field shadow-lg"
			transition:fly={{ y: -6, duration: 120 }}
		>
			{@render children?.()}
		</div>
	{/if}
</div>
