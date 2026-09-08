<script lang="ts">
  // Layered architecture diagram, rendered as one data-driven SVG.
  // Colors come from the theme CSS variables (--color-*), so the diagram
  // follows the site theme automatically (light today, dark-ready later).
  const X0 = 20;
  const X1 = 860;
  const INNER_X0 = 44;
  const INNER_X1 = 836;
  const COL_GAP = 16;
  const ARROW_GAP = 26;
  const START_Y = 18;
  const BOTTOM_PAD = 8;

  interface Col {
    title: string;
    lines: string[];
    span: number;
  }
  interface Layer {
    label: string;
    chip?: string;
    cols: Col[];
  }

  const layers: Layer[] = [
    {
      label: "Clients & admin",
      cols: [
        {
          title: "Studio — admin UI",
          lines: [
            "collections · records · fields",
            "rules · indexes · API keys · logs",
            "served by Phoenix at /_/ (SvelteKit SPA)",
          ],
          span: 7,
        },
        {
          title: "Your app (via SDK)",
          lines: [
            "REST /api/* · realtime WebSocket /socket",
            "official SDKs — TS · Swift · Android · Godot",
            "auth tokens, queries, realtime, files",
          ],
          span: 8,
        },
      ],
    },
    {
      label: "HTTP & API (Phoenix)",
      chip: "part of the Lazypock binary ↓",
      cols: [
        {
          title: "Router — LazypockWeb",
          lines: [
            "dynamic CRUD on /api/:collection",
            "/api/:collection/auth-* login & refresh",
            "static: files · settings · logs · crons",
          ],
          span: 7,
        },
        {
          title: "Plugs — every request passes through",
          lines: [
            "Auth.Plug verifies JWT (superuser + user)",
            "RequestLogger → _request_logs",
            "ConnectionId (realtime origin-exclusion)",
            "CustomRoutes from on_before_serve hooks",
          ],
          span: 8,
        },
      ],
    },
    {
      label: "Dynamic engines",
      cols: [
        {
          title: "Schema / DDL engine",
          lines: [
            "creates real Postgres tables & columns",
            "TypeMapper: field ↔ column types",
            "metadata in _collections / _fields",
            "auto-registers existing tables on boot",
          ],
          span: 5,
        },
        {
          title: "Hooks engine (~80 events)",
          lines: [
            "file-based Elixir hook modules",
            "before/after chain, abort w/ error",
            "app: bootstrap · before_serve · cron",
            "custom API routes (routerAdd-style)",
          ],
          span: 5,
        },
        {
          title: "Rule enforcer",
          lines: [
            "PocketBase-style rules per action",
            "nil → superuser only · empty → public",
            "filter string → SQL WHERE + params",
            "superuser & manageRule bypass",
          ],
          span: 5,
        },
      ],
    },
    {
      label: "Generic data layer",
      cols: [
        {
          title: "Dynamic queries",
          lines: [
            "GenericRecord over real tables",
            "filter · sort · expand · paginate",
            "relation fields → joins / expand",
          ],
          span: 5,
        },
        {
          title: "Files",
          lines: [
            "multipart uploads → /api/files",
            "adapter: local disk or S3 / R2",
            "thumbs + on-demand scaling",
          ],
          span: 5,
        },
        {
          title: "Realtime broadcaster",
          lines: [
            "CRUD event after every successful op",
            "Phoenix Channels PubSub fan-out",
            "origin excluded by connectionId",
          ],
          span: 5,
        },
      ],
    },
    {
      label: "PostgreSQL",
      chip: "external — connect via DATABASE_URL",
      cols: [
        {
          title: "PostgreSQL",
          lines: [
            "your data lives in real tables with real columns — no lock-in",
            "meta: _collections · _fields · _files · _superusers · _request_logs",
            "types, indexes, relations, JSONB — full Postgres power",
          ],
          span: 15,
        },
      ],
    },
  ];

  // Layout pass — turn the layer definitions into absolute coordinates.
  interface PlacedLayer {
    label: string;
    chip?: string;
    bandY: number;
    bandH: number;
    boxTop: number;
    boxH: number;
    nextY?: number; // top of the next layer (for the arrow)
    cols: { x: number; w: number; title: string; lines: string[] }[];
  }

  const placed: PlacedLayer[] = [];
  let y = START_Y;
  for (const layer of layers) {
    const n = layer.cols.length;
    const colSpans = layer.cols.map((c) => c.span);
    const spanTotal = colSpans.reduce((a, b) => a + b, 0);
    const usable = INNER_X1 - INNER_X0 - COL_GAP * (n - 1);
    const contentLines = Math.max(...layer.cols.map((c) => c.lines.length));
    const boxH = 46 + (contentLines - 1) * 18 + 16;
    const bandH = 40 + boxH + 20;
    const boxTop = y + 40;
    let x = INNER_X0;
    const cols = layer.cols.map((c) => {
      const w = (usable * c.span) / spanTotal;
      const col = { x, w, title: c.title, lines: c.lines };
      x += w + COL_GAP;
      return col;
    });
    placed.push({ label: layer.label, chip: layer.chip, bandY: y, bandH, boxTop, boxH, cols });
    y += bandH + ARROW_GAP;
  }
  for (let i = 0; i < placed.length - 1; i++) {
    placed[i].nextY = placed[i + 1].bandY;
  }
  const svgH = y - ARROW_GAP + BOTTOM_PAD;
  const arrowX = (X0 + X1) / 2;

  // The "single binary" boundary wraps everything except Postgres (bands 1..3
  // in the placed array) — the BEAM app + bundled Studio assets.
  const binY = placed[1].bandY - 8;
  const binBottom = placed[3].bandY + placed[3].bandH + 8;
  const binX = 10;
  const binW = X1 - X0 + 20;
