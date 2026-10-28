import { defineConfig, devices } from '@playwright/test';

// Boots the real server and the Studio dev server, then drives them. The server
// listens on 4000, which is where `vite.config.ts` proxies `/api`, so the Studio
// talks to it exactly as it would in development.
const DATABASE_URL =
	process.env.LAZYPOCK_DATABASE_URL ??
	'postgres://postgres:postgres@localhost:5432/lazypock_dev?sslmode=disable';
const SERVER_BIN = process.env.LAZYPOCK_E2E_BIN ?? '../target/debug/lazypock';
const FILES_DIR = process.env.LAZYPOCK_E2E_FILES ?? '/tmp/lazypock-e2e-files';

// The suite signs in once per test (~17 of them) and the Studio refreshes its token
// on every navigation, which passes the default 20 credential requests per minute
// and turns whichever test runs last into an unexplained 429. The limiter guards
// against guessing from outside, not against this machine, so it is off for the test
// server — `0` is its documented off switch.
const SERVER_ENV = [
	`LAZYPOCK_DATABASE_URL='${DATABASE_URL}'`,
	`LAZYPOCK_FILES_DIR='${FILES_DIR}'`,
	'LAZYPOCK_RATE_LIMIT_AUTH=0'
].join(' ');

export default defineConfig({
	testDir: './e2e',
	timeout: 60_000,
	expect: { timeout: 10_000 },
	fullyParallel: false,
	workers: 1,
	reporter: [['list']],
	use: {
		baseURL: 'http://127.0.0.1:5173',
		...devices['Desktop Chrome']
	},
	webServer: [
		{
			command: `${SERVER_ENV} '${SERVER_BIN}' serve`,
			url: 'http://127.0.0.1:4000/api/health',
			reuseExistingServer: !process.env.CI,
			timeout: 120_000
		},
		{
			command: 'bun run dev --port 5173 --host 127.0.0.1',
			url: 'http://127.0.0.1:5173/_/login',
			reuseExistingServer: !process.env.CI,
			timeout: 120_000
		}
	]
});
