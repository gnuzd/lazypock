# syntax=docker/dockerfile:1
# ─────────────────────────────────────────────────────────────────────────────
# LazyPock example image.
#
# Pulls the LATEST lazypock release binary straight from GitHub releases
# (github.com/gnuzd/lazypock) — no Elixir/Erlang build toolchain needed —
# and adds ImageMagick for thumbnails/on-demand image scaling.
#
# Build:
#   docker build -t lazypock:latest example/docker
#
# Pin a specific version (optional):
#   docker build --build-arg LAZYPOCK_VERSION=0.2.2 -t lazypock:0.2.2 example/docker
# ─────────────────────────────────────────────────────────────────────────────
FROM alpine:3.20

# Fail fast on any pipe failure (curl | jq below) — alpine ships /bin/sh as busybox
SHELL ["/bin/ash", "-eo", "pipefail"]

# ImageMagick (thumbnails + scaling), ca-certificates (HTTPS: OAuth, webhooks,
# Studio), curl + jq (to resolve the latest release — removed after download).
RUN apk add --no-cache imagemagick ca-certificates curl jq

# Resolve + download the latest (or pinned) Linux x86_64 release binary.
# GitHub uploads assets as `lazypock-<version>-linux-x86_64`.
ARG LAZYPOCK_VERSION=latest
RUN set -eux; \
    if [ "$LAZYPOCK_VERSION" = "latest" ]; then \
      ASSET_URL="$(curl -fsSL https://api.github.com/repos/gnuzd/lazypock/releases/latest \
        | jq -r '.assets[] | select(.name | endswith("-linux-x86_64")) | .browser_download_url')"; \
    else \
      ASSET_URL="https://github.com/gnuzd/lazypock/releases/download/v${LAZYPOCK_VERSION}/lazypock-${LAZYPOCK_VERSION}-linux-x86_64"; \
    fi; \
    [ -n "$ASSET_URL" ] || { echo "No linux-x86_64 asset found in the latest release" >&2; exit 1; }; \
    echo "Downloading $ASSET_URL"; \
    curl -fsSL -o /usr/local/bin/lazypock "$ASSET_URL"; \
    chmod +x /usr/local/bin/lazypock; \
    apk del curl jq

# Persistent app data (migrations, hooks, seeds, uploads) — mount a volume here.
ENV LAZYPOCK_DATA_DIR=/data/lazypock
RUN mkdir -p /data/lazypock

EXPOSE 4000

ENTRYPOINT ["lazypock"]
