declare global {
	interface Window {
		tinymce: {
			init: (opts: Record<string, unknown>) => void;
		};
	}
}

export {};
