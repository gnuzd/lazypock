# LazyPock Example Apps

Frontend examples that consume the LazyPock server from
[`../docker/`](../docker/) (or any LazyPock instance).

Planned / work in progress — add an app per framework:

- `react/` — React + Vite
- `svelte/` — SvelteKit
- `vue/` — Vue + Vite
- `next/` — Next.js
- …

Every app uses the published npm SDK:

```bash
npm install lazypock
```

```ts
import { LazypockClient } from "lazypock";

const client = new LazypockClient({ baseUrl: "http://localhost:4000/api" });
await client.login("admin@lazypock.app", "admin123"); // superuser

const posts = await client.collection("posts").getList(1, 30);
```

See the SDK repo — [github.com/gnuzd/lazypock-ts](https://github.com/gnuzd/lazypock-ts) —
for the full API.
