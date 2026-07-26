<script lang="ts">
	import {
		FileText,
		FileCode,
		Hash,
		ToggleLeft,
		Mail,
		Link,
		Calendar,
		CalendarCheck,
		ListChecks,
		Braces,
		Image,
		GitBranch,
		MapPin,
		Lock,
		Settings,
		GripVertical
	} from '@lucide/svelte';
	import { slugify } from '$lib/fieldTypes';
	import OptionRow from './OptionRow.svelte';
	import Button from './Button.svelte';
	import { slide } from 'svelte/transition';

	const typeIcons: Record<string, typeof FileText> = {
		text: FileText,
		editor: FileCode,
		number: Hash,
		bool: ToggleLeft,
		email: Mail,
		url: Link,
		date: Calendar,
		autodate: CalendarCheck,
		select: ListChecks,
		json: Braces,
		file: Image,
		relation: GitBranch,
		geoPoint: MapPin,
		password: Lock
	};

	let {
		field = $bindable<Record<string, unknown>>({}),
		fieldIndex = 0,
		collections = [] as Record<string, unknown>[]
	}: {
		field: Record<string, unknown>;
		fieldIndex?: number;
		collections?: Record<string, unknown>[];
	} = $props();

	let open = $state(false);

	function setOpt(key: string, val: unknown) {
		const opts = (field.options as Record<string, unknown>) || {};
		field.options = { ...opts, [key]: val };
	}

	function unsetOpt(key: string) {
		const opts = (field.options as Record<string, unknown>) || {};
		const rest = Object.fromEntries(Object.entries(opts).filter(([k]) => k !== key));
		field.options = rest;
	}
</script>

