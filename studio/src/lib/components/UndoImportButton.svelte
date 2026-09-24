<script lang="ts">
	import Button from '$lib/components/Button.svelte';
	import { client } from '$lib/client';
	import { toast } from 'svelte-sonner';

	/**
	 * Shows an "Undo last import" action when the server has a pre-import
	 * snapshot, and rolls the database back to it on click. Renders nothing
	 * until the status is loaded; shows a muted line when there is nothing to
	 * undo.
	 */
	let {
		class: className = '',
		/** Bump this from the parent (e.g. after a successful import) to refetch. */
		reloadToken = 0
	}: { class?: string; reloadToken?: number } = $props();

	let snapshot = $state<{ created_at?: string } | null>(null);
	let loaded = $state(false);
	let busy = $state(false);

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
		if (
			!confirm(
				'Undo the last import/restore? The database is rolled back to the state right before it — collections created since are dropped and records added since are removed.'
			)
		) {
			return;
		}

		busy = true;
		try {
			await client.http.post('/import/rollback', {});
			toast.success('Rolled back the last import');
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
			The last import/restore ran on {formatTs(snapshot.created_at)}. You can roll the database back
			to the state right before it — collections created since are dropped and records added since
			are removed.
		</p>
		<div class="mt-3">
			<Button class="btn-warning btn-sm" loading={busy} onclick={undo}>Undo last import</Button>
		</div>
	</div>
{:else if loaded}
	<p class="text-xs text-base-content/50 {className}">No recent import to undo.</p>
{/if}
