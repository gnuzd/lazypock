import type { Snippet } from 'svelte';

const _header = $state<{ current: Snippet | undefined }>({ current: undefined });
const _body = $state<{ current: Snippet | undefined }>({ current: undefined });
const _footer = $state<{ current: Snippet | undefined }>({ current: undefined });

export function setSidebar(header?: Snippet, body?: Snippet, footer?: Snippet) {
	_header.current = header;
	_body.current = body;
	_footer.current = footer;
}

export function getSidebar() {
	return {
		get header() {
			return _header.current;
		},
		get body() {
			return _body.current;
		},
		get footer() {
			return _footer.current;
		}
	};
}
