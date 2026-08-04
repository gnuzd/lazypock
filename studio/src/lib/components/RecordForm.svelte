<script lang="ts">
	import { slide } from 'svelte/transition';
	import RichEditor from '$lib/components/RichEditor.svelte';
	import SelectField from '$lib/components/SelectField.svelte';

	import { client } from '$lib/client';

	let {
		fields,
		collections = [] as Record<string, unknown>[],
		data = $bindable({}),
		disabled = false,
		errors = {},
		editing = false,
		passwordSaving = $bindable(false),
		passwordError = $bindable(''),
		onPasswordSave
	}: {
		/** Collection schema fields (as returned from backend) */
		fields: Record<string, unknown>[];
		/** All collections list (for resolving relation targets) */
		collections?: Record<string, unknown>[];
		/** Bindable form data — mutated in-place */
		data: Record<string, unknown>;
		/** Disable all inputs (e.g. during save) */
		disabled?: boolean;
		/** Per-field error messages */
		errors?: Record<string, string>;
		/** True if editing existing record */
		editing?: boolean;
		/** True while password is being saved */
		passwordSaving?: boolean;
		/** Error message for password save */
		passwordError?: string;
		/** Called when user saves password in edit mode */
		onPasswordSave?: (password: string) => void;
	} = $props();

	// ── Relation field helpers ──
	/** Cache of fetched records per target collection */
	let relationCache = $state<Record<string, Record<string, unknown>[]>>({});
	/** Search text per relation field */
	let relationSearch = $state<Record<string, string>>({});
	/** Whether each relation field's dropdown is open */
	let relationOpen = $state<Record<string, boolean>>({});

	function resolveTargetCollection(field: Record<string, unknown>): string | null {
		const collId = field.collectionId as string | undefined;
		if (!collId) return null;
		const coll = collections.find((c) => c.id === collId);
		return (coll?.name as string) ?? null;
	}

	function getPresentableField(collName: string): string | null {
		const coll = collections.find((c) => c.name === collName);
		if (!coll) return null;
		const fields = (coll.fields as Record<string, unknown>[]) ?? [];
		// Prefer the first non-ID field marked presentable; fall back to any text/email/name-like field
		const presentable = fields.find((f) => f.presentable && f.name !== 'id');
		if (presentable) return presentable.name as string;
		const nameLike = fields.find(
			(f) => f.name === 'name' || f.name === 'title' || f.name === 'email'
		);
		if (nameLike) return nameLike.name as string;
		return null;
	}

	async function openRelationDropdown(fieldName: string, targetColl: string) {
		if (relationCache[targetColl]) {
			relationOpen[fieldName] = true;
			relationOpen = { ...relationOpen };
			return;
		}
		// Fetch records from target collection
		try {
			const result = await client
				.collection(targetColl)
				.getList(1, 200);
			relationCache[targetColl] = (result?.items ?? []) as Record<string, unknown>[];
			relationCache = { ...relationCache };
			relationOpen[fieldName] = true;
			relationOpen = { ...relationOpen };
		} catch {
			// ignore
		}
	}

	function getFilteredOptions(
		fieldName: string,
		targetColl: string
	): { value: string; label: string }[] {
		const records = relationCache[targetColl] ?? [];
		const search = (relationSearch[fieldName] ?? '').toLowerCase();
		const presentField = getPresentableField(targetColl);

		return records
			.filter((r) => {
				if (!search) return true;
				const id = (r.id as string) ?? '';
				if (id.toLowerCase().includes(search)) return true;
				if (presentField) {
					const val = (r[presentField] as string) ?? '';
					if (val.toLowerCase().includes(search)) return true;
				}
				return false;
			})
			.map((r) => ({
				value: r.id as string,
				label: presentField
					? `${r[presentField] ?? ''} (${(r.id as string)?.slice(0, 8)}...)`
					: (r.id as string)
			}));
	}

	const TEXT_INPUT_TYPES = new Set(['text', 'number', 'email', 'url', 'password']);

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

	// ── Password management state ──
	let editPwOpen = $state(false);
	let newPassword = $state('');
	let confirmPassword = $state('');

	function update(fieldName: string, value: unknown) {
		data[fieldName] = value;
		data = { ...data };
	}
</script>

