<script lang="ts">
	import Button from '#lib/components/ui/Button.svelte';
	import { Box } from '@lucide/svelte';
	import Terminal from './Terminal.svelte';

	const { children } = $props();
	const version = '';
</script>

<div class="flex h-dvh w-full divide-x divide-base-300">
	<div class="hidden h-full flex-1 flex-col justify-between bg-base-200 md:flex">
		<div class="flex items-center gap-3 p-8">
			<Button shape="square" variant="neutral">
				<Box size={20} />
			</Button>

			<div class="flex flex-col">
				<span class="text-sm font-bold">Studio</span>
				<span class="text-sm">lazypock</span>
			</div>
		</div>

		<div class="flex flex-col gap-5 p-8">
			<div class="flex flex-col gap-1">
				<h2 class="font-medium text-base-content/70">
					Everything your backend does, in one studio.
				</h2>
				<p class="text-sm text-base-content/70">
					Collections, records, logs and settings — and the same API from your own code.
				</p>
			</div>
			<Terminal />
		</div>

		<div class="p-8 text-sm text-base-content/70">lazypock studio · {version}</div>
	</div>
	<div class="flex flex-1 flex-col items-center justify-center gap-10">
		<div class="hidden items-center max-sm:flex">
			<div class="flex gap-3">
				<Button shape="square" variant="neutral" size="lg">
					<Box size={36} />
				</Button>

				<div class="flex flex-col">
					<span class="text-lg font-bold">Studio</span>
					<span class="text-sm">lazypock</span>
				</div>
			</div>
		</div>

		{@render children()}
	</div>
</div>
