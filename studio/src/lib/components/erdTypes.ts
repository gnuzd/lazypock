import type { Node } from '@xyflow/svelte';

export interface CollectionNodeData {
	name: string;
	type: string;
	fields: Record<string, unknown>[];
	[key: string]: unknown;
}

export type CollectionNode = Node<CollectionNodeData, 'collection'>;
