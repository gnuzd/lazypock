<script lang="ts">
	import MediaLibrary from '$lib/components/MediaLibrary.svelte';
	import '../settings.css';
</script>

<h2 class="mb-1 text-lg font-semibold">Media Library</h2>
<p class="mb-4 text-sm text-base-content/60">
	Every file uploaded through the app, the SDK or the richtext editor. Click a file to see its
	details, or delete it — deleting removes the file and all of its variants from storage, and a file
	that a record still references asks for confirmation first.
</p>

<MediaLibrary inline mode="manage" title="Media Library" />
