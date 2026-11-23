<script lang="ts">
	import type { Snippet } from 'svelte';
	import type { HTMLButtonAttributes, HTMLAnchorAttributes } from 'svelte/elements';
	import { Loader2 } from '@lucide/svelte';
	import { match } from 'ts-pattern';
	import { cn } from '#lib/utils/cn.ts';

	type Size = 'xs' | 'sm' | 'md' | 'lg';
	type Shape = 'default' | 'square' | 'circle';
	type Style = 'solid' | 'outline' | 'ghost' | 'soft' | 'link';
	type Variant =
		'primary' | 'secondary' | 'accent' | 'neutral' | 'info' | 'success' | 'warning' | 'error';

	type CommonProps = {
		variant?: Variant;
		size?: Size;
		shape?: Shape;
		buttonStyle?: Style;
		loading?: boolean;
		fullWidth?: boolean;
		leadingIcon?: Snippet;
		trailingIcon?: Snippet;
		iconOnly?: Snippet;
		loadingText?: string;
		loadingMode?: 'overlay' | 'replace' | 'inline';
		children?: Snippet;
		class?: string;
	};

	type Props = CommonProps &
		(
			| (HTMLButtonAttributes & { href?: undefined })
			| (Omit<HTMLAnchorAttributes, 'type'> & { href: string })
		);

	let {
		variant = 'primary',
		size = 'md',
		shape = 'default',
		buttonStyle = 'solid', // <-- This is the prop
		loading = false,
		fullWidth = false,
		leadingIcon,
		trailingIcon,
		iconOnly,
		loadingText,
		loadingMode = 'overlay',
		children,
		class: classes = '',
		...rest
	}: Props = $props();

	const isAnchor = $derived('href' in rest && rest.href !== undefined);
	const isIconOnly = $derived(!!iconOnly);
	const isFixedShape = $derived(shape === 'square' || shape === 'circle');

	const isDisabled = $derived(
		isAnchor
			? loading || (rest as any)['aria-disabled'] === true
			: loading || (rest as any).disabled
	);

	// Fallback colors per variant if the theme is missing variables
	const BG_FALLBACK: Record<Variant, string> = {
		primary: 'oklch(75% 0.1 190)',
		secondary: 'oklch(75% 0.12 20)',
		accent: 'oklch(78% 0.12 70)',
		neutral: 'oklch(30% 0.03 280)',
		info: 'oklch(75% 0.12 225)',
		success: 'oklch(75% 0.14 160)',
		warning: 'oklch(85% 0.14 90)',
		error: 'oklch(65% 0.2 25)'
	};

	// Sensible content fallbacks (warning needs dark text on light yellow)
	const CONTENT_FALLBACK: Record<Variant, string> = {
		primary: 'oklch(98% 0 0)',
		secondary: 'oklch(98% 0 0)',
		accent: 'oklch(98% 0 0)',
		neutral: 'oklch(98% 0 0)',
		info: 'oklch(98% 0 0)',
		success: 'oklch(98% 0 0)',
		warning: 'oklch(20% 0 0)',
		error: 'oklch(98% 0 0)'
	};

	const SIZE_CLASSES: Record<Size, string> = {
		xs: 'h-8 min-h-8 text-xs',
		sm: 'h-9 min-h-9 text-sm',
		md: 'h-10 min-h-10 text-sm',
		lg: 'h-12 min-h-12 text-base'
	};
	const TEXT_PADDING: Record<Size, string> = {
		xs: 'px-2.5 gap-1.5',
		sm: 'px-3 gap-2',
		md: 'px-4 gap-2',
		lg: 'px-6 gap-2.5'
	};
	const SQUARE_PADDING: Record<Size, string> = {
		xs: 'w-8 min-w-8',
		sm: 'w-9 min-w-9',
		md: 'w-10 min-w-10',
		lg: 'w-12 min-w-12'
	};

	const shapeClass = $derived(
		match(shape)
			.with('circle', () => 'rounded-full')
			.with('square', () => 'rounded-[var(--radius-selector,1rem)]')
			.with('default', () => 'rounded-[var(--radius-field,1rem)]')
			.exhaustive()
	);

	const sizingClass = $derived(
		`${SIZE_CLASSES[size]} ${isFixedShape || isIconOnly ? SQUARE_PADDING[size] : TEXT_PADDING[size]}`
	);

	const styleClass = $derived(
		match(buttonStyle)
			.with('outline', () => 'btn-outline')
			.with('ghost', () => 'btn-ghost')
			.with('soft', () => 'btn-soft')
			.with('link', () => 'btn-link')
			.with('solid', () => 'btn-solid')
			.exhaustive()
	);
	const baseClass = $derived(
		cn([
			'btn-base inline-flex items-center justify-center font-medium transition-all duration-150 focus:outline-none',
			isDisabled ? 'opacity-50 cursor-not-allowed pointer-events-none' : 'cursor-pointer',
			fullWidth && !isFixedShape ? 'w-full' : '',
			sizingClass,
			shapeClass,
			styleClass,
			classes
		])
	);

	const themeVars = $derived(
		`--btn-bg: var(--color-${variant}, ${BG_FALLBACK[variant]}); --btn-ink: var(--color-${variant}-content, ${CONTENT_FALLBACK[variant]});`
	);

	const showSpinner = $derived(loading && (loadingMode === 'inline' || loadingMode === 'replace'));
	const hideContent = $derived(loading && loadingMode === 'overlay');
