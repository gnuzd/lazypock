<script lang="ts">
	import { fly } from 'svelte/transition';

	let {
		show = $bindable(false),
		title = '',
		closable = true,
		headerExtra,
		children
	}: {
		show: boolean;
		title?: string;
		closable?: boolean;
		headerExtra?: import('svelte').Snippet;
		children?: import('svelte').Snippet;
	} = $props();

	function close() {
		show = false;
	}
</script>

<svelte:body onkeydown={(e) => { if (e.key === 'Escape' && show) close(); }} />

{#if show}
	<div
		class="fixed inset-0 z-50 bg-neutral/50"
		role="presentation"
		onclick={close}
	>
		<div
			class="absolute top-0 right-0 h-full w-full max-w-xl flex flex-col bg-base-100 shadow-lg"
			role="dialog"
			aria-label={title || 'Side panel'}
			transition:fly={{ x: 320, duration: 200 }}
			onclick={(e) => e.stopPropagation()}
			onkeydown={(e) => { if (e.key === 'Escape') close(); }}
		>
			{#if title}
				<div class="flex items-center justify-between px-4 py-3 border-b border-base-300 shrink-0">
					<h2 class="font-semibold">{title}</h2>
					<div class="flex items-center gap-1">
						{@render headerExtra?.()}
						{#if closable}
							<button class="btn btn-ghost btn-sm px-2" onclick={close}>✕</button>
						{/if}
					</div>
				</div>
			{/if}
			<div class="flex-1 overflow-auto">
				{@render children?.()}
			</div>
		</div>
	</div>
{/if}
