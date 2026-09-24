<script lang="ts">
	import { client } from '$lib/client';
	import Button from '$lib/components/Button.svelte';
	import { toast } from 'svelte-sonner';
	import { buildImportPrompt, type PromptCollection } from '$lib/importPrompt';

	/**
	 * Copies a self-contained AI prompt (format rules + example + the current
	 * collections) so the user can ask any assistant to generate an import file.
	 */
	let { class: className = 'btn-outline btn-sm' }: { class?: string } = $props();

	let busy = $state(false);

	async function copyPrompt() {
		busy = true;
		try {
			const res = (await client.http.get('/collections')) as { items?: PromptCollection[] } | null;
			const prompt = buildImportPrompt(res?.items ?? []);
			await navigator.clipboard.writeText(prompt);
			toast.success('AI prompt copied — paste it into your assistant and describe what you want');
		} catch (e) {
			toast.error(`Could not copy the prompt: ${(e as Error).message}`);
		} finally {
			busy = false;
		}
	}
</script>

<Button class={className} loading={busy} onclick={copyPrompt}>Copy AI prompt</Button>
