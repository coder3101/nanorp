# syntax=docker/dockerfile:1

# ---------------------------------------------------------------------------
# Stage 1 — build the SSR binary + hydrated WASM/CSS with cargo-leptos
# ---------------------------------------------------------------------------
# `--platform=$BUILDPLATFORM` would let us cross-compile, but cargo-leptos
# builds natively per target; we instead build on the target platform (via
# QEMU for foreign archs) and fetch a prebuilt cargo-leptos to avoid compiling
# the tool under emulation.
FROM rust:1.97-bookworm AS builder

ARG TARGETARCH
ARG CARGO_LEPTOS_VERSION=0.3.7

# System deps: clang for the bundled SQLite (rusqlite), curl to fetch
# cargo-leptos. NOTE: we deliberately do NOT install binaryen — its `wasm-opt`
# (as invoked by cargo-leptos) mangles the wasm-bindgen externref table and
# breaks hydration ("RangeError: failed to grow table"). The official Leptos
# Debian containerfile skips wasm-opt for the same reason. We precompress the
# output instead to keep transfer size small.
RUN apt-get update && apt-get install -y --no-install-recommends \
        clang \
        curl \
        ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# The hydrate (client) crate compiles to wasm.
RUN rustup target add wasm32-unknown-unknown

# Install a prebuilt cargo-leptos for the target architecture.
RUN set -eux; \
    case "${TARGETARCH}" in \
        amd64) LT_ARCH=x86_64-unknown-linux-gnu ;; \
        arm64) LT_ARCH=aarch64-unknown-linux-gnu ;; \
        *) echo "unsupported arch: ${TARGETARCH}" >&2; exit 1 ;; \
    esac; \
    curl -fsSL "https://github.com/leptos-rs/cargo-leptos/releases/download/v${CARGO_LEPTOS_VERSION}/cargo-leptos-${LT_ARCH}.tar.gz" -o /tmp/cargo-leptos.tar.gz; \
    tar -xzf /tmp/cargo-leptos.tar.gz -C /tmp; \
    find /tmp -type f -name cargo-leptos -exec install -m 0755 {} /usr/local/bin/cargo-leptos \; ; \
    rm -rf /tmp/cargo-leptos*; \
    cargo-leptos --version

WORKDIR /app
COPY . .

# Release build → target/release/nanorp + target/site/*
# --precompress emits .gz/.br alongside assets (served automatically).
# Pin a newer wasm-opt: the default (v123) mangles the wasm-bindgen externref
# table and breaks hydration ("RangeError: failed to grow table").
ENV LEPTOS_WASM_OPT_VERSION=version_131
RUN cargo leptos build --release --precompress

# ---------------------------------------------------------------------------
# Stage 2 — minimal runtime
# ---------------------------------------------------------------------------
FROM debian:bookworm-slim AS runtime

# `image.source` is what links the published package back to the repository.
LABEL org.opencontainers.image.title="NanoRP" \
      org.opencontainers.image.description="A lightweight, self-hosted AI roleplay chat." \
      org.opencontainers.image.source="https://github.com/coder3101/nanorp" \
      org.opencontainers.image.licenses="MIT"

# ca-certificates: needed for HTTPS to OpenAI-compatible providers (rustls).
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Server binary and the site assets it serves (JS/WASM/CSS + favicon).
COPY --from=builder /app/target/release/nanorp /app/nanorp
COPY --from=builder /app/target/site /app/site

# Leptos runtime configuration. Bind to all interfaces inside the container.
ENV LEPTOS_OUTPUT_NAME=nanorp \
    LEPTOS_SITE_ROOT=/app/site \
    LEPTOS_SITE_PKG_DIR=pkg \
    LEPTOS_SITE_ADDR=0.0.0.0:3000 \
    RUST_LOG=info \
    XDG_CONFIG_HOME=/data

# Persistent app data (SQLite DB, avatars, attachments) lives in /data/nanorp.
# Runs as a dedicated non-root user. Named volumes inherit the ownership set
# here on first use; for bind mounts, chown the host directory to uid 10001
# (or override with `docker run --user`).
RUN useradd --system --uid 10001 --user-group --home-dir /data --shell /usr/sbin/nologin nanorp \
    && mkdir -p /data \
    && chown -R nanorp:nanorp /data
VOLUME ["/data"]

USER nanorp

EXPOSE 3000

CMD ["/app/nanorp"]