{#if !field['@toDelete']}
	<div
		class="overflow-hidden rounded-field border border-base-300 bg-base-100"
		role="region"
		tabindex="-1"
	>
		<!-- Header / summary row -->
		<div class="flex items-center gap-2 p-1.5">
			<div
				class="flex h-full w-6 cursor-grab items-center justify-center text-base-content/40 hover:text-base-content"
				draggable="true"
			>
				<GripVertical size={16} />
			</div>

			<!-- Type icon -->
			<div class="flex h-7 w-7 shrink-0 items-center justify-center rounded bg-base-200 opacity-60">
				{#each [typeIcons[field.type as string] || FileText] as Cmp (Cmp)}
					<Cmp size={16} />
				{/each}
			</div>

			<!-- Name input -->
			<div class="min-w-0 flex-1">
				<input
					type="text"
					class="input input-sm w-full border-none bg-transparent text-sm focus:outline-none"
					placeholder="Field name*"
					value={(field.name as string) || ''}
					disabled={!!field.system}
					oninput={(e) => {
						field.name = slugify((e.target as HTMLInputElement).value);
					}}
					spellcheck="false"
					required
				/>
			</div>

			<!-- Badges -->
			<div class="flex items-center gap-1 text-[10px] font-medium">
				{#if field.required}
					<span class="rounded bg-success/20 px-1 py-0.5 text-success">Required</span>
				{/if}
				{#if field.indexed}
					<span class="rounded bg-info/20 px-1 py-0.5 text-info">Indexed</span>
				{/if}
				{#if field.hidden}
					<span class="rounded bg-error/20 px-1 py-0.5 text-error">Hidden</span>
				{:else if field.presentable}
					<span class="rounded bg-info/20 px-1 py-0.5 text-info">Presentable</span>
				{/if}
			</div>

			<!-- Header extras -->
			{#if field.type === 'autodate'}
				{@const opts = (field.options as Record<string, unknown>) || {}}
				{@const opt =
					opts.onCreate && opts.onUpdate ? 'create/update' : opts.onUpdate ? 'update' : 'create'}
				<select
					class="select select-sm max-w-[120px] text-xs"
					value={opt}
					onchange={(e) => {
						const v = (e.target as HTMLSelectElement).value;
						const o = (field.options as Record<string, unknown>) || {};
						field.options = {
							...o,
							onCreate: v === 'create' || v === 'create/update',
							onUpdate: v === 'update' || v === 'create/update'
						};
					}}
				>
					<option value="create">Create</option>
					<option value="update">Update</option>
					<option value="create/update">Create/Update</option>
				</select>
			{:else if field.type === 'select' || field.type === 'multi_select'}
				<select
					class="select select-sm max-w-[80px] text-xs"
					value={(((field.options as Record<string, unknown>)?.maxSelect as number) || 1) > 1
						? 'multiple'
						: 'single'}
					onchange={(e) => {
						const o = { ...(field.options as Record<string, unknown>) };
						o.maxSelect = (e.target as HTMLSelectElement).value === 'multiple' ? 10 : 1;
						field.options = o;
					}}
				>
					<option value="single">Single</option>
					<option value="multiple">Multiple</option>
				</select>
			{:else if field.type === 'relation'}
				<select
					class="select select-sm max-w-[150px] text-xs"
					value={(field.collectionId as string) || ''}
					onchange={(e) => (field.collectionId = (e.target as HTMLSelectElement).value)}
				>
					<option value="" disabled>Select collection*</option>
					{#each collections.filter((c) => (c.type as string) !== 'view') as coll (coll.id as string)}
						<option value={coll.id as string}>{coll.name as string}</option>
					{/each}
				</select>
				<select
					class="select select-sm max-w-[80px] text-xs"
					value={((field.options as Record<string, unknown>)?.maxSelect as number) > 1
						? 'multiple'
						: 'single'}
					onchange={(e) => {
						setOpt('maxSelect', (e.target as HTMLSelectElement).value === 'multiple' ? 10 : 1);
					}}
				>
					<option value="single">Single</option>
					<option value="multiple">Multiple</option>
				</select>
			{:else if field.type === 'file'}
				<select
					class="select select-sm max-w-[80px] text-xs"
					value={((field.options as Record<string, unknown>)?.maxSelect as number) > 1
						? 'multiple'
						: 'single'}
					onchange={(e) => {
						const v = (field.options as Record<string, unknown>)?.maxSelect;
						setOpt(
							'maxSelect',
							(e.target as HTMLSelectElement).value === 'multiple' ? (v as number) || 10 : 1
						);
					}}
				>
					<option value="single">Single</option>
					<option value="multiple">Multiple</option>
				</select>
			{/if}

			<!-- Settings toggle (expand inline) -->
			<button
				type="button"
				class="btn btn-ghost btn-sm px-1"
				title="Field options"
				onclick={() => (open = !open)}
			>
				<Settings size={16} />
			</button>
		</div>

		<!-- Expanded inline settings (slide transition) -->
		{#if open}
			{@const opts = (field.options as Record<string, unknown>) || {}}
			<div
				class="space-y-2 border-t border-base-300 p-3 text-sm"
				transition:slide={{ duration: 150 }}
			>
				{#if field.type === 'text'}
					<div class="mb-2 flex items-center gap-2 rounded-field bg-base-200/40 p-1.5">
						<input
							type="checkbox"
							class="checkbox checkbox-sm"
							id="field-{fieldIndex}-multiline"
							checked={!!field.multiline}
							onchange={(e) => (field.multiline = (e.target as HTMLInputElement).checked)}
						/>
						<label class="cursor-pointer text-xs" for="field-{fieldIndex}-multiline"
							>Multiline (textarea)</label
						>
					</div>
					<div class="grid grid-cols-2 gap-3">
						<div class="rounded-field bg-base-200/40 p-1.5">
							<label
								class="mb-1 block text-xs font-medium text-base-content/70"
								for="field-{fieldIndex}-min">Min length</label
							>
							<input
								type="number"
								id="field-{fieldIndex}-min"
								step="1"
								min="0"
								placeholder="No min limit"
								class="input input-sm w-full"
								value={(opts.min as number) ?? ''}
								oninput={(e) => {
									const v = (e.target as HTMLInputElement).value;
									if (v === '') {
										unsetOpt('min');
										return;
									}
									if (v.length > 1 && v[0] == '0') return;
									setOpt('min', parseInt(v, 10));
								}}
							/>
						</div>
						<div class="rounded-field bg-base-200/40 p-1.5">
							<label
								class="mb-1 block text-xs font-medium text-base-content/70"
								for="field-{fieldIndex}-max">Max length</label
							>
							<input
								type="number"
								id="field-{fieldIndex}-max"
								step="1"
								min={(opts.min as number) || 0}
								placeholder="Default 5000"
								class="input input-sm w-full"
								value={(opts.max as number) ?? ''}
								oninput={(e) => {
									const v = (e.target as HTMLInputElement).value;
									if (v === '') {
										unsetOpt('max');
										return;
									}
									if (v.length > 1 && v[0] == '0') return;
									setOpt('max', parseInt(v, 10));
								}}
							/>
						</div>
						<div class="rounded-field bg-base-200/40 p-1.5">
							<label
								class="mb-1 block text-xs font-medium text-base-content/70"
								for="field-{fieldIndex}-pattern">Validation pattern</label
							>
							<input
								type="text"
								id="field-{fieldIndex}-pattern"
								placeholder="e.g. ^[a-z0-9]+$"
								class="input input-sm w-full"
								value={(opts.pattern as string) || ''}
								oninput={(e) => {
									const v = (e.target as HTMLInputElement).value;
									if (v) setOpt('pattern', v);
									else unsetOpt('pattern');
								}}
							/>
						</div>
						<div class="rounded-field bg-base-200/40 p-1.5">
							<label
								class="mb-1 block text-xs font-medium text-base-content/70"
								for="field-{fieldIndex}-autogenerate">Autogenerate pattern</label
							>
							<input
								type="text"
								id="field-{fieldIndex}-autogenerate"
								placeholder="e.g. [a-z0-9]{30}"
								class="input input-sm w-full"
								value={(opts.autogeneratePattern as string) || ''}
								oninput={(e) => {
									const v = (e.target as HTMLInputElement).value;
									if (v) setOpt('autogeneratePattern', v);
									else unsetOpt('autogeneratePattern');
								}}
							/>
						</div>
					</div>
				{:else if field.type === 'number'}
					<div class="grid grid-cols-2 gap-3">
						<div class="rounded-field bg-base-200/40 p-1.5">
							<label
								class="mb-1 block text-xs font-medium text-base-content/70"
								for="field-{fieldIndex}-min">Min</label
							>
							<input
								type="number"
								id="field-{fieldIndex}-min"
								class="input input-sm w-full"
								value={typeof opts.min === 'number' ? opts.min : ''}
								oninput={(e) => {
									const v = (e.target as HTMLInputElement).value;
									if (v === '') {
										unsetOpt('min');
										return;
									}
									setOpt('min', Number(v));
								}}
							/>
						</div>
						<div class="rounded-field bg-base-200/40 p-1.5">
							<label
								class="mb-1 block text-xs font-medium text-base-content/70"
								for="field-{fieldIndex}-max">Max</label
							>
							<input
								type="number"
								id="field-{fieldIndex}-max"
								min={(opts.min as number) || undefined}
								class="input input-sm w-full"
								value={typeof opts.max === 'number' ? opts.max : ''}
								oninput={(e) => {
									const v = (e.target as HTMLInputElement).value;
									if (v === '') {
										unsetOpt('max');
										return;
									}
									setOpt('max', Number(v));
								}}
							/>
						</div>
					</div>
				{:else if field.type === 'email'}
					<div class="grid grid-cols-2 gap-3">
						<div class="rounded-field bg-base-200/40 p-1.5">
							<label
								class="mb-1 block text-xs font-medium text-base-content/70"
								for="field-{fieldIndex}-except">Except domains</label
							>
							<input
								type="text"
								id="field-{fieldIndex}-except"
								disabled={!!(opts.onlyDomains as string[])?.length}
								class="input input-sm w-full"
								value={(opts.exceptDomains as string[])?.join(', ') || ''}
								onchange={(e) => {
									const v = (e.target as HTMLInputElement).value
										.split(',')
										.map((s) => s.trim())
										.filter(Boolean);
									if (v.length) setOpt('exceptDomains', v);
									else unsetOpt('exceptDomains');
								}}
							/>
						</div>
						<div class="rounded-field bg-base-200/40 p-1.5">
							<label
								class="mb-1 block text-xs font-medium text-base-content/70"
								for="field-{fieldIndex}-only">Only domains</label
							>
							<input
								type="text"
								id="field-{fieldIndex}-only"
								disabled={!!(opts.exceptDomains as string[])?.length}
								class="input input-sm w-full"
								value={(opts.onlyDomains as string[])?.join(', ') || ''}
								onchange={(e) => {
									const v = (e.target as HTMLInputElement).value
										.split(',')
										.map((s) => s.trim())
										.filter(Boolean);
									if (v.length) setOpt('onlyDomains', v);
									else unsetOpt('onlyDomains');
								}}
							/>
						</div>
					</div>
				{:else if field.type === 'date'}
					<div class="grid grid-cols-2 gap-3">
						<div class="rounded-field bg-base-200/40 p-1.5">
							<label
								class="mb-1 block text-xs font-medium text-base-content/70"
								for="field-{fieldIndex}-minDate">Min date</label
							>
							<input
								type="datetime-local"
								id="field-{fieldIndex}-minDate"
								step="1"
								class="input input-sm w-full"
								value={(opts.min as string) || ''}
								onchange={(e) => {
									const v = (e.target as HTMLInputElement).value;
									if (v) setOpt('min', v);
									else unsetOpt('min');
								}}
							/>
						</div>
						<div class="rounded-field bg-base-200/40 p-1.5">
							<label
								class="mb-1 block text-xs font-medium text-base-content/70"
								for="field-{fieldIndex}-maxDate">Max date</label
							>
							<input
								type="datetime-local"
								id="field-{fieldIndex}-maxDate"
								step="1"
								class="input input-sm w-full"
								value={(opts.max as string) || ''}
								onchange={(e) => {
									const v = (e.target as HTMLInputElement).value;
									if (v) setOpt('max', v);
									else unsetOpt('max');
								}}
							/>
						</div>
					</div>
				{:else if field.type === 'select' || field.type === 'multi_select'}
					{@const values = (opts.values as string[]) || []}

					<div class="rounded-field bg-base-200/40 p-1.5">
						<label class="mb-1 block text-xs font-medium text-base-content/70">Options</label>
						<div class="flex flex-col gap-1">
							{#each values as val, i (i)}
								<OptionRow
									value={val}
									onchange={(v) => {
										const newOpts = { ...opts };
										const newVals = [...values];
										newVals[i] = v;
										newOpts.values = newVals;
										field.options = newOpts;
									}}
									onremove={() => {
										const newOpts = { ...opts };
										newOpts.values = values.filter((_: string, j: number) => j !== i);
										field.options = newOpts;
									}}
								/>
							{/each}
							<Button
								type="button"
								class="btn-sm w-fit px-1 text-xs font-medium"
								onclick={() => {
									const newOpts = { ...opts };
									newOpts.values = [...values, ''];
									field.options = newOpts;
								}}>+ Add option</Button
							>
						</div>
					</div>

					{#if (((field.options as Record<string, unknown>)?.maxSelect as number) || 1) > 1}
						<div class="rounded-field bg-base-200/40 p-1.5">
							<label
								class="mb-1 block text-xs font-medium text-base-content/70"
								for="field-{fieldIndex}-maxSelect">Max select</label
							>
							<input
								type="number"
								id="field-{fieldIndex}-maxSelect"
								step="1"
								min="2"
								placeholder="Defaults to 10"
								class="input input-sm w-full"
								value={((field.options as Record<string, unknown>)?.maxSelect as number) || ''}
								onchange={(e) => {
									const newOpts = { ...(field.options as Record<string, unknown>) };
									newOpts.maxSelect = Math.max(
										2,
										parseInt((e.target as HTMLInputElement).value, 10)
									);
									field.options = newOpts;
								}}
							/>
						</div>
					{/if}
				{:else if field.type === 'json'}
					<div class="rounded-field bg-base-200/40 p-1.5">
						<label
							class="mb-1 block text-xs font-medium text-base-content/70"
							for="field-{fieldIndex}-maxSize">Max size (bytes)</label
						>
						<input
							type="number"
							id="field-{fieldIndex}-maxSize"
							step="1"
							min="0"
							placeholder="Default ~1MB"
							class="input input-sm w-full"
							value={(opts.maxSize as number) ?? ''}
							oninput={(e) => {
								const v = (e.target as HTMLInputElement).value;
								if (v === '') {
									unsetOpt('maxSize');
									return;
								}
								if (v.length > 1 && v[0] == '0') return;
								setOpt('maxSize', parseInt(v, 10));
							}}
						/>
					</div>
				{:else if field.type === 'file'}
					<div class="grid grid-cols-2 gap-3">
						<div class="col-span-2 rounded-field bg-base-200/40 p-1.5">
							<label
								class="mb-1 block text-xs font-medium text-base-content/70"
								for="field-{fieldIndex}-mimeTypes">Allowed mime types</label
							>
							<input
								type="text"
								id="field-{fieldIndex}-mimeTypes"
								placeholder="No restriction"
								class="input input-sm w-full"
								value={(opts.mimeTypes as string[])?.join(', ') || ''}
								onchange={(e) => {
									const v = (e.target as HTMLInputElement).value
										.split(',')
										.map((s) => s.trim())
										.filter(Boolean);
									if (v.length) setOpt('mimeTypes', v);
									else unsetOpt('mimeTypes');
								}}
							/>
						</div>
						<div class="rounded-field bg-base-200/40 p-1.5">
							<label
								class="mb-1 block text-xs font-medium text-base-content/70"
								for="field-{fieldIndex}-thumbs">Thumb sizes</label
							>
							<input
								type="text"
								id="field-{fieldIndex}-thumbs"
								placeholder="e.g. 50x50, 480x720"
								class="input input-sm w-full"
								value={(opts.thumbs as string[])?.join(', ') || ''}
								onchange={(e) => {
									const v = (e.target as HTMLInputElement).value
										.split(',')
										.map((s) => s.trim())
										.filter(Boolean);
									if (v.length) setOpt('thumbs', v);
									else unsetOpt('thumbs');
								}}
							/>
						</div>
						<div class="rounded-field bg-base-200/40 p-1.5">
							<label
								class="mb-1 block text-xs font-medium text-base-content/70"
								for="field-{fieldIndex}-maxSize">Max size (bytes)</label
							>
							<input
								type="number"
								id="field-{fieldIndex}-maxSize"
								step="1"
								min="0"
								placeholder="~5MB default"
								class="input input-sm w-full"
								value={(opts.maxSize as number) ?? ''}
								oninput={(e) => {
									const v = (e.target as HTMLInputElement).value;
									if (v === '') {
										unsetOpt('maxSize');
										return;
									}
									if (v.length > 1 && v[0] == '0') return;
									setOpt('maxSize', parseInt(v, 10));
								}}
							/>
						</div>
						<div class="mt-1 flex items-center gap-2 rounded-field bg-base-200/40 p-1.5">
							<input
								type="checkbox"
								class="checkbox checkbox-sm"
								id="field-{fieldIndex}-protected"
								checked={!!opts.protected}
								onchange={(e) => {
									if ((e.target as HTMLInputElement).checked) setOpt('protected', true);
									else unsetOpt('protected');
								}}
							/>
							<label class="cursor-pointer text-xs" for="field-{fieldIndex}-protected"
								>Protected</label
							>
						</div>
					</div>
				{:else if field.type === 'relation'}
					<div class="rounded-field bg-base-200/40 p-1.5" hidden={(opts.maxSelect as number) < 2}>
						<label
							class="mb-1 block text-xs font-medium text-base-content/70"
							for="field-{fieldIndex}-minSelect">Min select</label
						>
						<input
							type="number"
							id="field-{fieldIndex}-minSelect"
							step="1"
							min="0"
							placeholder="No min limit"
							class="input input-sm w-full"
							value={(opts.minSelect as number) ?? ''}
							onchange={(e) => {
								const v = (e.target as HTMLInputElement).value;
								if (v === '') {
									unsetOpt('minSelect');
									return;
								}
								setOpt('minSelect', parseInt(v, 10));
							}}
						/>
					</div>
					<div class="rounded-field bg-base-200/40 p-1.5" hidden={(opts.maxSelect as number) < 2}>
						<label
							class="mb-1 block text-xs font-medium text-base-content/70"
							for="field-{fieldIndex}-maxSelect">Max select</label
						>
						<input
							type="number"
							id="field-{fieldIndex}-maxSelect"
							step="1"
							min={(opts.minSelect as number) || 2}
							placeholder="Default to single"
							class="input input-sm w-full"
							value={(opts.maxSelect as number) ?? ''}
							onchange={(e) => {
								const v = parseInt((e.target as HTMLInputElement).value, 10);
								setOpt('maxSelect', v > 1 ? v : 1);
							}}
						/>
					</div>
					<div class="rounded-field bg-base-200/40 p-1.5">
						<label
							class="mb-1 block text-xs font-medium text-base-content/70"
							for="field-{fieldIndex}-cascade">Cascade delete</label
						>
						<select
							class="select select-sm w-full"
							id="field-{fieldIndex}-cascade"
							value={opts.cascadeDelete ? 'true' : 'false'}
							onchange={(e) => {
								setOpt('cascadeDelete', (e.target as HTMLSelectElement).value === 'true');
							}}
						>
							<option value="false">False</option>
							<option value="true">True</option>
						</select>
					</div>
				{/if}

				<!-- Help text for all types -->
				<div class="rounded-field bg-base-200/40 p-1.5">
					<label
						class="mb-1 block text-xs font-medium text-base-content/70"
						for="field-{fieldIndex}-help">Help text</label
					>
					<input
						type="text"
						id="field-{fieldIndex}-help"
						class="input input-sm w-full"
						value={(opts.help as string) || ''}
						oninput={(e) => {
							const v = (e.target as HTMLInputElement).value;
							if (v) setOpt('help', v);
							else unsetOpt('help');
						}}
					/>
				</div>

				<!-- Toggles inside settings panel -->
				<div class="mt-2 border-t border-base-300 pt-2">
					<div class="flex items-center gap-3 text-xs">
						{#if field.type === 'text' || field.type === 'email' || field.type === 'url' || field.type === 'select' || field.type === 'relation' || field.type === 'editor' || field.type === 'password'}
							<label class="flex cursor-pointer items-center gap-1">
								<input
									type="checkbox"
									class="checkbox checkbox-sm"
									id="field-{fieldIndex}-required"
									checked={!!field.required}
									onchange={(e) => (field.required = (e.target as HTMLInputElement).checked)}
								/>
								Required
							</label>
						{/if}
						<label class="flex cursor-pointer items-center gap-1">
							<input
								type="checkbox"
								class="checkbox checkbox-sm"
								id="field-{fieldIndex}-indexed"
								checked={!!field.indexed}
								onchange={(e) => (field.indexed = (e.target as HTMLInputElement).checked)}
							/>
							Indexed
						</label>
						<label class="flex cursor-pointer items-center gap-1">
							<input
								type="checkbox"
								class="checkbox checkbox-sm"
								id="field-{fieldIndex}-presentable"
								checked={!!field.presentable}
								onchange={(e) => {
									field.presentable = (e.target as HTMLInputElement).checked;
									if (field.presentable) field.hidden = false;
								}}
								disabled={!!field.hidden}
							/>
							Presentable
						</label>
						<label class="flex cursor-pointer items-center gap-1">
							<input
								type="checkbox"
								class="checkbox checkbox-sm"
								id="field-{fieldIndex}-hidden"
								checked={!!field.hidden}
								onchange={(e) => {
									field.hidden = (e.target as HTMLInputElement).checked;
									if (field.hidden) field.presentable = false;
								}}
							/>
							Hidden
						</label>
						<div class="ml-auto">
							<button
								type="button"
								class="btn btn-ghost btn-xs px-1 text-error hover:text-error"
								onclick={() => (field['@toDelete'] = true)}
							>
								<svg
									width="14"
									height="14"
									viewBox="0 0 24 24"
									fill="none"
									stroke="currentColor"
									stroke-width="2"
									stroke-linecap="round"
									stroke-linejoin="round"
									><polyline points="3 6 5 6 21 6" /><path
										d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"
									/></svg
								>
							</button>
						</div>
					</div>
				</div>
			</div>
		{/if}
	</div>
{/if}
