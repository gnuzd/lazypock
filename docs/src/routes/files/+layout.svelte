<script lang="ts">
	let { children } = $props();
</script>

<svelte:head>
	<title>File storage &amp; images — Lazypock Docs</title>
	<meta
		name="description"
		content="Lazypock file storage: upload policy and limits, image presets and variants, local vs S3/R2 backends, direct uploads, the Studio media library, reference tracking and cleanup, plus the files CLI and API reference."
	/>
</svelte:head>

<div class="max-w-3xl">
	<div class="prose-md">
		{@render children()}
	</div>
</div>
