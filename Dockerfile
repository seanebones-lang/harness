LABEL version="0.1.2-beta"
LABEL license="LicenseRef-NextEleven-Proprietary"
LABEL description="Harness — Multi-Provider Rust Coding Agent"

# ── Stage 1: builder ────────────────────────────────────────────────────────
FROM rust:1.76-slim-bookworm AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    libsqlite3-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Cache dependencies by copying manifests first
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
COPY src/ src/
COPY extensions/ extensions/
COPY apps/ apps/
COPY config/ config/
COPY tests/ tests/

RUN cargo build --release

# ── Stage 2: runtime ────────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime

LABEL version="0.1.2-beta"
LABEL license="LicenseRef-NextEleven-Proprietary"
LABEL description="Harness — Multi-Provider Rust Coding Agent"

RUN apt-get update && apt-get install -y --no-install-recommends \
    libssl3 \
    libsqlite3-0 \
    ca-certificates \
    git \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/harness /usr/local/bin/harness

RUN chmod +x /usr/local/bin/harness

WORKDIR /workspace

ENTRYPOINT ["harness"]
