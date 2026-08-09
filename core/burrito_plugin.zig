// Burrito plugin — runs inside the wrapper before the payload is extracted/launched.
//
// Problem: Burrito caches the extracted release in an install dir keyed ONLY on
// app version (`{release}_erts-{erts}_{version}`). If the version never changes
// (e.g. mix.exs stayed at 0.1.0 across many releases), every new binary reuses
// the OLD cached extraction and the new code — including the boot-time
// `migrate!()` — never runs.
//
// Fix: on every launch, delete stale cached install dirs — any sibling version
// dir other than the one this binary is about to use. The wrapper then
// re-extracts the fresh payload from this binary.
//
// IMPORTANT: never delete `install_dir` itself or its parent. The wrapper has
// already created `install_dir` and will unpack into it right after this
// plugin returns — deleting it causes a FileNotFound crash.
const std = @import("std");
const Io = std.Io;

pub fn burrito_plugin_entry(install_dir: []const u8, program_manifest_json: []const u8) void {
    _ = program_manifest_json;

    // install_dir is `<parent>/lazypock_erts-<erts>_<version>`.
    const parent = std.fs.path.dirname(install_dir) orelse return;
    const current_name = std.fs.path.basename(install_dir);

    // Delete every sibling version dir except the current one.
    var arena = std.heap.ArenaAllocator.init(std.heap.page_allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    // Delete every sibling version dir except the current one.
    var dir = Io.Dir.openDirAbsolute(io, parent, .{ .access_sub_paths = true, .iterate = true }) catch return;
    defer dir.close(io);

    var itr = dir.iterate();
    while (itr.next(io) catch null) |entry| {
        if (entry.kind != .directory) continue;
        if (std.mem.eql(u8, entry.name, current_name)) continue;
        // Only touch lazypock-style cache dirs.
        if (!std.mem.startsWith(u8, entry.name, "lazypock_erts-")) continue;

        const stale_path = std.fs.path.join(allocator, &[_][]const u8{ parent, entry.name }) catch continue;
        Io.Dir.cwd().deleteTree(io, stale_path) catch {};
    }
}

// Not a root module — compiled as a Burrito plugin module. This test block
// only guards syntax/type-checking via `zig test`.
const io = std.Options.debug_io;
test "plugin removes stale cached install dirs but keeps current" {
    const test_root = "/tmp/lazypock_plugin_test/.burrito";
    Io.Dir.cwd().deleteTree(io, "/tmp/lazypock_plugin_test") catch {};
    Io.Dir.cwd().createDirPath(io, test_root ++ "/lazypock_erts-17.0.3_0.1.0") catch {};
    Io.Dir.cwd().createDirPath(io, test_root ++ "/lazypock_erts-17.0.3_0.2.0") catch {};

    burrito_plugin_entry(test_root ++ "/lazypock_erts-17.0.3_0.2.0", "{}");

    // Stale is deleted, current is kept (the wrapper will re-extract only if
    // _metadata.json is missing; we must NOT break the current install dir).
    const stale_gone = Io.Dir.cwd().access(io, test_root ++ "/lazypock_erts-17.0.3_0.1.0", .{}) catch null;
    const current_kept = Io.Dir.cwd().access(io, test_root ++ "/lazypock_erts-17.0.3_0.2.0", .{}) catch null;
    try std.testing.expect(stale_gone == null);
    try std.testing.expect(current_kept != null);

    Io.Dir.cwd().deleteTree(io, "/tmp/lazypock_plugin_test") catch {};
}
