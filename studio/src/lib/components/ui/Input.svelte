<script lang="ts" generics="T extends string | number">
	import type { Snippet } from 'svelte';
	import type { HTMLInputAttributes, HTMLTextareaAttributes } from 'svelte/elements';
	import { Eye, EyeOff, X, AlertCircle } from '@lucide/svelte';

	type Size = 'sm' | 'md' | 'lg';
	type InputType = 'text' | 'email' | 'password' | 'number' | 'search' | 'tel' | 'url' | 'textarea';

	type Props = Omit<HTMLInputAttributes, 'size' | 'value'> &
		Omit<HTMLTextareaAttributes, 'size' | 'value'> & {
			value?: T | null;
			type?: InputType;
			size?: Size;
			label?: string;
			hint?: string;
			error?: string;
			clearable?: boolean;
			leadingIcon?: Snippet;
			trailingIcon?: Snippet;
			classes?: {
				root?: string;
				label?: string;
				wrapper?: string;
				input?: string;
				actions?: string;
				hint?: string;
				error?: string;
			};
			vars?: Record<string, string>;
			id?: string;
		};

	let {
		value = $bindable(null),
		type = 'text',
		size = 'md',
		label,
		hint,
		error,
		clearable = false,
		disabled = false,
		readonly = false,
		leadingIcon,
		trailingIcon,
		classes = {},
		vars = {},
		id = `input-${Math.random().toString(36).slice(2, 9)}`,
		class: className = '',
		...rest
	}: Props = $props();

	let inputEl: HTMLInputElement | HTMLTextAreaElement | null = $state(null);
	let showPassword = $state(false);

	const isTextarea = $derived(type === 'textarea');
	const isPassword = $derived(type === 'password');
	const hasError = $derived(!!error);
	const isDisabled = $derived(disabled || readonly);
	const actualType = $derived(isPassword && showPassword ? 'text' : type);

	const varsStyle = $derived(
		Object.entries(vars)
			.map(([k, v]) => `${k}: ${v}`)
			.join('; ')
	);

	function clear() {
		value = null;
		if (inputEl) {
			inputEl.value = '';
			inputEl.focus();
			inputEl.dispatchEvent(new Event('input', { bubbles: true }));
		}
	}

	function focus() {
		inputEl?.focus();
	}
	export { focus };
</script>

<div
	class="input-root {classes.root ?? ''} {className}"
	class:disabled={isDisabled}
	style={varsStyle}
