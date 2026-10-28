// The registration surface a hook file sees, and the dispatch entry point the
// server calls. Kept in JavaScript rather than assembled in Rust so the whole
// boundary is readable in one place.

(function () {
    "use strict";

    /** event name + ":" + collection -> array of handlers. */
    const registry = new Map();
    globalThis.__lazypock_hooks = registry;

    function register(eventName) {
        return function (collection, handler) {
            if (typeof collection !== "string" || collection.length === 0) {
                throw new Error(eventName + ": a collection name is required");
            }
            if (typeof handler !== "function") {
                throw new Error(eventName + ": a handler function is required");
            }
            const key = eventName + ":" + collection;
            if (!registry.has(key)) {
                registry.set(key, []);
            }
            registry.get(key).push(handler);
        };
    }

    globalThis.onRecordBeforeCreate = register("recordBeforeCreate");
    globalThis.onRecordAfterCreate = register("recordAfterCreate");
    globalThis.onRecordBeforeUpdate = register("recordBeforeUpdate");
    globalThis.onRecordAfterUpdate = register("recordAfterUpdate");
    globalThis.onRecordBeforeDelete = register("recordBeforeDelete");
    globalThis.onRecordAfterDelete = register("recordAfterDelete");

    /**
     * `$http` — the outbound requests a hook may make.
     *
     * Synchronous, because this runtime has no job queue: the call runs on the hook
     * worker's own thread and is bounded by its timeout. Every rule (host
     * allowlist, public-address-only, https, no redirects, size caps) is enforced
     * on the Rust side; this object only shapes the calls and the responses.
     *
     *     const res = $http.get("https://api.example.com/thing");
     *     if (!res.ok) throw new Error("upstream said " + res.status);
     *     const body = res.json();
     */
    globalThis.$http = (function () {
        function send(options) {
            const request = {
                method: options && options.method ? String(options.method).toUpperCase() : "GET",
                url: options && options.url ? String(options.url) : "",
                headers: (options && options.headers) || {}
            };
            if (options && options.body !== undefined && options.body !== null) {
                request.body =
                    typeof options.body === "string" ? options.body : JSON.stringify(options.body);
            }
            if (options && options.timeoutMs !== undefined && options.timeoutMs !== null) {
                request.timeoutMs = options.timeoutMs;
            }

            const envelope = JSON.parse(__lazypock_http(JSON.stringify(request)));
            if (!envelope.ok) {
                throw new Error(envelope.error);
            }
            return {
                status: envelope.status,
                ok: envelope.status >= 200 && envelope.status < 300,
                headers: envelope.headers,
                body: envelope.body,
                text: function () {
                    return envelope.body;
                },
                json: function () {
                    try {
                        return JSON.parse(envelope.body);
                    } catch (err) {
                        throw new Error("$http: the response body is not JSON: " + err.message);
                    }
                }
            };
        }

        function shorthand(method, withBody) {
            return function (url, bodyOrOptions, maybeOptions) {
                const options = withBody
                    ? Object.assign({}, maybeOptions || {}, { method: method, url: url, body: bodyOrOptions })
                    : Object.assign({}, bodyOrOptions || {}, { method: method, url: url });
                return send(options);
            };
        }

        return {
            send: send,
            get: shorthand("GET", false),
            delete: shorthand("DELETE", false),
            post: shorthand("POST", true),
            put: shorthand("PUT", true),
            patch: shorthand("PATCH", true)
        };
    })();

    /** How many handlers are registered for an event and collection. */
    globalThis.__lazypock_count = function (eventName, collection) {
        const handlers = registry.get(eventName + ":" + collection);
        return handlers ? handlers.length : 0;
    };

    /**
     * Run every handler for an event and return the resulting event as JSON.
     *
     * A handler may mutate `e.record` in place or return a replacement object;
     * both are honoured, because both are the obvious thing to write.
     */
    globalThis.__lazypock_dispatch = function (eventName, collection, payload) {
        const handlers = registry.get(eventName + ":" + collection) || [];
        const event = JSON.parse(payload);

        for (const handler of handlers) {
            const result = handler(event);

            // An async handler cannot be awaited here: there is no job queue in
            // this context. Silently ignoring the promise would drop the handler's
            // work, so say so instead.
            if (result && typeof result.then === "function") {
                throw new Error(
                    "a hook returned a promise, which this runtime cannot await; " +
                        "an asynchronous hook belongs in the job queue"
                );
            }
            if (result && typeof result === "object") {
                event.record = result;
            }
        }

        return JSON.stringify(event);
    };
})();
