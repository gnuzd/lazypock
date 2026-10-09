# syntax=docker/dockerfile:1

# LazyPock ships the prebuilt Linux binary from the GitHub Release unchanged, so
# the image is byte-identical to the `lazypock-<version>-linux-x86_64` asset
# users download themselves. The `docker` job in .github/workflows/release.yml
# downloads that asset, verifies it against the release checksums and only then
# builds (and smoke-tests) this image.
FROM debian:bookworm-slim

# No Elixir/Erlang runtime is needed here — the binary bundles its own ERTS.
#  * imagemagick      -> image resizing/thumbnails (Lazypock.Images shells
#                        out to `magick`/`convert`; bookworm ships
#                        ImageMagick 6 = `convert`)
#  * ca-certificates  -> outbound TLS (S3/R2, SMTP)
#  * curl             -> the HEALTHCHECK below
#  * procps           -> `pkill`, used by the entrypoint to forward signals
# Packages are deliberately unpinned (DL3008): the image is rebuilt per release
# and should pick up the current Debian security fixes for them.
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
        imagemagick \
        procps \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --create-home --uid 1000 lazypock

# Burrito extracts the release into $HOME/.local/share/lazypock on first run, so
# HOME has to be writable by the runtime user. Uploads are redirected to a
# volume: the release default (`<app_dir>/priv/uploads`) lives *inside* that
# extraction directory and is wiped whenever the container is recreated.
ENV HOME=/home/lazypock \
    LAZYPOCK_STORAGE_PATH=/data/uploads \
    PORT=4000

WORKDIR /home/lazypock

COPY --chown=lazypock:lazypock lazypock /usr/local/bin/lazypock
COPY docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh

RUN install -d -o lazypock -g lazypock /data/uploads \
    && chmod 0755 /usr/local/bin/lazypock /usr/local/bin/docker-entrypoint.sh

USER lazypock

VOLUME ["/data/uploads"]
EXPOSE 4000

# /api/health is unauthenticated and reports the version plus file-queue depths.
# --start-period covers the first-boot extraction + migrations.
HEALTHCHECK --interval=15s --timeout=5s --start-period=45s --retries=3 \
    CMD curl -fsS "http://127.0.0.1:${PORT}/api/health" >/dev/null || exit 1

ENTRYPOINT ["docker-entrypoint.sh"]
