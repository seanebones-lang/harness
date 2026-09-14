# Build the same pinned, locked release candidate as CI.
FROM rust:1.95.0-slim-bookworm AS builder
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev cmake clang \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /build
COPY Cargo.toml Cargo.lock rust-toolchain.toml build.rs ./
COPY crates/ crates/
COPY src/ src/
COPY static/ static/
COPY benches/ benches/
COPY tests/ tests/
COPY demo/ demo/
RUN cargo build --locked --profile release-lto --bin harness

FROM debian:bookworm-slim AS runtime
LABEL version="1.3.0" \
      license="LicenseRef-NextEleven-Proprietary" \
      description="Harness — Multi-Provider Rust Coding Agent"
RUN apt-get update && apt-get install -y --no-install-recommends \
    libssl3 ca-certificates git \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --create-home --uid 10001 harness \
    && mkdir -p /workspace /home/harness/.harness \
    && chown -R harness:harness /workspace /home/harness/.harness
COPY --from=builder /build/target/release-lto/harness /usr/local/bin/harness
USER harness
WORKDIR /workspace
ENTRYPOINT ["harness"]
CMD ["--help"]
