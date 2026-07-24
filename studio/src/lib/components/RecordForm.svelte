<script lang="ts">
	let {
		fields,
		data = $bindable({}),
		disabled = false,
		errors = {}
	}: {
		/** Collection schema fields (as returned from backend) */
		fields: Record<string, unknown>[];
		/** Bindable form data — mutated in-place */
		data: Record<string, unknown>;
		/** Disable all inputs (e.g. during save) */
		disabled?: boolean;
		/** Per-field error messages */
		errors?: Record<string, string>;
	} = $props();

	const TEXT_INPUT_TYPES = new Set([
		'text', 'number', 'email', 'url', 'editor', 'password'
	]);

	function isTextInput(f: Record<string, unknown>): boolean {
		return TEXT_INPUT_TYPES.has(f.type as string);
	}

	function getInputType(f: Record<string, unknown>): string {
		const t = f.type as string;
		if (t === 'number') return 'number';
		if (t === 'email') return 'email';
		if (t === 'url') return 'url';
		if (t === 'password') return 'password';
		return 'text';
	}

	function update(fieldName: string, value: unknown) {
		data[fieldName] = value;
		// Trigger reactivity for $bindable
		data = { ...data };
	}
</script>

<div class="record-form">
	{#each fields.filter((f) => !f.system) as field (field.name as string)}
		{@const options = (field.options ?? {}) as Record<string, unknown>}
		{@const type = field.type as string}
		{@const name = field.name as string}
		{@const required = !!field.required}
		{@const choices = ((options?.values as string[]) || [])}

		<!-- ═══ BOOLEAN ═══ -->
		{#if type === 'bool'}
			<label class="field field-bool" class:required>
				<input
					type="checkbox"
					checked={!!data[name]}
					disabled={disabled}
					onchange={(e) => update(name, (e.target as HTMLInputElement).checked)}
				/>
				<span class="check-label">{name}</span>
			</label>

		<!-- ═══ JSON / GEO ═══ -->
		{:else if type === 'json' || type === 'geo'}
			<div class="field" class:required>
				<label for="f_{name}">{name}</label>
				<textarea
					id="f_{name}"
					disabled={disabled}
					value={data[name] != null ? JSON.stringify(data[name], null, 2) : ''}
					onchange={(e) => {
						try {
							update(name, JSON.parse((e.target as HTMLTextAreaElement).value));
						} catch {
							// keep existing value if invalid JSON
						}
					}}
					rows="4"
					placeholder={'{}'}
				></textarea>
					<span class="field-help">Valid JSON</span>
				{#if errors[name]}
					<span class="field-error">{errors[name]}</span>
				{/if}
			</div>

		<!-- ═══ DATE ═══ -->
		{:else if type === 'date'}
			<div class="field" class:required>
				<label for="f_{name}">{name}</label>
				<input
					id="f_{name}"
					type="date"
					disabled={disabled}
					value={(data[name] as string) ?? ''}
					oninput={(e: Event) => update(name, (e.target as HTMLInputElement).value)}
					placeholder="YYYY-MM-DD"
				/>
				{#if errors[name]}
					<span class="field-error">{errors[name]}</span>
				{/if}
			</div>

		<!-- ═══ DATETIME ═══ -->
		{:else if type === 'datetime' || type === 'autodate'}
			<div class="field" class:required>
				<label for="f_{name}">{name}</label>
				<input
					id="f_{name}"
					type="datetime-local"
					disabled={disabled}
					value={(data[name] as string) ?? ''}
					oninput={(e: Event) => update(name, (e.target as HTMLInputElement).value)}
				/>
				{#if errors[name]}
					<span class="field-error">{errors[name]}</span>
				{/if}
			</div>

		<!-- ═══ SELECT (single if maxSelect <= 1, multi otherwise) ═══ -->
		{:else if type === 'select'}
			{@const maxSelect = ((options?.maxSelect as number) || 1)}
			{#if maxSelect > 1}
				{@const selected = (data[name] as string[]) || []}
				<div class="field" class:required>
					<label>{name}</label>
					<div class="multi-select-chips">
						{#each choices as choice (choice)}
							<button
								type="button"
								disabled={disabled}
								class="chip"
								class:selected={selected.includes(choice)}
								onclick={() => {
									if (selected.includes(choice)) {
										update(name, selected.filter((s) => s !== choice));
									} else {
										update(name, [...selected, choice]);
									}
								}}
							>
								{choice}
							</button>
						{/each}
					</div>
				{#if errors[name]}
					<span class="field-error">{errors[name]}</span>
				{/if}
			</div>
			{:else}
				<div class="field" class:required>
					<label for="f_{name}">{name}</label>
					<select
							id="f_{name}"
							disabled={disabled}
							value={(data[name] as string) ?? ''}
							onchange={(e) => update(name, (e.target as HTMLSelectElement).value)}
					>
						<option value="">—</option>
						{#each choices as choice (choice)}
							<option value={choice}>{choice}</option>
						{/each}
					</select>
					{#if errors[name]}
						<span class="field-error">{errors[name]}</span>
					{/if}
				</div>
			{/if}

		<!-- ═══ FILE ═══ -->
		{:else if type === 'file' || type === 'multi_file'}
			<div class="field" class:required>
				<label>{name}</label>
				{#if data[name]}
					<div class="file-row">
						<span class="file-name">{String(data[name])}</span>
						<button
							type="button"
							disabled={disabled}
							class="btn-text danger"
							onclick={() => update(name, null)}
						>Remove</button>
					</div>
				{:else}
					<div class="file-placeholder">
						<span>Upload file</span>
					</div>
				{/if}
				<span class="field-help">File upload (drag & drop or click)</span>
			{#if errors[name]}
				<span class="field-error">{errors[name]}</span>
			{/if}
		</div>

		<!-- ═══ RELATION ═══ -->
		{:else if type === 'relation'}
			<div class="field" class:required>
				<label for="f_{name}">{name}</label>
				<input
					id="f_{name}"
					type="text"
					disabled={disabled}
					value={(data[name] as string) ?? ''}
					oninput={(e: Event) => update(name, (e.target as HTMLInputElement).value)}
					placeholder="Related record ID"
				/>
				<span class="field-help">Related record ID{options.maxSelect && (options.maxSelect as number) > 1 ? 's (comma-separated)' : ''}</span>
			{#if errors[name]}
				<span class="field-error">{errors[name]}</span>
			{/if}
		</div>

		<!-- ═══ TEXT / NUMBER / EMAIL / URL / EDITOR / PASSWORD ═══ -->
		{:else if isTextInput(field)}
			{@const isMulti = type === 'editor' || type === 'text'}
			{@const min = options?.min as number | undefined}
			{@const max = options?.max as number | undefined}

			<div class="field" class:required>
				<label for="f_{name}">{name}</label>
				{#if isMulti}
					<textarea
						id="f_{name}"
						disabled={disabled}
						value={(data[name] as string) ?? ''}
						oninput={(e: Event) => update(name, (e.target as HTMLTextAreaElement).value)}
						rows="4"
						placeholder={name}
					></textarea>
				{:else}
					<input
						id="f_{name}"
						type={getInputType(field)}
						required={required}
						disabled={disabled}
						value={(data[name] as string) ?? ''}
						oninput={(e: Event) => update(name, (e.target as HTMLInputElement).value)}
						placeholder={name}
					/>
				{/if}
				{#if min != null || max != null}
					<span class="field-help">Min: {min ?? '—'}, Max: {max ?? '—'}</span>
				{/if}
				{#if errors[name]}
					<span class="field-error">{errors[name]}</span>
				{/if}
			</div>
		{/if}
	{/each}
</div>

<style>
	/* ── Container ── */
	.record-form {
		display: flex;
		flex-direction: column;
		gap: 8px;
	}

	/* ── Field wrapper ── */
	.field {
		position: relative;
		display: block;
		outline: 0;
		width: 100%;
		min-width: 0;
		border-radius: var(--radius-field);
		background: color-mix(in oklab, var(--color-base-content) 8%, var(--color-base-100));
		transition: background var(--animation-speed, 0.2s);
	}

	.field:focus-within {
		background: color-mix(in oklab, var(--color-base-content) 12%, var(--color-base-100));
	}

	.field.required label::after {
		vertical-align: top;
		content: '*';
		color: var(--color-error);
		font-size: 0.75em;
		line-height: 1;
		margin: -5px 0 0 -2px;
	}

	/* ── Label ── */
	label {
		display: flex;
		width: 100%;
		gap: 5px;
		line-height: 1;
		align-items: center;
		align-self: center;
		min-height: 24px;
		padding: 9px 12px 1px;
		font-weight: bold;
		white-space: normal;
		color: var(--color-base-content);
		opacity: 0.7;
		font-size: 0.875rem;
		transition: color var(--animation-speed, 0.2s);
	}

	.field:focus-within label {
		opacity: 1;
		color: var(--color-base-content);
	}

	/* ── Disabled state ── */
	.field:has(input:disabled),
	.field:has(textarea:disabled),
	.field:has(select:disabled) {
		opacity: 0.5;
		pointer-events: none;
	}

	input:disabled,
	textarea:disabled,
	select:disabled {
		cursor: default;
		color: color-mix(in oklab, var(--color-base-content) 60%, transparent);
	}

	/* ── Input / Textarea / Select ── */
	.field input,
	.field textarea,
	.field select {
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
		padding: 10px 12px;
		color: var(--color-base-content);
		font-size: 0.9375rem;
		font-family: system-ui, sans-serif;
		align-self: stretch;
	}

	.field input::placeholder,
	.field textarea::placeholder {
		user-select: none;
		color: color-mix(in oklab, var(--color-base-content) 40%, transparent);
		font-weight: inherit;
		font-family: inherit;
	}

	.field input:focus,
	.field textarea:focus,
	.field select:focus {
		outline: 0;
	}

	/* Remove number input spinners */
	input[type='number']::-webkit-outer-spin-button,
	input[type='number']::-webkit-inner-spin-button {
		-webkit-appearance: none;
		margin: 0;
	}
	input[type='number'] {
		-moz-appearance: textfield;
	}

	/* Textarea */
	.field textarea {
		padding-top: 8px;
		padding-bottom: 9px;
		line-height: 1.5;
		resize: vertical;
		min-height: 38px;
		max-height: 300px;
	}

	/* Select */
	.field select {
		cursor: pointer;
		appearance: none;
		-webkit-appearance: none;
		padding-right: 28px;
		background-image: url("data:image/svg+xml,%3Csvg width='16' height='16' viewBox='0 0 24 24' fill='none' stroke='%23666' stroke-width='2'%3E%3Cpolyline points='6 9 12 15 18 9'/%3E%3C/svg%3E");
		background-repeat: no-repeat;
		background-position: right 8px center;
		background-size: 16px;
	}

	/* ── Help text ── */
	.field-help {
		display: block;
		width: 100%;
		margin: 3px 0 0;
		font-size: 0.875rem;
		line-height: 1.4;
		color: var(--color-base-content);
		opacity: 0.7;
		padding: 0 12px 8px;
	}

	/* ── Field error text ── */
	.field-error {
		display: block;
		width: 100%;
		margin: 2px 0 0;
		font-size: 0.8125rem;
		line-height: 1.3;
		color: var(--color-error);
		padding: 0 12px 8px;
	}

	/* ── BOOL: custom checkbox ── */
	.field-bool {
		--checkboxSize: 20px;
		display: flex;
		gap: 10px;
		align-items: center;
		padding: 0;
		background: none;
		cursor: pointer;
		min-height: 38px;
	}

	.field-bool:focus-within {
		background: none;
	}

	.field-bool input {
		position: absolute;
		height: 1px;
		width: 1px;
		opacity: 0;
	}

	.check-label {
		display: flex;
		align-items: center;
		gap: 8px;
		margin: 0;
		padding: 0 0 0 calc(var(--checkboxSize) + 7px);
		width: auto;
		background: none;
		user-select: none;
		cursor: pointer;
		font-weight: normal;
		color: var(--color-base-content);
		font-size: 0.9375rem;
		line-height: 1.4;
		min-height: var(--checkboxSize);
		position: relative;
		opacity: 1;
	}

	.check-label::before {
		content: '';
		display: block;
		position: absolute;
		left: 0;
		top: 0;
		width: var(--checkboxSize);
		height: var(--checkboxSize);
		border-radius: var(--radius-field);
		background: color-mix(in oklab, var(--color-base-content) 8%, var(--color-base-100));
		border: 2px solid color-mix(in oklab, var(--color-base-content) 20%, transparent);
		transition: border-color 0.2s, box-shadow 0.2s, background 0.2s;
		box-sizing: border-box;
	}

	.check-label::after {
		content: '';
		display: flex;
		align-items: center;
		justify-content: center;
		position: absolute;
		z-index: 1;
		left: 0;
		top: 0;
		width: var(--checkboxSize);
		height: var(--checkboxSize);
		text-align: center;
		font-size: 1.15rem;
		font-weight: normal;
		color: var(--color-success);
		opacity: 0;
		transition: transform 0.15s, opacity 0.15s;
		box-sizing: border-box;
	}

	input:checked ~ .check-label::after {
		content: '✓';
		opacity: 1;
		transform: scale(1);
	}

	input:checked ~ .check-label::before {
		border-color: var(--color-success);
		background: color-mix(in oklab, var(--color-success) 15%, var(--color-base-100));
	}

	/* ── MULTI_SELECT chips ── */
	.multi-select-chips {
		display: flex;
		flex-wrap: wrap;
		gap: 4px;
		padding: 6px 12px 8px;
	}

	.chip {
		display: inline-flex;
		align-items: center;
		padding: 4px 10px;
		font-size: 0.8125rem;
		border-radius: 999px;
		border: 1px solid color-mix(in oklab, var(--color-base-content) 15%, transparent);
		background: color-mix(in oklab, var(--color-base-content) 6%, var(--color-base-100));
		color: var(--color-base-content);
		cursor: pointer;
		transition: background 0.15s, border-color 0.15s;
		outline: 0;
	}

	.chip.selected {
		background: color-mix(in oklab, var(--color-primary) 20%, var(--color-base-100));
		border-color: var(--color-primary);
		color: var(--color-primary);
	}

	.chip:hover {
		background: color-mix(in oklab, var(--color-primary) 10%, var(--color-base-100));
	}

	/* ── FILE ── */
	.file-row {
		display: flex;
		align-items: center;
		gap: 8px;
		padding: 8px 12px;
	}

	.file-name {
		font-size: 0.875rem;
		opacity: 0.6;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		max-width: 180px;
	}

	.btn-text {
		background: none;
		border: none;
		cursor: pointer;
		font-size: 0.8125rem;
		padding: 2px 4px;
		outline: 0;
	}
	.btn-text.danger {
		color: var(--color-error);
	}

	.file-placeholder {
		padding: 8px 12px;
		font-size: 0.875rem;
		opacity: 0.4;
	}
</style>
