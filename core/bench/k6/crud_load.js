// k6 HTTP load test for the LazyPock REST API.
//
// Prerequisites:
//   - k6 installed: https://k6.io/docs/get-started/installation/
//   - a running LazyPock server with a `posts` collection and, for the write
//     scenario, a token: TOKEN=<auth-token>
//
// Run:
//   BASE_URL=http://localhost:4000/api TOKEN=<token> k6 run bench/k6/crud_load.js
//
// No baselines exist yet, so the target thresholds below are recorded as
// comments rather than enforced. Once a baseline is captured, move them into
// `options.thresholds` to turn them into pass/fail gates.
//
// Targets (to establish):
//   GET  /posts?perPage=30           p99 < 20ms @ 500 RPS
//   POST /posts                      p99 < 30ms @ 200 RPS
//   GET  /posts?filter=<complex>     record a baseline

import http from "k6/http";
import { check } from "k6";

const BASE = __ENV.BASE_URL || "http://localhost:4000/api";
const TOKEN = __ENV.TOKEN || "";
const COLLECTION = __ENV.COLLECTION || "posts";

export const options = {
	scenarios: {
		list: {
			executor: "ramping-vus",
			startVUs: 10,
			stages: [
				{ duration: "30s", target: 100 },
				{ duration: "30s", target: 500 },
				{ duration: "30s", target: 0 },
			],
			exec: "listRecords",
		},
		filtered: {
			executor: "constant-vus",
			vus: 20,
			duration: "30s",
			startTime: "95s",
			exec: "listFiltered",
		},
		create: {
			executor: "ramping-arrival-rate",
			startRate: 10,
			timeUnit: "1s",
			preAllocatedVUs: 50,
			maxVUs: 200,
			stages: [
				{ duration: "15s", target: 50 },
				{ duration: "15s", target: 200 },
				{ duration: "15s", target: 0 },
			],
			startTime: "130s",
			exec: "createRecord",
		},
	},
	// thresholds: {
	//   "http_req_duration{scenario:list}": ["p(99)<20"],
	//   "http_req_duration{scenario:create}": ["p(99)<30"],
	// },
};

function headers() {
	const h = { "Content-Type": "application/json" };
	if (TOKEN) h.Authorization = `Bearer ${TOKEN}`;
	return h;
}

export function listRecords() {
	const res = http.get(`${BASE}/${COLLECTION}?perPage=30`, { headers: headers() });
	check(res, { "list 200": (r) => r.status === 200 });
}

export function listFiltered() {
	const filter = encodeURIComponent(
		"(title ~ 'x' && views > 5) || (published = true && rating >= 4)",
	);
	const res = http.get(`${BASE}/${COLLECTION}?perPage=30&filter=${filter}`, {
		headers: headers(),
	});
	check(res, { "filtered 200": (r) => r.status === 200 });
}

export function createRecord() {
	const body = JSON.stringify({
		title: `bench-${__VU}-${__ITER}-${Date.now()}`,
		body: "load-test record",
	});
	const res = http.post(`${BASE}/${COLLECTION}`, body, { headers: headers() });
	check(res, { "create 200/201": (r) => r.status === 200 || r.status === 201 });
}