</script>

<div class="overflow-x-auto">
  <svg
    viewBox="0 0 880 {svgH}"
    class="arch-svg"
    role="img"
    aria-label="Layered architecture diagram of Lazypock: clients and admin UI at the top, the Phoenix HTTP/API layer, the dynamic engines, the generic data layer, all inside a single-binary boundary, above PostgreSQL."
    style="min-width: 660px"
  >
    <!-- Single-binary boundary (Phoenix + engines + data layer) -->
    <rect class="arch-boundary" x={binX} y={binY} width={binW} height={binBottom - binY} rx="18" />

    {#each placed as L}
      <g>
        <!-- Band -->
        <rect class="arch-band" x={X0} y={L.bandY} width={X1 - X0} height={L.bandH} rx="14" />
        <text class="arch-band-label" x={INNER_X0} y={L.bandY + 26}>{L.label}</text>

        {#if L.chip}
          <g class="arch-chip">
            <rect x={X1 - 292} y={L.bandY + 10} width="272" height="19" rx="9.5" />
            <text x={X1 - 284} y={L.bandY + 23}>{L.chip}</text>
          </g>
        {/if}

        <!-- Column boxes -->
        {#each L.cols as c}
          <g>
            <rect class="arch-box" x={c.x} y={L.boxTop} width={c.w} height={L.boxH} rx="10" />
            <text class="arch-title" x={c.x + 16} y={L.boxTop + 25}>{c.title}</text>
            {#each c.lines as line, li}
              <text class="arch-line" x={c.x + 16} y={L.boxTop + 47 + li * 18}>{line}</text>
            {/each}
          </g>
        {/each}
      </g>

      <!-- Arrow to the next layer -->
      {#if L.nextY}
        <g>
          <line
            class="arch-arrow"
            x1={arrowX}
            y1={L.bandY + L.bandH - 4}
            x2={arrowX}
            y2={L.nextY - 2}
          />
          <path
            class="arch-arrowhead"
            d="M {arrowX - 4} {L.nextY - 9} L {arrowX} {L.nextY - 1} L {arrowX + 4} {L.nextY - 9} Z"
          />
        </g>
      {/if}
    {/each}
  </svg>
</div>

<style>
  .arch-svg {
    display: block;
    width: 100%;
    height: auto;
  }

  .arch-boundary {
    fill: color-mix(in oklch, var(--color-primary) 4%, transparent);
    stroke: var(--color-primary);
    stroke-width: 1.4;
    stroke-dasharray: 5 4;
    opacity: 0.75;
  }

  .arch-band {
    fill: color-mix(in oklch, var(--color-base-200) 40%, transparent);
    stroke: var(--color-base-300);
    stroke-width: 1;
  }

  .arch-band-label {
    fill: color-mix(in oklch, var(--color-base-content) 55%, transparent);
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 0.09em;
    text-transform: uppercase;
  }

  .arch-chip rect {
    fill: color-mix(in oklch, var(--color-primary) 9%, transparent);
    stroke: color-mix(in oklch, var(--color-primary) 45%, transparent);
    stroke-width: 1;
  }

  .arch-chip text {
    fill: var(--color-primary);
    font-size: 10.5px;
    font-weight: 600;
  }

  .arch-box {
    fill: var(--color-base-100);
    stroke: var(--color-base-300);
    stroke-width: 1;
  }

  .arch-title {
    fill: var(--color-base-content);
    font-size: 13px;
    font-weight: 700;
  }

  .arch-line {
    fill: color-mix(in oklch, var(--color-base-content) 72%, transparent);
    font-size: 11.5px;
  }

  .arch-arrow {
    stroke: color-mix(in oklch, var(--color-base-content) 45%, transparent);
    stroke-width: 1.5;
    fill: none;
  }

  .arch-arrowhead {
    fill: color-mix(in oklch, var(--color-base-content) 45%, transparent);
  }
</style>
