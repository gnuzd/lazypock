<script lang="ts">
	let {
		value = $bindable(''),
		type = 'text',
		placeholder = '',
		label = '',
		help = '',
		error = '',
		disabled = false,
		required = false,
		name = '',
		id = '',
		autofocus = false,
		class: className = ''
	}: {
		value?: string;
		type?: string;
		placeholder?: string;
		label?: string;
		help?: string;
		error?: string;
		disabled?: boolean;
		required?: boolean;
		name?: string;
		id?: string;
		autofocus?: boolean;
		class?: string;
	} = $props();

	let inputEl: HTMLInputElement | undefined = $state();

	$effect(() => {
		if (autofocus && inputEl) {
			inputEl.focus();
		}
	});
</script>

<div class="field {className} {error ? 'error' : ''} {disabled ? 'disabled' : ''}" class:required>
	{#if label}
		<label for={id || name} class="field-label">{label}</label>
	{/if}
	<input
		bind:this={inputEl}
		{type}
		{name}
		id={id || name}
		bind:value
		{placeholder}
		{disabled}
		{required}
		class="field-input"
		oninput={(e) => {
			value = (e.target as HTMLInputElement).value;
		}}
	/>
	{#if help && !error}
		<span class="field-help">{help}</span>
	{/if}
	{#if error}
		<span class="field-help error">{error}</span>
	{/if}
</div>

<style>
	/* ── Field wrapper ── */
	.field {
		position: relative;
		display: block;
		outline: 0;
		width: 100%;
		min-width: 0;
		border-radius: var(--radius-field);
		background: var(
			--input-color,
			color-mix(in oklab, var(--color-base-content) 8%, var(--color-base-100))
		);
		transition: background var(--animation-speed, 0.2s);
	}

	.field:focus-within {
		background: var(
			--input-focus-color,
			color-mix(in oklab, var(--color-base-content) 12%, var(--color-base-100))
		);
	}

	.field.required label::after {
		vertical-align: top;
		content: '*';
		color: var(--color-error);
		font-size: 0.75em;
		line-height: 1;
		margin: -5px 0 0 -2px;
	}

	.field.disabled {
		opacity: 0.5;
		pointer-events: none;
	}

	/* ── Label ── */
	.field-label {
		display: flex;
		width: 100%;
		gap: 5px;
		line-height: 1;
		align-items: center;
		align-self: center;
		min-height: 24px;
		padding: 9px var(--input-padding, 12px) 1px;
		font-weight: bold;
		white-space: normal;
		color: var(--color-base-hint, var(--color-base-content));
		opacity: 0.7;
		font-size: var(--font-size-sm, 0.875rem);
		transition: color var(--animation-speed, 0.2s);
	}

	/* ── Input ── */
	.field-input {
		display: inline-block;
		vertical-align: top;
		outline: 0;
		border: 0;
		margin: 0;
		width: 100%;
		background: none;
		font-weight: normal;
		line-height: 1;
		letter-spacing: inherit;
		padding: 10px var(--input-padding, 12px);
		color: var(--color-base-content);
		font-size: var(--font-size-base, 0.9375rem);
		font-family: var(--font-sans, system-ui, sans-serif);
		align-self: stretch;
	}

	.field-input::placeholder {
		user-select: none;
		color: var(
			--color-base-disabled,
			color-mix(in oklab, var(--color-base-content) 40%, transparent)
		);
		font-weight: inherit;
		font-family: inherit;
	}

	.field-input:focus,
	.field-input:focus-visible,
	.field-input:focus-within {
		outline: 0;
	}

	.field-input:autofill {
		background: none;
		-webkit-text-fill-color: var(--color-base-content);
		box-shadow: 0 0 0 50px var(--input-color) inset;
		transition: box-shadow var(--animation-speed, 0.2s);
	}

	.field-input:focus:autofill {
		box-shadow: 0 0 0 50px var(--input-focus-color) inset;
	}

	/* ── Help text ── */
	.field-help {
		display: block;
		width: 100%;
		margin: 7px 0 0;
		font-size: var(--font-size-sm, 0.875rem);
		line-height: var(--line-height-sm, 1.4);
		color: var(--color-base-hint, var(--color-base-content));
		opacity: 0.7;
		padding: 0 var(--input-padding, 12px) 8px;
	}

	.field-help.error {
		color: var(--color-error);
		opacity: 1;
	}

	/* ── Error state ── */
	.field.error {
		--input-color: color-mix(in srgb, var(--color-error), var(--color-base-100) 80%);
		--input-focus-color: color-mix(in srgb, var(--color-error), var(--color-base-100) 75%);
	}

	.field.error .field-label,
	.field.error .field-input {
		color: var(--color-error) !important;
	}
</style>
