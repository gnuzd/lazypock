---
title: Files
---

# Files

Upload, delete, and build URLs for file records.

```typescript
// Upload a file
const file = await client.files.upload(fileInput.files[0]);

// Get file metadata
const meta = await client.files.getUrl(file.id);

// Delete a file
await client.files.delete(file.id);
```

## Utilities

- `getFileUrl(baseUrl, fileId)` — Construct a file URL from base URL and file ID (utility).
