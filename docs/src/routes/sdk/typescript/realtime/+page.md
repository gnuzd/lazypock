---
title: Realtime
---

# Realtime

Subscribe to live record changes — PocketBase-style, callback-first.

```typescript
// Subscribe to all changes in a collection
const off = client.collection('posts').subscribe((event) => {
  console.log(event.action); // 'create' | 'update' | 'delete'
  console.log(event.record);
});

// Subscribe to a specific record only
client.collection('posts').subscribe((event) => { /* ... */ }, 'abc123');

// Unsubscribe
client.collection('posts').unsubscribe();

// ...or call the returned unsubscribe function for one-shot listeners:
off();
```

## Anonymous / rule-based realtime

Realtime subscriptions honor your API **and list rules** — matching PocketBase behavior. This means
**non-logged-in users can subscribe** to collections whose list rules are public (empty `""` string)
or anon-friendly (`@request.auth.*` filters). The SDK auto-connects the WebSocket on first use, so no
token is required to receive public change events:

```typescript
// Works without logging in, as long as the collection's list rule allows it
const off = client.collection('public_feed').subscribe((e) => {
  console.log(e.action, e.record);
});
```

## Low-level realtime service

For advanced use cases you can talk to the underlying service directly:

- `realtime.connect(opts)` — Connect to WebSocket
- `realtime.disconnect()` — Disconnect
- `realtime.subscribe(topic, callback)` — Low-level subscribe (topic like `collection:posts`)
- `realtime.unsubscribe(topic, callback?)` — Low-level unsubscribe
