import { client } from '$lib/client';
import type { CollectionsMessage } from 'lazypock';
import { get, writable } from 'svelte/store';

/**
 * Shared collections state used by the collections layout, list page,
 * and the per-collection editor page.
 */

export const collections = writable<Record<string, unknown>[]>([]);
export const activeName = writable('');

export async function loadCollections(): Promise<Record<string, unknown>[]> {
	try {
		const res = await client.collections.getList({
			page: 1,
			perPage: 200
		});
		const items = (res?.items ?? []) as Record<string, unknown>[];
		collections.set(items);
		return items;
	} catch {
		return [];
	}
}

export function subscribeToCollectionChanges(): () => void {
	const cb = (e: CollectionsMessage) => {
		if (e.action === 'create' || e.action === 'update' || e.action === 'delete') {
			loadCollections();
		}
	};
	return client.collections.subscribe(cb);
}

export function getCollections(): Record<string, unknown>[] {
	return get(collections);
}
