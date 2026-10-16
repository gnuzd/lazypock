# syntax=docker/dockerfile:1
#
# LazyPock — one image, one binary. It serves the API and the Studio together.
#
# Three stages: build the SPA, build the server with the SPA embedded, then keep
# only the server. Nothing from the build stages reaches the runtime image.
#
# There is deliberately no ImageMagick here. Thumbnails and scaled images are
# produced in-process (PLAN.md section 10), so the sidecar the Elixir image
# needed is gone. A deployment that sets LAZYPOCK_IMAGE_SERVICE_URL moves that
# work to a service instead, which is a network dependency rather than a host
# package.
#
# Likewise no `git` and no `pg_dump`: scheduled backups are left to the platform
# (Coolify backs up databases and application volumes on a schedule), so the
# image does not need the client tools those would require.

# ───────────────────────────────────────────────────────────────── studio ──
# The SPA. Its SDK is a published npm package (`lazypock`), so this stage needs
# nothing from outside the build context — no sibling checkout of the SDK repo.
FROM oven/bun:1.3.14 AS studio
WORKDIR /studio
COPY studio/package.json studio/bun.lock ./
RUN bun install --frozen-lockfile
COPY studio/ ./
RUN bun run build

# ───────────────────────────────────────────────────────────────── server ──
FROM rust:1.99-bookworm AS server
WORKDIR /src
# `aws-lc-rs` is rustls' default crypto provider (PLAN.md risk 12). It is C, and
# it is built here with cmake, so the toolchain has to be present.
RUN apt-get update \
 && apt-get install -y --no-install-recommends cmake \
 && rm -rf /var/lib/apt/lists/*
COPY . .
COPY --from=studio /studio/build/studio /src/studio/build/studio
# `LAZYPOCK_STUDIO_DIR` is what makes the built SPA the one that gets embedded,
# rather than whatever happened to be in `crates/studio-embed/dist`.
RUN LAZYPOCK_STUDIO_DIR=/src/studio/build/studio cargo build --release --bin lazypock

# ──────────────────────────────────────────────────────────────── runtime ──
FROM debian:bookworm-slim AS runtime

# `ca-certificates` is for outbound TLS: S3, the image service, `$http` in hooks
# and (later) OAuth endpoints. `tini` is PID 1 so signals are forwarded and
# zombies reaped; the server drains live SSE connections on SIGTERM, and that
# only happens if the signal reaches it.
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates tini \
 && rm -rf /var/lib/apt/lists/*

COPY --from=server /src/target/release/lazypock /usr/local/bin/lazypock

# Everything the server writes lives under one path, and it runs as a user that
# can write only that path. `VOLUME` is declared so a deployment that forgets to
# mount anything still keeps its data across a container replacement rather than
# losing it without a word.
# `--system` is deliberately not used: it expects a uid below 1000, and a fixed
# high uid is what makes the volume's ownership predictable on the host.
RUN useradd --uid 10001 --home-dir /data --shell /usr/sbin/nologin --no-create-home lazypock \
 && mkdir -p /data/hooks /data/files \
 && chown -R lazypock:lazypock /data
WORKDIR /data
VOLUME ["/data"]

# Set explicitly rather than relying on the defaults, which are relative and
# would otherwise nest as `/data/data/files`.
ENV LAZYPOCK_DATA_DIR=/data \
    LAZYPOCK_HOOKS_DIR=/data/hooks \
    LAZYPOCK_FILES_DIR=/data/files \
    LAZYPOCK_LISTEN_ADDR=0.0.0.0:4000 \
    LAZYPOCK_ENV=production
EXPOSE 4000

USER lazypock

# Probes the server's own `/api/health` over loopback, so it needs no extra
# package in the image.
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
  CMD ["lazypock", "healthcheck"]

# `serve` is the default subcommand but stays overridable, so one-off commands
# work without a second entrypoint:
#   docker run --rm -e LAZYPOCK_DATABASE_URL=... <image> superuser create --email a@b.c
ENTRYPOINT ["/usr/bin/tini", "--", "lazypock"]
CMD ["serve"]