<div class="record-form">
	{#each fields.filter((f) => f.type !== 'autodate' && f.name !== 'id' && f.type !== 'password') as field (field.name as string)}
		{@const options = (field.options ?? {}) as Record<string, unknown>}
		{@const type = field.type as string}
		{@const name = field.name as string}
		{@const required = !!field.required}
		{@const choices = (options?.values as string[]) || []}

		<!-- ═══ BOOLEAN ═══ -->
		{#if type === 'bool'}
			<label class="field field-bool" class:required>
				<input
					type="checkbox"
					checked={!!data[name]}
					{disabled}
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
					{disabled}
					value={data[name] != null ? JSON.stringify(data[name], null, 2) : ''}
					onchange={(e) => {
						try {
							update(name, JSON.parse((e.target as HTMLTextAreaElement).value));
						} catch {
							// keep existing value if invalid JSON
						}
					}}
					rows="4"
					placeholder={'{}'}></textarea>
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
					{disabled}
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
					{disabled}
					value={(data[name] as string) ?? ''}
					oninput={(e: Event) => update(name, (e.target as HTMLInputElement).value)}
				/>
				{#if errors[name]}
					<span class="field-error">{errors[name]}</span>
				{/if}
			</div>

			<!-- ═══ SELECT ═══ -->
		{:else if type === 'select' || type === 'multi_select'}
			{@const maxSelect = (options?.maxSelect as number) || 1}
			<div class="field" class:required>
				<label>{name}</label>
				<SelectField {choices} {maxSelect} bind:value={data[name]} {disabled} />
				{#if errors[name]}
					<span class="field-error">{errors[name]}</span>
				{/if}
			</div>

			<!-- ═══ FILE ═══ -->
		{:else if type === 'file' || type === 'multi_file'}
			<div class="field" class:required>
				<label>{name}</label>
				{#if data[name]}
					<div class="file-row">
						<span class="file-name">{String(data[name])}</span>
						<button
							type="button"
							{disabled}
							class="btn-text danger"
							onclick={() => update(name, null)}>Remove</button
						>
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

			<!-- ═══ RELATION (searchable dropdown) ═══ -->
		{:else if type === 'relation'}
			{@const targetColl = resolveTargetCollection(field)}
			<div class="field" class:required>
				<label for="f_{name}">{name}</label>
				<div class="relation-wrap">
					<input
						id="f_{name}"
						type="text"
						{disabled}
						value={(data[name] as string) ?? ''}
						placeholder={targetColl ? 'Search ' + targetColl + '...' : 'Related record ID'}
						oninput={(e: Event) => update(name, (e.target as HTMLInputElement).value)}
						onfocus={() => {
							if (targetColl) openRelationDropdown(name, targetColl);
						}}
						onblur={() => {
							setTimeout(() => {
								relationOpen[name] = false;
								relationOpen = { ...relationOpen };
							}, 200);
						}}
					/>
					{#if targetColl && relationOpen[name]}
						{@const filtered = getFilteredOptions(name, targetColl)}
						<div class="relation-dropdown">
							<input
								type="text"
								class="relation-search"
								placeholder="Type to filter..."
								value={relationSearch[name] ?? ''}
								oninput={(e: Event) => {
									relationSearch[name] = (e.target as HTMLInputElement).value;
									relationSearch = { ...relationSearch };
								}}
							/>
							<div class="relation-options">
								{#each filtered as opt}
									<button
										type="button"
										class="relation-option"
										class:active={data[name] === opt.value}
										onmousedown={() => {
											update(name, opt.value);
											relationOpen[name] = false;
											relationSearch[name] = '';
											relationOpen = { ...relationOpen };
											relationSearch = { ...relationSearch };
										}}>{opt.label}</button
									>
								{/each}
								{#if filtered.length === 0}
									<div class="relation-empty">No matching records</div>
								{/if}
							</div>
						</div>
					{/if}
				</div>
				<span class="field-help"
					>Related record{options.maxSelect && (options.maxSelect as number) > 1
						? 's (multi-select not yet supported in dropdown)'
						: ''}</span
				>
				{#if errors[name]}
					<span class="field-error">{errors[name]}</span>
				{/if}
			</div>

			<!-- ═══ EDITOR (rich text) ═══ -->
		{:else if type === 'editor'}
			<div class="field field-editor" class:required>
				<label>{name}</label>
				<RichEditor bind:value={data[name]} {disabled} />
				{#if errors[name]}
					<span class="field-error">{errors[name]}</span>
				{/if}
			</div>

			<!-- ═══ TEXT / NUMBER / EMAIL / URL ═══ -->
		{:else if isTextInput(field)}
			{@const isMulti = type === 'text' && !!field.multiline}
			{@const min = options?.min as number | undefined}
			{@const max = options?.max as number | undefined}

			<div class="field" class:required>
				<label for="f_{name}">{name}</label>
				{#if isMulti}
					<textarea
						id="f_{name}"
						{disabled}
						value={(data[name] as string) ?? ''}
						oninput={(e: Event) => update(name, (e.target as HTMLTextAreaElement).value)}
						rows="4"
						placeholder={name}></textarea>
				{:else}
					<input
						id="f_{name}"
						type={getInputType(field)}
						{required}
						{disabled}
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

	<!-- ═══ PASSWORD SECTION (separate from main fields) ═══ -->
	{#each fields.filter((f) => f.type === 'password') as field (field.name as string)}
		{@const name = field.name as string}
		<div class="field">
			<label for="pw_{name}">Password</label>

			{#if editing}
				{#if !editPwOpen}
					<button
						type="button"
						class="btn btn-ghost w-full text-left font-normal"
						style="padding: 10px 12px; font-size: 0.9375rem"
						onclick={() => (editPwOpen = true)}
					>
						<svg
							width="16"
							height="16"
							viewBox="0 0 24 24"
							fill="none"
							stroke="currentColor"
							stroke-width="2"
							stroke-linecap="round"
							stroke-linejoin="round"
							class="mr-2 inline"
							><rect x="3" y="11" width="18" height="11" rx="2" ry="2" /><path
								d="M7 11V7a5 5 0 0 1 10 0v4"
							/></svg
						>
						Change password
					</button>
				{:else}
					<div transition:slide>
						<input
							id="pw_{name}"
							type="password"
							disabled={passwordSaving}
							bind:value={newPassword}
							placeholder="New password"
						/>
						<input
							id="pw_{name}_confirm"
							type="password"
							disabled={passwordSaving}
							bind:value={confirmPassword}
							placeholder="Confirm password"
						/>
						<div class="flex items-center gap-2 px-0 pt-1 pb-1">
							<button
								type="button"
								class="btn btn-ghost btn-sm"
								onclick={() => {
									editPwOpen = false;
									newPassword = '';
									confirmPassword = '';
								}}
								disabled={passwordSaving}
							>
								Cancel
							</button>
							<button
								type="button"
								class="btn btn-primary btn-sm"
								onclick={() => onPasswordSave?.(newPassword)}
								disabled={passwordSaving ||
									!newPassword ||
									!confirmPassword ||
									newPassword !== confirmPassword}
							>
								{passwordSaving ? 'Saving...' : 'Save Password'}
							</button>
						</div>
					</div>
				{/if}
			{:else}
				<input
					id="pw_{name}"
					type="password"
					disabled={disabled || passwordSaving}
					value={(data[name] as string) ?? ''}
					oninput={(e: Event) => update(name, (e.target as HTMLInputElement).value)}
					placeholder="Password"
				/>
			{/if}

			{#if passwordError}
				<span class="field-error">{passwordError}</span>
			{:else if editing && editPwOpen && confirmPassword && newPassword !== confirmPassword}
				<span class="field-error">Passwords do not match</span>
			{/if}
		</div>
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

	/* ── Editor field: seamless integration with the RichEditor ── */
	.field-editor {
		background: color-mix(in oklab, var(--color-base-content) 8%, var(--color-base-100));
		padding: 0 0 8px;
		border: none;
		border-radius: var(--radius-field);
		overflow: hidden;
	}

	.field-editor label {
		padding: 8px 12px 6px;
		opacity: 1;
		color: var(--color-base-content);
		font-weight: 600;
	}

	.field-editor .field-error {
		padding: 4px 12px 8px;
	}

	.field-editor:focus-within {
		background: color-mix(in oklab, var(--color-base-content) 12%, var(--color-base-100));
	}

	/* ── Disabled state ── */
	.field:has(input:disabled),
	.field:has(textarea:disabled) {
		opacity: 0.5;
		pointer-events: none;
	}

	input:disabled,
	textarea:disabled {
		cursor: default;
		color: color-mix(in oklab, var(--color-base-content) 60%, transparent);
	}

	/* ── Input / Textarea / Select ── */
	.field input,
	.field textarea {
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
	.field textarea:focus {
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
		transition:
			border-color 0.2s,
			box-shadow 0.2s,
			background 0.2s;
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
		transition:
			transform 0.15s,
			opacity 0.15s;
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

	/* ── RELATION DROPDOWN ── */
	.relation-wrap {
		position: relative;
	}

	.relation-dropdown {
		position: absolute;
		top: 100%;
		left: 0;
		right: 0;
		z-index: 50;
		background: var(--color-base-100);
		border: 1px solid var(--color-base-300);
		border-radius: var(--radius-field);
		box-shadow: 0 4px 12px rgba(0, 0, 0, 0.15);
		max-height: 240px;
		display: flex;
		flex-direction: column;
		overflow: hidden;
	}

	.relation-search {
		width: 100%;
		padding: 8px 10px;
		border: none;
		border-bottom: 1px solid var(--color-base-300);
		background: transparent;
		color: var(--color-base-content);
		font-size: 0.8125rem;
		outline: none;
		box-sizing: border-box;
	}

	.relation-options {
		overflow-y: auto;
		flex: 1;
	}

	.relation-option {
		display: block;
		width: 100%;
		padding: 7px 10px;
		border: none;
		background: transparent;
		color: var(--color-base-content);
		font-size: 0.8125rem;
		cursor: pointer;
		text-align: left;
	}

	.relation-option:hover {
		background: color-mix(in oklab, var(--color-base-content) 10%, var(--color-base-100));
	}

	.relation-option.active {
		background: color-mix(in oklab, var(--color-primary) 15%, var(--color-base-100));
		color: var(--color-primary);
	}

	.relation-empty {
		padding: 12px 10px;
		font-size: 0.8125rem;
		opacity: 0.4;
		text-align: center;
	}
</style>
