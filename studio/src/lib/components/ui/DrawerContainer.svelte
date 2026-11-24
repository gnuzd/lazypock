<script lang="ts">
	import { drawer } from '#lib/utils/drawer.svelte.ts';
	import DrawerWrapper from './DrawerWrapper.svelte';
</script>

{#each drawer.instances as instance (instance.id)}
	{@const Comp = instance.component}
	{@const close = () => drawer.close(instance.id)}
	<DrawerWrapper
		open={true}
		title={instance.options.title}
		size={instance.options.size}
		position={instance.options.position}
		dismissible={instance.options.dismissible}
		onclose={close}
	>
		<Comp {...instance.options.props} onClose={close} />
	</DrawerWrapper>
{/each}
