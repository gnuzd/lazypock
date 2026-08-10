<script lang="ts">
	import { slide } from 'svelte/transition';
	import RichEditor from '$lib/components/RichEditor.svelte';
	import SelectField from '$lib/components/SelectField.svelte';

	import { client } from '$lib/client';
	import Modal from '$lib/components/Modal.svelte';
	import { getFileUrl, getThumbUrl, type FileRecord } from 'lazypock';

	let {
		fields,
		collections = [] as Record<string, unknown>[],
		collectionName = '',
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
		/** Current collection name (for file-ownership metadata). */
		collectionName?: string;
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
		if (collId) {
			const coll = collections.find((c) => c.id === collId);
			if (coll) return (coll.name as string) ?? null;
		}
		// Fall back to options.collection (name) — pre-existing data may not have
		// collectionId set.
		const opts = (field.options as Record<string, unknown>) || {};
		const collName = opts['collection'] as string | undefined;
		if (collName && collections.some((c) => c.name === collName)) return collName;
		return null;
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
			const result = await client.collection(targetColl).getList(1, 200);
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

	/** Human-readable label for a related record (presentable field, name/email, or id). */
	function recordLabel(rec: Record<string, unknown>, targetColl: string): string {
		const presentField = getPresentableField(targetColl);
		const val = presentField ? ((rec[presentField] as string) ?? '') : '';
		const id = (rec.id as string) ?? '';
		if (val) return `${val} (${id.slice(0, 8)}...)`;
		return id;
	}

	/** Current value(s) for a field as an array of ids. */
	function recordValue(fieldName: string): string[] {
		const v = data[fieldName];
		if (Array.isArray(v)) return (v as string[]).filter(Boolean);
		return v ? [v as string] : [];
	}

	/** Labels for the currently selected value(s). */
	function selectedLabels(fieldName: string, targetColl: string): string[] {
		const records = relationCache[targetColl] ?? [];
		const selected = recordValue(fieldName);
		if (selected.length === 0) return [];
		return selected.map((id) => {
			const rec = records.find((r) => r.id === id);
			return rec ? recordLabel(rec, targetColl) : id;
		});
	}

	async function toggleRelationOption(fieldName: string, value: string, maxSelect: number) {
		if (maxSelect > 1) {
			const cur = recordValue(fieldName);
			if (cur.includes(value)) {
				update(fieldName, cur.length > 1 ? cur.filter((v) => v !== value) : null);
			} else {
				update(fieldName, [...cur, value]);
			}
		} else {
			update(fieldName, value);
			relationOpen[fieldName] = false;
			relationSearch[fieldName] = '';
			relationOpen = { ...relationOpen };
			relationSearch = { ...relationSearch };
		}
	}

	function removeRelationValue(fieldName: string, value: string) {
		const cur = recordValue(fieldName);
		if (cur.length <= 1) {
			update(fieldName, null);
		} else {
			update(fieldName, cur.filter((v) => v !== value));
		}
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

	// ── File upload state ──
	let fileUploading = $state<Record<string, boolean>>({});
	let fileError = $state<Record<string, string>>({});
	/** Uploaded file metadata keyed by file ID (for display of name/url/thumb). */
	let fileMeta = $state<
		Record<string, { filename: string; url: string; thumbs?: Record<string, string> }>
	>({});

	function rememberFile(
		fileId: string,
		filename: string,
		url: string,
		thumbs?: Record<string, string>
	) {
		fileMeta[fileId] = { filename, url, thumbs };
		fileMeta = { ...fileMeta };
	}

	/**
	 * Upload a file via the SDK's FilesService and store its ID in the
	 * record's file/multi_file field. Uploads immediately on selection.
	 */
	async function uploadFile(fieldName: string, rawFiles: FileList | null) {
		if (!rawFiles || rawFiles.length === 0) return;
		const files = Array.from(rawFiles);
		fileUploading[fieldName] = true;
		fileError[fieldName] = '';
		fileUploading = { ...fileUploading };

		const field = fields.find((f) => f.name === fieldName);
		const recordId = editing ? ((data['id'] as string) ?? undefined) : undefined;

		try {
			const uploaded: string[] = [];
			for (const f of files) {
				const res = await client.files.upload(f, f.name, undefined, {
					collectionName,
					fieldName,
					recordId
				});
				if (res?.id) {
					uploaded.push(res.id);
					rememberFile(res.id, res.filename, res.url, res.thumbs);
				}
			}

			if (uploaded.length === 0) throw new Error('Upload failed — no file returned');
			const isMulti = (field?.type as string) === 'multi_file';
			if (isMulti) {
				update(fieldName, [...recordValue(fieldName), ...uploaded]);
			} else {
				update(fieldName, uploaded[0]);
			}
		} catch (e) {
			fileError[fieldName] = (e as Error).message || 'Upload failed';
		} finally {
			fileUploading[fieldName] = false;
			fileUploading = { ...fileUploading };
		}
	}

	// ── File library picker state ──
	let pickerOpen = $state(false);
	let pickerField = $state('');
	let pickerItems = $state<FileRecord[]>([]);
	let pickerLoading = $state(false);
	let pickerError = $state('');

	async function openPicker(fieldName: string) {
		pickerField = fieldName;
		pickerOpen = true;
		pickerError = '';
		pickerLoading = true;
		try {
			const res = await client.files.list({ mime: 'image/', perPage: 200 });
			pickerItems = res?.items ?? [];
		} catch (e) {
			pickerError = (e as Error).message || 'Failed to load library';
			pickerItems = [];
		} finally {
			pickerLoading = false;
		}
	}

	function closePicker() {
		pickerOpen = false;
		pickerField = '';
		pickerItems = [];
	}

	function pickFile(fileId: string) {
		const isMulti =
			pickerField && fields.find((f) => f.name === pickerField)?.type === 'multi_file';
		if (isMulti) {
			const cur = recordValue(pickerField);
			if (!cur.includes(fileId)) update(pickerField, [...cur, fileId]);
		} else {
			update(pickerField, fileId);
		}
		closePicker();
	}

	async function deleteFromLibrary(fileId: string) {
		if (!confirm('Delete this file permanently? This cannot be undone.')) return;
		try {
			await client.files.delete(fileId);
			pickerItems = pickerItems.filter((f) => f.id !== fileId);
		} catch (e) {
			pickerError = (e as Error).message || 'Delete failed';
		}
	}

	async function removeFile(fieldName: string, fileId: string) {
		// Unlink-only (PocketBase model): the physical file stays in the library.
		// Delete from the library (permanent) happens via the picker's delete button.
		const current = recordValue(fieldName);
		if (current.length > 0 && current[0] === fileId && !Array.isArray(data[fieldName])) {
			update(fieldName, null);
		} else {
			update(
				fieldName,
				current.filter((id) => id !== fileId)
			);
		}
	}

	function displayUrl(fileId: string): string {
		return fileMeta[fileId]?.url ?? getFileUrl('/api', fileId);
	}

	/** Pick the smallest thumbnail URL for a file, or null if none. */
	function thumbUrl(fileId: string, fieldName?: string): string | null {
		const thumbs = fileMeta[fileId]?.thumbs;
		if (thumbs && Object.keys(thumbs).length > 0) {
			const size = Object.keys(thumbs).sort((a, b) => a.length - b.length)[0];
			return thumbs[size] ?? getThumbUrl('/api', fileId, size);
		}
		// Pre-existing file (no upload meta): derive from the field's configured
		// thumb sizes in the collection schema — the URL is deterministic.
		const field = fields.find((f) => f.name === fieldName);
		const opts = (field?.options ?? {}) as Record<string, unknown>;
		const sizes = Array.isArray(opts.thumbs) ? (opts.thumbs as string[]) : [];
		if (sizes.length === 0) return null;
		const smallest = [...sizes].sort((a, b) => a.length - b.length)[0];
		return getThumbUrl('/api', fileId, smallest);
	}

	/** True when the file is an image (from mime or filename extension). */
	function isImageFile(fileId: string): boolean {
		const meta = fileMeta[fileId];
		const fn = (meta?.filename ?? '').toLowerCase();
		return /(\.png|\.jpe?g|\.gif|\.webp|\.svg)$/.test(fn);
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
			{@const isMulti = type === 'multi_file'}
			{@const currentFiles = recordValue(name)}
			<div class="field" class:required>
				<label>{name}</label>

				{#if currentFiles.length > 0}
					<div class="file-list">
						{#each currentFiles as fileId (fileId)}
							{@const turl = thumbUrl(fileId, name)}
							<div class="file-row">
								<a
									href={displayUrl(fileId)}
									target="_blank"
									rel="noopener noreferrer"
									class="file-name file-link"
								>
									{#if turl && isImageFile(fileId)}
										<img src={turl} alt={fileMeta[fileId]?.filename ?? fileId} class="file-thumb" />
									{/if}
									<span>{fileMeta[fileId]?.filename ?? fileId}</span>
								</a>
								<button
									type="button"
									{disabled}
									class="btn-text danger"
									onclick={() => removeFile(name, fileId)}>Remove</button
								>
							</div>
						{/each}
					</div>
				{/if}

				<div class="file-actions">
					<label class="file-upload" class:disabled={disabled || fileUploading[name]}>
						<input
							type="file"
							{disabled}
							{...isMulti ? { multiple: true } : {}}
							onchange={(e) => uploadFile(name, (e.target as HTMLInputElement).files)}
						/>
						<span
							>{fileUploading[name]
								? 'Uploading…'
								: currentFiles.length
									? 'Add file'
									: 'Upload file'}</span
						>
					</label>
					<button type="button" class="btn-text" {disabled} onclick={() => openPicker(name)}
						>Library</button
					>
				</div>

				<span class="field-help"
					>File upload ({isMulti ? 'multi-file' : 'single-file'}) — click to choose, or pick from
					Library</span
				>
				{#if fileError[name]}
					<span class="field-error">{fileError[name]}</span>
				{/if}
				{#if errors[name]}
					<span class="field-error">{errors[name]}</span>
				{/if}
			</div>

			<!-- ═══ RELATION (searchable dropdown) ═══ -->
		{:else if type === 'relation'}
			{@const targetColl = resolveTargetCollection(field)}
			{@const isMultiRel = (options?.maxSelect as number) > 1}
			<div class="field" class:required>
				<label for="f_{name}">{name}</label>
				<div class="relation-wrap">
					{#if targetColl}
						<!-- Button-style trigger showing the selected label(s) -->
						<div
							class="relation-trigger"
							class:open={relationOpen[name]}
							role="button"
							tabindex="0"
							{...disabled ? { 'aria-disabled': 'true' } : {}}
							onclick={() => {
								if (disabled) return;
								if (relationOpen[name]) {
									relationOpen[name] = false;
									relationOpen = { ...relationOpen };
								} else {
									openRelationDropdown(name, targetColl);
								}
							}}
							onkeydown={(e: KeyboardEvent) => {
								if (e.key === 'Enter' || e.key === ' ') {
									e.preventDefault();
									if (!disabled) {
										if (relationOpen[name]) {
											relationOpen[name] = false;
											relationOpen = { ...relationOpen };
										} else {
											openRelationDropdown(name, targetColl);
										}
									}
								}
							}}
						>
							{#if (selectedLabels(name, targetColl).length > 0)}
								<div class="relation-trigger-labels">
									{#each selectedLabels(name, targetColl) as lbl, li (lbl)}
										<span class="relation-chip">
											{lbl}
											{#if !disabled && isMultiRel}
												<button
													type="button"
													class="relation-chip-x"
													tabindex="-1"
													onclick={(e) => {
														e.stopPropagation();
														removeRelationValue(name, recordValue(name)[li]);
													}}
													>×</button
												>
											{/if}
										</span>
									{/each}
								</div>
							{:else}
								<span class="relation-trigger-placeholder"
									>Select {targetColl} record{isMultiRel ? 's' : ''}...</span
								>
							{/if}
							<span class="relation-caret">▾</span>
						</div>
						{#if relationOpen[name]}
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
									{#each filtered as opt (opt.value)}
										<button
											type="button"
											class="relation-option"
											class:active={isMultiRel
												? recordValue(name).includes(opt.value)
												: data[name] === opt.value}
											onmousedown={() => toggleRelationOption(name, opt.value, isMultiRel ? (options?.maxSelect as number) : 1)}
											>{opt.label}</button
										>
									{/each}
									{#if filtered.length === 0}
										<div class="relation-empty">No matching records</div>
									{/if}
								</div>
							</div>
						{/if}
					{:else}
						<div class="relation-missing">
							No target collection configured. Edit the collection to pick one.
						</div>
					{/if}
				</div>
				<span class="field-help"
					>Related record{isMultiRel ? 's (multi-select)' : ''} from
					{targetColl ?? 'unknown collection'}</span
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

<!-- ═══ FILE LIBRARY PICKER ═══ -->
<Modal show={pickerOpen} title="Image Library">
	{#if pickerLoading}
		<div class="picker-status">Loading images…</div>
	{:else if pickerError}
		<div class="picker-status picker-error">{pickerError}</div>
	{:else if pickerItems.length === 0}
		<div class="picker-status">No images uploaded yet. Upload files to build the library.</div>
	{:else}
		<div class="picker-grid">
			{#each pickerItems as item (item.id)}
				{@const t = item.thumbs
					? Object.keys(item.thumbs).sort((a, b) => a.length - b.length)[0]
					: undefined}
				<div class="picker-cell" role="button" tabindex="0" onclick={() => pickFile(item.id)}>
					{#if t && item.thumbs}
						<img src={item.thumbs[t]} alt={item.filename} class="picker-img" loading="lazy" />
					{:else}
						<div class="picker-img picker-img-empty">
							<span class="picker-no-thumb">No thumb</span>
						</div>
					{/if}
					<span class="picker-name">{item.filename}</span>
					<button
						type="button"
						class="picker-delete"
						aria-label="Delete {item.filename}"
						onclick={(e) => {
							e.stopPropagation();
							deleteFromLibrary(item.id);
						}}>×</button
					>
				</div>
			{/each}
		</div>
	{/if}
</Modal>

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
	.file-list {
		display: flex;
		flex-direction: column;
		gap: 4px;
	}

	.file-row {
		display: flex;
		align-items: center;
		gap: 8px;
		padding: 4px 12px;
	}

	.file-name {
		font-size: 0.875rem;
		opacity: 0.6;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		max-width: 180px;
		text-decoration: underline;
	}

	.file-link {
		display: flex;
		align-items: center;
		gap: 8px;
		min-width: 0;
	}

	.file-thumb {
		width: 40px;
		height: 40px;
		object-fit: cover;
		border-radius: 6px;
		flex-shrink: 0;
		background: var(--color-base-200, #f0f0f0);
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

	.file-upload {
		display: flex;
		align-items: center;
		justify-content: center;
		gap: 6px;
		padding: 8px 12px;
		border: 1px dashed var(--color-base-300);
		border-radius: var(--radius-field);
		font-size: 0.875rem;
		color: var(--color-base-content);
		opacity: 0.8;
		cursor: pointer;
		margin-top: 4px;
		transition:
			opacity 0.15s ease,
			border-color 0.15s ease;
	}
	.file-upload:hover {
		opacity: 1;
		border-color: var(--color-base-content);
	}
	.file-upload.disabled {
		opacity: 0.4;
		pointer-events: none;
	}
	.file-upload input[type='file'] {
		display: none;
	}

	/* ── RELATION DROPDOWN ── */
	.relation-wrap {
		position: relative;
	}

	.relation-trigger {
		display: flex;
		align-items: center;
		gap: 6px;
		min-height: 38px;
		padding: 4px 10px;
		border: 1px solid var(--color-base-300);
		border-radius: var(--radius-field);
		background: var(--color-base-100);
		cursor: pointer;
		transition: border-color 0.15s;
		box-sizing: border-box;
	}

	.relation-trigger:hover {
		border-color: var(--color-base-content);
	}

	.relation-trigger.open {
		border-color: var(--color-primary);
	}

	.relation-trigger-labels {
		display: flex;
		flex-wrap: wrap;
		gap: 4px;
		flex: 1;
	}

	.relation-chip {
		display: inline-flex;
		align-items: center;
		gap: 4px;
		padding: 2px 8px;
		border-radius: 999px;
		background: color-mix(in oklab, var(--color-primary) 12%, var(--color-base-100));
		color: var(--color-base-content);
		font-size: 0.75rem;
		max-width: 100%;
	}

	.relation-chip-x {
		border: none;
		background: none;
		color: inherit;
		cursor: pointer;
		font-size: 0.875rem;
		line-height: 1;
		padding: 0;
		opacity: 0.6;
	}

	.relation-chip-x:hover {
		opacity: 1;
	}

	.relation-trigger-placeholder {
		flex: 1;
		color: var(--color-base-hint);
		font-size: 0.8125rem;
	}

	.relation-caret {
		opacity: 0.5;
		font-size: 0.75rem;
	}

	.relation-missing {
		padding: 8px 10px;
		border: 1px dashed var(--color-base-300);
		border-radius: var(--radius-field);
		font-size: 0.8125rem;
		opacity: 0.6;
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

	/* ── File library picker ── */
	.file-actions {
		display: flex;
		gap: 8px;
		align-items: center;
	}

	.picker-status {
		padding: 24px;
		text-align: center;
		opacity: 0.5;
		font-size: 0.8125rem;
	}

	.picker-error {
		color: var(--color-error);
		opacity: 1;
	}

	.picker-grid {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(110px, 1fr));
		gap: 10px;
		max-height: 60vh;
		overflow-y: auto;
	}

	.picker-cell {
		position: relative;
		display: flex;
		flex-direction: column;
		gap: 4px;
		padding: 6px;
		border: 1px solid var(--color-base-300);
		border-radius: var(--radius-box, 8px);
		cursor: pointer;
	}

	.picker-cell:hover {
		border-color: var(--color-primary);
	}

	.picker-img {
		width: 100%;
		height: 80px;
		object-fit: cover;
		border-radius: 4px;
		background: var(--color-base-200);
	}

	.picker-img-empty {
		display: flex;
		align-items: center;
		justify-content: center;
	}

	.picker-no-thumb {
		font-size: 0.6875rem;
		opacity: 0.4;
	}

	.picker-name {
		font-size: 0.6875rem;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.picker-delete {
		position: absolute;
		top: 4px;
		right: 4px;
		width: 20px;
		height: 20px;
		line-height: 1;
		border: none;
		border-radius: 50%;
		background: color-mix(in oklab, var(--color-error) 80%, #000);
		color: #fff;
		font-size: 0.75rem;
		cursor: pointer;
		opacity: 0;
	}

	.picker-cell:hover .picker-delete {
		opacity: 1;
	}
</style>
