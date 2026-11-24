<script lang="ts">
	import type { Snippet } from 'svelte';
	import { fly, fade, slide } from 'svelte/transition';
	import { quintOut } from 'svelte/easing';
	import type { DrawerSize, DrawerPosition } from '#lib/utils/drawer.svelte.ts';
	import { cn } from '#lib/utils/cn.ts';
	import Button from './Button.svelte';
	import { X } from '@lucide/svelte';

	type Props = {
		open: boolean;
		title?: string;
		size?: DrawerSize;
		position?: DrawerPosition;
		dismissible?: boolean;
		onclose: () => void;
		children: Snippet;
	};

	let {
		open,
		title,
		size = 'md',
		position = 'right',
		dismissible = true,
		onclose,
		children
	}: Props = $props();

	const sizeClasses: Record<DrawerSize, string> = {
		sm: 'w-80',
		md: 'w-96',
		lg: 'w-[32rem]',
		xl: 'w-[48rem]',
		full: 'w-screen'
	};

	// Body scroll lock
	$effect(() => {
		if (typeof document === 'undefined' || !open) return;
		const prev = document.body.style.overflow;
		document.body.style.overflow = 'hidden';
		return () => {
			document.body.style.overflow = prev;
		};
	});

	// ESC to close
	$effect(() => {
		if (!open || !dismissible) return;
		const handler = (e: KeyboardEvent) => {
			if (e.key === 'Escape') onclose();
		};
		window.addEventListener('keydown', handler);
		return () => window.removeEventListener('keydown', handler);
	});
</script>

<div
	class="fixed inset-0 z-40 bg-neutral/50"
	transition:fade={{ duration: 180 }}
	onclick={() => dismissible && onclose()}
	onkeydown={(e) => e.key === 'Enter' && dismissible && onclose()}
	role="button"
	tabindex="-1"
	aria-label="Close drawer"
></div>

<aside
	class={cn(
		'fixed top-2 bottom-2 z-50 flex max-w-[95vw] flex-col rounded-box bg-base-100 shadow-2xl',
		position === 'left' ? 'left-0' : 'right-2',
		sizeClasses[size]
	)}
	transition:fly={{
		x: position === 'left' ? -400 : 400,
		duration: 260,
		easing: quintOut
	}}
	role="dialog"
	aria-modal="true"
	aria-label={title}
>
	<!-- Header -->
	<div class="flex items-center justify-between border-b px-3 py-2">
		<h2 class="text-lg font-semibold text-gray-900">{title ?? ''}</h2>
		{#if dismissible}
			<Button
				variant="neutral"
				buttonStyle="ghost"
				size="xs"
				shape="square"
				onclick={onclose}
				aria-label="Close drawer"
			>
				<X size={16} />
			</Button>
		{/if}
	</div>

	<!-- Content -->
	<div class="flex-1 overflow-y-auto px-5 py-4">
		{@render children()}
	</div>
</aside>
