<script lang="ts">
	import Seo from '$lib/components/Seo.svelte';

	let { children } = $props();

	const title = 'File storage & images — Lazypock Docs';
	const description =
		'Lazypock file storage: upload policy and limits, image presets and variants, local vs S3/R2 backends, direct uploads, the Studio media library, reference tracking and cleanup, plus the files CLI and API reference.';
</script>

<Seo {title} {description} />

<div class="max-w-3xl">
	<div class="prose-md">
		{@render children()}
	</div>
</div>