>
	{#if label}
		<label class="input-label {classes.label ?? ''}" for={id}>{label}</label>
	{/if}

	<div
		class="input-wrapper {classes.wrapper ?? ''}"
		class:sm={size === 'sm'}
		class:md={size === 'md'}
		class:lg={size === 'lg'}
		class:textarea={isTextarea}
		class:error={hasError}
	>
		{#if leadingIcon}
			<div class="input-icon leading">{@render leadingIcon()}</div>
		{/if}

		{#if isTextarea}
			<textarea
				bind:this={inputEl}
				{id}
				class="input-field {classes.input ?? ''}"
				{disabled}
				{readonly}
				aria-invalid={hasError}
				{...rest as HTMLTextareaAttributes}>{value}</textarea
			>
		{:else}
			<input
				bind:this={inputEl}
				{id}
				type={actualType}
				class="input-field {classes.input ?? ''}"
				bind:value
				{disabled}
				{readonly}
				aria-invalid={hasError}
				{...rest as HTMLInputAttributes}
			/>
		{/if}

		<div class="input-actions {classes.actions ?? ''}">
			{#if clearable && value && !isDisabled}
				<button type="button" class="input-action-btn" onclick={clear} aria-label="Clear input">
					<X size={14} strokeWidth={2} />
				</button>
			{/if}

			{#if isPassword}
				<button
					type="button"
					class="input-action-btn"
					onclick={() => (showPassword = !showPassword)}
					aria-label={showPassword ? 'Hide password' : 'Show password'}
				>
					{#if showPassword}
						<EyeOff size={14} strokeWidth={2} />
					{:else}
						<Eye size={14} strokeWidth={2} />
					{/if}
				</button>
			{/if}

			{#if trailingIcon}
				<div class="input-icon trailing">{@render trailingIcon()}</div>
			{/if}
		</div>
	</div>

	{#if error}
		<div class="input-error {classes.error ?? ''}">
			<AlertCircle size={12} />
			<span>{error}</span>
		</div>
	{:else if hint}
		<div class="input-hint {classes.hint ?? ''}">{hint}</div>
	{/if}
</div>

<style>
	.input-root {
		display: flex;
		flex-direction: column;
		gap: 6px;
		width: 100%;
		color: var(--color-base-content, #0f172a);
		font-family: inherit;

		/* Local overrides mapped directly to theme */
		--input-bg: var(--color-base-100, #fff);
		--input-border: var(--color-base-300, #e5e7eb);
		--input-accent: var(--color-primary, #2563eb);
		--input-error: var(--color-error, #dc2626);
		--input-radius: var(--radius-field, 1rem);
	}

	.input-label {
		font-size: 12px;
		font-weight: 600;
		color: color-mix(in oklch, var(--color-base-content, #0f172a) 60%, transparent);
	}

	.input-wrapper {
		display: flex;
		align-items: center;
		background: var(--input-bg);
		border: 1px solid var(--input-border);
		border-radius: var(--input-radius);
		transition:
			border-color 0.15s,
			box-shadow 0.15s;
		overflow: hidden;
		box-sizing: border-box;
		height: 40px;
		padding: 0 12px;
	}

	.input-wrapper.sm {
		height: 32px;
		padding: 0 10px;
		gap: 6px;
	}
	.input-wrapper.md {
		height: 40px;
		padding: 0 12px;
		gap: 8px;
	}
	.input-wrapper.lg {
		height: 48px;
		padding: 0 14px;
		gap: 10px;
	}

	.input-wrapper.error {
		border-color: var(--input-error);
	}
	.input-wrapper.error:focus-within {
		box-shadow: 0 0 0 3px color-mix(in oklch, var(--input-error) 20%, transparent);
	}

	.input-root.disabled {
		opacity: 0.6;
	}
	.input-root.disabled .input-wrapper {
		background: var(--color-base-200, #f1f2f5);
		cursor: not-allowed;
	}

	.input-field {
		flex: 1;
		min-width: 0;
		border: none;
		background: transparent;
		color: var(--color-base-content, #0f172a);
		outline: none;
		padding: 0;
		font-family: inherit;
		height: 100%;
	}
	.input-field::placeholder {
		color: color-mix(in oklch, var(--color-base-content, #0f172a) 40%, transparent);
	}
	.input-field:disabled {
		cursor: not-allowed;
	}

	.input-wrapper.sm .input-field {
		font-size: 12px;
	}
	.input-wrapper.md .input-field {
		font-size: 13px;
	}
	.input-wrapper.lg .input-field {
		font-size: 14px;
	}

	/* Textarea overrides */
	.input-wrapper.textarea {
		height: auto;
		align-items: flex-start;
		padding: 10px 12px;
	}
	.input-wrapper.textarea .input-field {
		resize: vertical;
		min-height: 80px;
		padding: 0;
		line-height: 1.5;
		font-size: 13px;
	}

	.input-icon {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		color: color-mix(in oklch, var(--color-base-content, #0f172a) 60%, transparent);
		flex-shrink: 0;
	}
	.input-icon svg {
		width: 16px;
		height: 16px;
	}

	.input-actions {
		display: inline-flex;
		align-items: center;
		gap: 2px;
		flex-shrink: 0;
	}
	.input-action-btn {
		display: inline-grid;
		place-items: center;
		width: 24px;
		height: 24px;
		border-radius: 4px;
		border: none;
		background: transparent;
		color: color-mix(in oklch, var(--color-base-content, #0f172a) 60%, transparent);
		cursor: pointer;
		transition:
			background-color 0.15s,
			color 0.15s;
	}
	.input-action-btn:hover {
		background: var(--color-base-200, #f1f2f5);
		color: var(--color-base-content, #0f172a);
	}

	.input-hint {
		font-size: 11px;
		color: color-mix(in oklch, var(--color-base-content, #0f172a) 60%, transparent);
	}
	.input-error {
		display: flex;
		align-items: center;
		gap: 4px;
		font-size: 11px;
		color: var(--input-error);
	}
</style>