</script>

{#snippet spinner()}
	<Loader2 class="shrink-0 animate-spin" size="1em" />
{/snippet}

{#snippet innerContent()}
	{#if showSpinner}
		{@render spinner()}
	{/if}

	<span class="inline-flex items-center justify-center gap-[inherit]" class:invisible={hideContent}>
		{#if loading && loadingMode === 'replace' && loadingText}
			<span>{loadingText}</span>
		{:else if loading && loadingMode === 'replace' && !isFixedShape}
			{@render children?.()}
		{:else if isIconOnly}
			{@render iconOnly?.()}
		{:else}
			{#if leadingIcon}<span class="shrink-0 [&>*]:h-[1em] [&>*]:w-[1em]">
					{@render leadingIcon()}
				</span>{/if}
			{@render children?.()}
			{#if trailingIcon}<span class="shrink-0 [&>*]:h-[1em] [&>*]:w-[1em]">
					{@render trailingIcon()}
				</span>{/if}
		{/if}
	</span>

	{#if loading && loadingMode === 'overlay'}
		<span class="absolute inset-0 inline-flex items-center justify-center text-[1em]">
			{@render spinner()}
		</span>
	{/if}
{/snippet}

{#if isAnchor}
	{@const a = rest as HTMLAnchorAttributes}
	{@const opensNew = a.target === '_blank'}
	<a
		{...a}
		href={isDisabled ? undefined : a.href}
		rel={opensNew ? ['noopener', 'noreferrer', a.rel].filter(Boolean).join(' ') : a.rel}
		role={isDisabled ? 'link' : undefined}
		aria-disabled={isDisabled || undefined}
		tabindex={isDisabled ? -1 : undefined}
		class="{baseClass} relative"
		style={themeVars}
		onclick={(e) => {
			if (isDisabled) return e.preventDefault();
			a.onclick?.(e);
		}}
	>
		{@render innerContent()}
	</a>
{:else}
	{@const b = rest as HTMLButtonAttributes}
	<button
		{...b}
		type={b.type ?? 'button'}
		disabled={isDisabled}
		aria-busy={loading}
		class="{baseClass} relative"
		style={themeVars}
		onclick={b.onclick}
	>
		{@render innerContent()}
	</button>
{/if}

<style>
	.btn-base {
		border: 1px solid transparent;
	}

	/* SOLID */
	.btn-base.btn-solid {
		background-color: var(--btn-bg);
		color: var(--btn-ink);
	}
	.btn-base.btn-solid:hover:not(:disabled) {
		background-color: color-mix(in oklch, var(--btn-bg) 85%, black);
	}
	.btn-base.btn-solid:focus-visible {
		box-shadow: 0 0 0 3px color-mix(in oklch, var(--btn-bg) 30%, transparent);
	}

	/* OUTLINE */
	.btn-base.btn-outline {
		background-color: transparent;
		color: var(--btn-bg);
		border-color: var(--btn-bg);
	}
	.btn-base.btn-outline:hover:not(:disabled) {
		background-color: color-mix(in oklch, var(--btn-bg) 10%, transparent);
	}
	.btn-base.btn-outline:focus-visible {
		box-shadow: 0 0 0 3px color-mix(in oklch, var(--btn-bg) 30%, transparent);
	}

	/* GHOST */
	.btn-base.btn-ghost {
		background-color: transparent;
		color: var(--btn-bg);
	}
	.btn-base.btn-ghost:hover:not(:disabled) {
		background-color: color-mix(in oklch, var(--btn-bg) 10%, transparent);
	}

	/* SOFT */
	.btn-base.btn-soft {
		background-color: color-mix(in oklch, var(--btn-bg) 15%, transparent);
		color: var(--btn-bg);
	}
	.btn-base.btn-soft:hover:not(:disabled) {
		background-color: color-mix(in oklch, var(--btn-bg) 25%, transparent);
	}

	/* LINK */
	.btn-base.btn-link {
		background-color: transparent;
		color: var(--btn-bg);
		text-underline-offset: 2px;
	}
	.btn-base.btn-link:hover:not(:disabled) {
		text-decoration: underline;
	}
</style>
