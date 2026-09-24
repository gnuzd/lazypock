<script lang="ts">
	import Button from '$lib/components/Button.svelte';
	import Modal from '$lib/components/Modal.svelte';
	import { client } from '$lib/client';
	import { toast } from 'svelte-sonner';

	/**
	 * Shows an "Undo last import" action when the server has a pre-import
	 * snapshot, and rolls the database back to it (behind a confirm modal).
	 * Renders nothing until the status is loaded; shows a muted line when there
	 * is nothing to undo.
	 */
	let {
		class: className = '',
		/** Bump this from the parent (e.g. after a successful import) to refetch. */
		reloadToken = 0
	}: { class?: string; reloadToken?: number } = $props();

	let snapshot = $state<{ created_at?: string } | null>(null);
	let loaded = $state(false);
	let busy = $state(false);
	let confirmOpen = $state(false);

	async function load() {
		try {
			const res = (await client.http.get('/import/status')) as {
				snapshot?: { created_at?: string } | null;
			} | null;
			snapshot = res?.snapshot ?? null;
		} catch {
			snapshot = null;
		} finally {
			loaded = true;
		}
	}

	async function undo() {
		if (!snapshot || busy) return;

		busy = true;
		try {
			await client.http.post('/import/rollback', {});
			toast.success('Rolled back the last import');
			confirmOpen = false;
			await load();
		} catch (e) {
			toast.error(`Rollback failed: ${(e as Error).message}`);
		} finally {
			busy = false;
		}
	}

	function formatTs(ts?: string) {
		if (!ts) return 'an earlier session';
		const d = new Date(ts);
		return Number.isNaN(d.getTime()) ? ts : d.toLocaleString();
	}

	$effect(() => {
		// Refetch whenever the parent bumps reloadToken (and once on mount).
		if (reloadToken >= 0) load();
	});
</script>

{#if loaded && snapshot}
	<div class="rounded-box border border-warning/40 bg-warning/10 p-4 {className}">
		<p class="text-sm font-semibold text-warning">Undo available</p>
		<p class="mt-1 text-xs text-base-content/70">
			The last import/restore ran on {formatTs(snapshot.created_at)}.
		</p>
		<div class="mt-3">
			<Button class="btn-warning btn-sm" onclick={() => (confirmOpen = true)}
				>Undo last import</Button
			>
		</div>
	</div>

	<Modal bind:show={confirmOpen} title="Undo last import?">
		<p class="text-sm">
			Roll the database back to the state right before the import that ran on
			<strong>{formatTs(snapshot.created_at)}</strong>?
		</p>
		<ul class="mt-3 list-disc space-y-1 pl-5 text-xs text-base-content/70">
			<li>Collections created since are dropped.</li>
			<li>Records added since are removed.</li>
			<li>Changed collections and records are restored to their previous state.</li>
			<li>System collections are never touched.</li>
		</ul>
		<div class="mt-4 flex justify-end gap-2">
			<Button class="btn-ghost btn-sm" onclick={() => (confirmOpen = false)}>Cancel</Button>
			<Button class="btn-warning btn-sm" loading={busy} onclick={undo}>Undo import</Button>
		</div>
	</Modal>
{:else if loaded}
	<p class="text-xs text-base-content/50 {className}">No recent import to undo.</p>
{/if}
