<script lang="ts">
	import { onMount } from 'svelte';

	/* ---------- Types ---------- */
	type Kind = 'plain' | 'prompt' | 'keyword' | 'string' | 'comment' | 'number' | 'type' | 'method';

	interface Piece {
		text: string;
		kind: Kind;
	}

	/* ---------- Piece helpers ---------- */
	const piece =
		(kind: Kind) =>
		(text: string): Piece => ({ text, kind });

	const plain = piece('plain');
	const prompt = piece('prompt');
	const keyword = piece('keyword');
	const string = piece('string');
	const comment = piece('comment');
	const number = piece('number');
	const ty = piece('type');
	const method = piece('method');

	/* ---------- Script ---------- */
	const SCRIPT: Piece[][] = [
		[prompt('$'), plain(' npm install lazypock')],
		[],
		[
			keyword('import'),
			plain(' {'),
			ty(' LazypockClient'),
			plain(' }'),
			keyword(' from'),
			string(" 'lazypock'"),
			plain(';')
		],
		[],
		[keyword('const'), plain(' client ='), keyword(' new'), ty(' LazypockClient'), plain('({')],
		[method('  baseUrl'), plain(':'), string(" 'http://localhost:4000/api'"), plain(',')],
		[plain('});')],
		[],
		[comment('// sign in as a superuser')],
		[
			keyword('await'),
			plain(' client.'),
			method('login'),
			plain('('),
			string("'admin@example.com'"),
			plain(','),
			string(" 'password'"),
			plain(');')
		],
		[],
		[comment('// list the newest posts')],
		[keyword('const'), plain(' posts ='), keyword(' await'), plain(' client')],
		[plain('  .'), method('collection'), plain('('), string("'posts'"), plain(')')],
		[
			plain('  .'),
			method('getList'),
			plain('('),
			number('1'),
			plain(','),
			number(' 30'),
			plain(');')
		],
		[],
		[comment('// create one')],
		[
			keyword('await'),
			plain(' client.'),
			method('collection'),
			plain('('),
			string("'posts'"),
			plain(').'),
			method('create'),
			plain('({')
		],
		[method('  title'), plain(':'), string(" 'Hello'"), plain(',')],
		[method('  published'), plain(':'), keyword(' true'), plain(',')],
		[plain('});')]
	];

	/* ---------- Timing ---------- */
	const TYPE_TICK_MS = 22;
	const HOLD_TICKS = 180;

	/* ---------- Character counts ---------- */
	const charCount = (s: string) => [...s].length;
	const TOTAL_CHARS = SCRIPT.flat().reduce((n, p) => n + charCount(p.text), 0);

	/* ---------- Precompute line offsets ---------- */
	let running = 0;
	const lines = SCRIPT.map((line) => {
		const lineStart = running;
		const pieces: Array<Piece & { start: number; len: number }> = [];
		let from = lineStart;
		for (const p of line) {
			const len = charCount(p.text);
			pieces.push({ ...p, start: from, len });
			from += len;
		}
		const lineEnd = from;
		running = lineEnd;
		return { lineStart, lineEnd, pieces };
	});

	/* ---------- Svelte 5 reactive state ---------- */
	let typed = $state(TOTAL_CHARS);

	const rendered = $derived(
		lines.map((line) => ({
			...line,
			pieces: line.pieces.map((p) => ({
				...p,
				shown: [...p.text].slice(0, Math.max(0, Math.min(typed - p.start, p.len))).join('')
			})),
			cursor: (() => {
				const at = typed;
				const here =
					line.lineStart === line.lineEnd
						? at === line.lineStart
						: at >= line.lineStart && at < line.lineEnd;
				return here && at < TOTAL_CHARS ? '▌' : '';
			})()
		}))
	);

	/* ---------- Animation ---------- */
	onMount(() => {
		const mq = window.matchMedia('(prefers-reduced-motion: reduce)');
		if (mq.matches) {
			typed = TOTAL_CHARS;
			return;
		}

		typed = 0;
		let n = 0;
		const id = setInterval(() => {
			n += 1;
			typed = n > TOTAL_CHARS + HOLD_TICKS ? 0 : n;
			if (typed === 0) n = 0;
		}, TYPE_TICK_MS);

		return () => clearInterval(id);
	});

	/* ---------- Colors (daisyUI) ---------- */
	function kindColor(kind: Kind): string {
		switch (kind) {
			case 'plain':
				return 'var(--color-base-content)';
			case 'prompt':
			case 'comment':
				return 'var(--color-neutral)';
			case 'keyword':
				return 'var(--color-primary)';
			case 'type':
			case 'method':
				return 'var(--color-info)';
			case 'string':
				return 'var(--color-success)';
			case 'number':
				return 'var(--color-warning)';
		}
	}
</script>

<div class="flex flex-col overflow-hidden rounded-box border bg-base-100">
	<div class="flex items-center gap-2 border-b px-4 py-3">
		<div class="flex gap-1.5">
			<span class="dot bg-error"></span>
			<span class="dot bg-warning"></span>
			<span class="dot bg-success"></span>
		</div>
		<span class="text-xs text-base-content/70">lazypock · typescript sdk</span>
	</div>

	<div class="flex w-full flex-col p-4">
		{#each rendered as line}
			<div class="row">
				{#each line.pieces as p}
					<span class="mono" style:color={kindColor(p.kind)}>{p.shown}</span>
				{/each}
				<span class="mono cursor">{line.cursor}</span>
			</div>
		{/each}
	</div>
</div>

<style>
	.dot {
		width: 9px;
		height: 9px;
		border-radius: 9999px;
	}

	/* Every row has a fixed height, so the panel never grows while typing. */
	.row {
		width: 100%;
		display: flex;
		flex-direction: row;
		align-items: center;
		height: 18px;
	}

	.mono {
		font-size: 0.75rem;
		font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
		white-space: pre; /* keeps the leading spaces of each piece */
	}

	.cursor {
		color: var(--color-base-content);
	}
</style>
