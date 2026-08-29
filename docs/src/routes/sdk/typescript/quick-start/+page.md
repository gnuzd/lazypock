---
title: Quick Start
---

# Quick Start

```typescript
import { LazypockClient } from 'lazypock';

const client = new LazypockClient({ baseUrl: 'http://localhost:4000/api' });

// Superuser login
await client.login('admin@example.com', 'password');

// Or auth collection login
await client.login('user@example.com', 'password', 'users');
// Or using the explicit method:
await client.authWithPassword('users', 'user@example.com', 'password');

// List records
const posts = await client.collection('posts').getList(1, 30);
// or fetch all pages:
const all = await client.collection('posts').getFullList();

// Create a record
const newPost = await client.collection('posts').create({ title: 'Hello', published: true });

// Auth collections — create a user (password is optional + write-only)
const user = await client.collection('users').create({
  email: 'ada@example.com',
  password: 'correct-horse-battery' // hashed server-side, never returned
});
const session = await client.authWithPassword('users', 'ada@example.com', 'correct-horse-battery');
// session.token — stored in client.authStore for subsequent requests

// File upload
const file = await client.files.upload(fileInput.files[0]);

// Real-time subscriptions (PocketBase-style: callback-first)
client.collection('posts').subscribe((e) => console.log(e.action, e.record));
```

## Next steps

- [Type Safety](/sdk/typescript/type-safety) — codegen a fully typed client
- [Queries](/sdk/typescript/queries) — `select()`, filters, sort, expand
- [Realtime](/sdk/typescript/realtime) — live subscriptions
- [Files](/sdk/typescript/files) — uploads and file URLs
- [Auth](/sdk/typescript/auth) — auth collections and token handling
