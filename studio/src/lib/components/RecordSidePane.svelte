<script lang="ts">
	import { client } from '$lib/client';
	import Dropdown from '$lib/components/Dropdown.svelte';
	import SidePane from '$lib/components/SidePane.svelte';
	import RecordForm from '$lib/components/RecordForm.svelte';
	import Button from '$lib/components/Button.svelte';
	import { buildRecordSchema, cleanRecordData } from '$lib/validation';
	import type { z } from 'zod';

	/**
	 * Record create/edit sidepane. Owns the form state, validation, save,
	 * password-change, and delete actions for the currently open record.
	 */
	let {
		collection = null as Record<string, unknown> | null,
		collections = [] as Record<string, unknown>[],
		show = $bindable(false),
		recordData = $bindable<Record<string, unknown>>({}),
		editingRecordId = $bindable<string | null>(null),
		onSaved,
		onDeleted
	}: {
		/** The collection the record belongs to (drives the dynamic schema). */
		collection: Record<string, unknown> | null;
		/** All collections (for resolving relation targets in RecordForm). */
		collections?: Record<string, unknown>[];
		/** Open/close the pane. */
		show: boolean;
		/** Live form data — bound from the parent. */
		recordData: Record<string, unknown>;
		/** Set when editing an existing record. */
		editingRecordId: string | null;
		/** Called after a successful create/update. */
		onSaved?: () => void;
		/** Called after a successful delete. */
		onDeleted?: () => void;
	} = $props();

	let recordSaving = $state(false);
	let recordError = $state('');
	let recordFieldErrors = $state<Record<string, string>>({});
	// ── Password change state (edit mode only) ──
	let passwordSaving = $state(false);
	let passwordError = $state('');

	// Dynamic record schema from collection fields
	let recordSchema = $state<z.ZodObject<Record<string, z.ZodTypeAny>> | null>(null);

	$effect(() => {
		if (!collection) {
			recordSchema = null;
			return;
		}
		const schemaFields = (collection.fields as Record<string, unknown>[]) ?? [];
		recordSchema = buildRecordSchema(schemaFields).schema;
	});

	async function saveRecord() {
		if (recordSaving || !collection || !recordSchema) return;

		// Validate with zod
		const result = recordSchema.safeParse(recordData);
		if (!result.success) {
			const fieldErrs: Record<string, string> = {};
			for (const issue of result.error.issues) {
				const fieldName = issue.path.join('.');
				if (!fieldErrs[fieldName]) {
					fieldErrs[fieldName] = issue.message;
				}
			}
			recordFieldErrors = fieldErrs;
			return;
		}

		recordFieldErrors = {};
		recordSaving = true;
		recordError = '';
		const collName = collection.name as string;
		const schemaFields = (collection?.fields as Record<string, unknown>[]) ?? [];
		const cleaned = cleanRecordData(result.data as Record<string, unknown>, schemaFields);
		try {
			if (editingRecordId) {
				await client.updateRecord(collName, editingRecordId, cleaned);
			} else {
				await client.createRecord(collName, cleaned);
			}
			show = false;
			onSaved?.();
		} catch (e) {
			recordError = (e as Error).message || 'Failed to save record';
		} finally {
			recordSaving = false;
		}
	}

	async function savePassword(password: string) {
		if (!collection || !editingRecordId) return;
		passwordSaving = true;
		passwordError = '';
		try {
			// Find the password fields (e.g. password_hash)
			const schemaFields = (collection?.fields as Record<string, unknown>[]) ?? [];
			const pwFields = schemaFields.filter((f) => f.type === 'password');
			const payload: Record<string, string> = {};
			for (const f of pwFields) {
				payload[f.name as string] = password;
			}
			await client.updateRecord(collection.name as string, editingRecordId, payload);
			passwordSaving = false;
			passwordError = '';
			onSaved?.();
		} catch (e) {
			passwordError = (e as Error).message || 'Failed to save password';
		} finally {
			passwordSaving = false;
		}
	}

	async function deleteRecord() {
		if (!collection || !editingRecordId || recordSaving) return;
		if (!confirm('Delete this record? This action cannot be undone.')) return;
		recordSaving = true;
		recordError = '';
		const collName = collection.name as string;
		try {
			await client.deleteRecord(collName, editingRecordId);
			show = false;
			onDeleted?.();
		} catch (e) {
			recordError = (e as Error).message || 'Failed to delete record';
		} finally {
			recordSaving = false;
		}
	}
</script>

<SidePane bind:show title={editingRecordId ? 'Edit Record' : 'New Record'} closable={false}>
	{#snippet headerExtra()}
		{#if editingRecordId}
			<Dropdown>
				{#snippet trigger()}
					<button type="button" class="btn btn-ghost btn-sm px-2">
						<svg
							width="16"
							height="16"
							viewBox="0 0 24 24"
							fill="none"
							stroke="currentColor"
							stroke-width="2"
							stroke-linecap="round"
							stroke-linejoin="round"
							><circle cx="12" cy="5" r="1" /><circle cx="12" cy="12" r="1" /><circle
								cx="12"
								cy="19"
								r="1"
							/></svg
						>
					</button>
				{/snippet}
				<div class="min-w-[140px] p-1">
					<button
						type="button"
						class="flex w-full cursor-pointer items-center gap-2 rounded-field border-none bg-transparent px-3 py-1.5 text-sm text-error hover:bg-error/10"
						onclick={deleteRecord}
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
						Delete
					</button>
				</div>
			</Dropdown>
		{/if}
	{/snippet}
	<div class="flex h-full min-h-0 flex-col">
		<div class="flex-1 overflow-y-auto p-4">
			<RecordForm
				fields={((collection?.fields ?? []) as Record<string, unknown>[]).toSorted(
					(a, b) => ((a.sort_order as number) ?? 0) - ((b.sort_order as number) ?? 0)
				)}
				{collections}
				bind:data={recordData}
				disabled={recordSaving}
				errors={recordFieldErrors}
				editing={!!editingRecordId}
				bind:passwordSaving
				bind:passwordError
				onPasswordSave={savePassword}
			/>
		</div>

		{#if recordError}
			<div class="shrink-0 border-t border-base-300 bg-error/10 px-4 py-2 text-xs text-error">
				{recordError}
			</div>
		{/if}

		<div class="flex shrink-0 items-center gap-2 border-t border-base-300 px-4 py-3">
			<Button class="btn-ghost mr-auto" onclick={() => (show = false)}>Close</Button>
			<Button
				class="btn-primary"
				loading={recordSaving}
				disabled={recordSaving}
				onclick={saveRecord}
			>
				{editingRecordId ? 'Update' : 'Create'}
			</Button>
		</div>
	</div>
</SidePane>
