<script lang="ts">
	import MediaLibrary from '$lib/components/MediaLibrary.svelte';
</script>

<div class="p-6">
	<h2 class="mb-1 text-lg font-semibold">Media</h2>
	<p class="mb-4 text-sm text-base-content/60">
		Every file uploaded through the app, the SDK or the richtext editor. Deleting a file removes it
		(and all of its variants) from storage; files still referenced by a record ask for confirmation
		first.
	</p>

	<MediaLibrary inline mode="manage" title="Media" />
</div>
