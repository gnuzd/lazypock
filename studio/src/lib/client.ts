import { createClient } from './lazypock.types';

// The studio SPA is served by the core Phoenix app, which proxies the
// Lazypock API under `/api`.
export const client = createClient({ baseUrl: '/api' });
