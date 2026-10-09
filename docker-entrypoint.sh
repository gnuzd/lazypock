#!/bin/sh
# Signal forwarding for the container release.
#
# LazyPock's launcher (Burrito) spawns the BEAM as a child and never forwards
# signals to it; as PID 1 that launcher also ignores SIGTERM. Without this shim
# a `docker stop` would sit out the whole stop timeout and then SIGKILL the VM,
# skipping Erlang's graceful shutdown. Here we forward TERM/INT to the VM —
# `init:stop/0` stops accepting, drains and closes the DB pool — and then wait
# for the launcher to reap it and exit with its status.
set -eu

lazypock "$@" &
pid=$!

# `beam.smp` is the ERTS VM process spawned by the launcher.
forward() { pkill -TERM -x beam.smp || true; }
trap forward TERM INT

# That trap interrupts `wait`, so keep waiting until the launcher is really
# gone: returning on the interrupted wait would tear the container down while
# the VM was still draining, and the teardown would kill it anyway.
status=0
while :; do
  status=0
  wait "$pid" || status=$?
  kill -0 "$pid" 2>/dev/null || break
done

exit "$status"
