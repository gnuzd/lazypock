<script>
	import Button from '#lib/components/ui/Button.svelte';
	import Input from '#lib/components/ui/Input.svelte';
	import Plus from '@lucide/svelte/icons/plus';
	import Search from '@lucide/svelte/icons/search';
</script>

{#snippet searchIcon()}
	<Search size={16} class="text-base-content/50" />
{/snippet}

<div class="flex flex-1 divide-x divide-base-300">
	<div class="flex w-[82vw] max-w-75 flex-col justify-between divide-y divide-base-300">
		<div class="w-full p-3">
			<Input size="sm" leadingIcon={searchIcon} />
		</div>

		<div class="flex flex-1 flex-col overflow-auto p-2">list</div>
		<div class="p-3">
			<Button size="sm" variant="neutral" buttonStyle="outline" class="w-full">
				<Plus size={16} />
				New Collections
			</Button>
		</div>
	</div>
	<div class="flex-1">left</div>
</div>
