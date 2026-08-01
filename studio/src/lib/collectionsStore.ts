import { client } from '$lib/client';
import { get } from 'svelte/store';
import { writable } from 'svelte/store';

/**
 * Shared collections state used by the collections layout, list page,
 * and the per-collection editor page.
 */

export const collections = writable<Record<string, unknown>[]>([]);
export const activeName = writable('');

export async function loadCollections(): Promise<Record<string, unknown>[]> {
	try {
		const res = await client.listCollections('page=1&perPage=200');
		const items = res?.items ?? [];
		collections.set(items);
		return items;
	} catch {
		return [];
	}
}

export function subscribeToCollectionChanges(): () => void {
	const cb = (e: { event: string }) => {
		if (e.event === 'create' || e.event === 'update' || e.event === 'delete') {
			loadCollections();
		}
	};
	client.realtime.subscribe('collections', cb);
	return () => client.realtime.unsubscribe('collections', cb);
}

export function getCollections(): Record<string, unknown>[] {
	return get(collections);
}
