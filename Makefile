.PHONY: all build test lint doc bench clean docker-build docker-run

# Default target
all: build

# Build the release binary (thin LTO, stripped)
build:
	cargo build --profile release-lto

# Run the full test suite (no API keys required)
test:
	cargo test --all

# Lint: clippy (deny warnings) + fmt check
lint:
	cargo clippy --all-targets --all-features -- -D warnings
	cargo fmt --all -- --check

# Generate and open workspace documentation
doc:
	cargo doc --workspace --no-deps --open

# Run benchmarks (requires nightly or bench feature)
bench:
	cargo bench --all

# Remove build artifacts
clean:
	cargo clean

# Build the Docker image
docker-build:
	docker build -t harness:latest .

# Run the Docker container interactively (Ollama local backend, no API key needed)
docker-run:
	docker compose up
