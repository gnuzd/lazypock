<script lang="ts">
	import { client } from '$lib/client';
	import Button from '$lib/components/Button.svelte';

	let backingUp = $state(false);

	async function doBackup() {
		backingUp = true;
		try {
			const res = (await client.http.get('/export')) as Record<string, unknown> | null;
			if (res) {
				const blob = new Blob([JSON.stringify(res, null, 2)], { type: 'application/json' });
				const url = URL.createObjectURL(blob);
				const a = document.createElement('a');
				a.href = url;
				a.download = `lazypock-backup-${new Date().toISOString().slice(0, 10)}.json`;
				a.click();
				URL.revokeObjectURL(url);
			}
		} catch {
			// ignore
		} finally {
			backingUp = false;
		}
	}
</script>

<h2 class="mb-4 text-lg font-semibold">Backups</h2>
<div class="rounded-box border border-base-300 bg-base-100 p-6">
	<p class="mb-4 text-sm text-base-content/70">
		Download a full JSON backup of all collections and their data.
	</p>
	<Button class="btn-primary" loading={backingUp} onclick={doBackup}>Download Backup</Button>
</div>
