<script lang="ts">
	import { client } from '$lib/client';
	import { onMount } from 'svelte';
	import { ShieldCheck } from '@lucide/svelte';
	import Button from '$lib/components/Button.svelte';
	import Modal from '$lib/components/Modal.svelte';
	import RuleField from '$lib/components/RuleField.svelte';
	import MediaLibrary from '$lib/components/MediaLibrary.svelte';
	import '../settings.css';

	let listRule = $state<string | null>(null);
	let deleteRule = $state<string | null>(null);
	let showRules = $state(false);
	let loading = $state(true);
	let saving = $state(false);
	let saved = $state(false);
	let error = $state('');
	let showFields = $state(false);

	// Non-rule keys currently under `files` (e.g. `unattached_ttl_ms`). Kept and
	// sent back because PATCH /settings merges at the top level only, so the
	// whole `files` object is replaced on save.
	let otherFiles = $state<Record<string, unknown>>({});

	const availableFields = [
		{ name: 'id', type: 'UUID' },
		{ name: 'filename', type: 'text' },
		{ name: 'extension', type: 'text' },
		{ name: 'mime_type', type: 'text' },
		{ name: 'size', type: 'number' },
		{ name: 'origin', type: '"field" | "editor" | "library"' },
		{ name: 'status', type: 'text' },
		{ name: 'collection_name', type: 'text' },
		{ name: 'field_name', type: 'text' },
		{ name: 'record_id', type: 'text' },
		{ name: 'uploaded_by', type: 'the uploader id (@request.auth.id)' },
		{ name: 'created_at', type: 'date' }
	];

	onMount(async () => {
		try {
			const res = (await client.http.get('/settings')) as Record<string, unknown> | null;
			const files = ((res?.files as Record<string, unknown>) ?? {}) as Record<string, unknown>;
			const rules = (files.rules as Record<string, unknown>) ?? {};

			listRule = (rules.listRule as string | null) ?? null;
			deleteRule = (rules.deleteRule as string | null) ?? null;
			otherFiles = Object.fromEntries(Object.entries(files).filter(([key]) => key !== 'rules'));
		} catch (e) {
			error = (e as Error).message || 'Could not load the file rules';
		} finally {
			loading = false;
		}
	});

	function openRules() {
		saved = false;
		error = '';
		showRules = true;
	}

	async function save() {
		saving = true;
		saved = false;
		error = '';
		try {
			await client.http.patch('/settings', {
				files: { ...otherFiles, rules: { listRule, deleteRule } }
			});
			saved = true;
			setTimeout(() => (saved = false), 2000);
		} catch (e) {
			error = (e as Error).message || 'Could not save the file rules';
		} finally {
			saving = false;
		}
	}
</script>

<div class="mb-4 flex items-start justify-between gap-4">
	<div>
		<h2 class="mb-1 text-lg font-semibold">Media Library</h2>
		<p class="text-sm text-base-content/60">
			Every file uploaded through the app, the SDK or the richtext editor. Click a file to see its
			details, or delete it — deleting removes the file and all of its variants from storage, and a
			file that a record still references asks for confirmation first.
		</p>
	</div>
	<Button class="btn-outline btn-sm shrink-0" onclick={openRules}>
		<ShieldCheck size={15} />
		File rules
	</Button>
</div>

<Modal bind:show={showRules} title="File rules" size="lg">
	<form
		class="grid gap-4"
		onsubmit={(e) => {
			e.preventDefault();
			save();
		}}
	>
		<p class="text-xs text-base-content/70">
			Control who can list and delete files through the API. Rules follow the same three states as
			collection rules: <strong>locked</strong> (superusers only), <strong>empty</strong> (anyone), or
			a filter evaluated against each file. Superusers always bypass. Uploads require authentication regardless,
			and files are still served publicly at their URL.
		</p>

		{#if error}
			<p class="text-xs text-error">{error}</p>
		{/if}

		<div class="space-y-4">
			<RuleField
				label="List/Search rule"
				name="fileListRule"
				bind:value={listRule}
				placeholder="uploaded_by = @request.auth.id"
				disabled={loading}
			/>
			<RuleField
				label="Delete rule"
				name="fileDeleteRule"
				bind:value={deleteRule}
				placeholder="uploaded_by = @request.auth.id"
				disabled={loading}
			/>
		</div>

		<div>
			<button
				type="button"
				class="cursor-pointer text-xs font-medium text-primary hover:underline"
				onclick={() => (showFields = !showFields)}
			>
				{showFields ? 'Hide available fields' : 'Show available fields'}
			</button>

			{#if showFields}
				<div
					class="mt-2 rounded-field border border-base-300 bg-base-200/50 p-3 text-xs text-base-content/80"
				>
					<p class="mb-2">
						Example — let uploaders manage their own files, and app admins manage everything:
					</p>
					<pre
						class="mb-2 overflow-x-auto rounded bg-base-300/40 p-2 font-mono text-[11px]">uploaded_by = @request.auth.id || @request.auth.role = 'admin'</pre>
					<p class="mb-1">
						Put the field on the left of the comparison. <code>@request.auth.id</code>,
						<code>@request.auth.email</code> and <code>@request.auth.role</code> refer to the authenticated
						user.
					</p>
					<table class="w-full">
						<tbody>
							{#each availableFields as field (field.name)}
								<tr>
									<td class="py-0.5 pr-3 font-mono whitespace-nowrap">{field.name}</td>
									<td class="py-0.5 text-base-content/60">{field.type}</td>
								</tr>
							{/each}
						</tbody>
					</table>
				</div>
			{/if}
		</div>

		<div class="flex items-center justify-end gap-3">
			{#if saved}<span class="text-xs text-success">Saved!</span>{/if}
			<Button
				class="btn-ghost btn-sm"
				type="button"
				disabled={saving}
				onclick={() => (showRules = false)}>Close</Button
			>
			<Button class="btn-primary btn-sm" loading={saving} disabled={saving || loading} type="submit"
				>Save rules</Button
			>
		</div>
	</form>
</Modal>

<MediaLibrary inline mode="manage" title="Media Library" />
