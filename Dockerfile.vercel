# syntax=docker/dockerfile:1

# ---- build ------------------------------------------------------------------
FROM rust:1-bookworm AS builder

# SQL is verified at compile time against the committed .sqlx cache, so the build
# needs no reachable database. See the "Database" section of the README.
ENV SQLX_OFFLINE=true

RUN rustup target add wasm32-unknown-unknown \
    && cargo install cargo-leptos --locked

WORKDIR /app

# Manifests first, so the dependency download layer survives source-only changes.
COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src && echo 'fn main() {}' > src/main.rs && echo '' > src/lib.rs \
    && cargo fetch \
    && rm -rf src

COPY . .

# cargo-leptos has moved the server binary between target/release and
# target/server/release across versions, so locate it rather than assume.
RUN cargo leptos build --release \
    && mkdir -p /out \
    && cp "$(find target -maxdepth 3 -type f -name pricey -perm -u+x ! -path '*/deps/*' | head -1)" /out/pricey \
    && cp -r target/site /out/site

# ---- runtime ----------------------------------------------------------------
FROM debian:bookworm-slim AS runtime

# ca-certificates is needed twice over: for Neon's TLS and for scraping https pages.
# Leaving it out fails at runtime rather than at build time.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

RUN useradd --create-home --uid 10001 pricey
WORKDIR /app

COPY --from=builder /out/pricey /app/pricey
COPY --from=builder /out/site /app/site

ENV LEPTOS_OUTPUT_NAME=pricey \
    LEPTOS_SITE_ROOT=site \
    LEPTOS_SITE_PKG_DIR=pkg \
    LEPTOS_SITE_ADDR=0.0.0.0:8080 \
    RUST_LOG=info,pricey=debug

USER pricey
EXPOSE 8080

# PORT, when the platform sets it, overrides LEPTOS_SITE_ADDR (see src/main.rs).
CMD ["/app/pricey"]
