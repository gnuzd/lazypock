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
