.PHONY: all build test lint verify smoke web-test supply-chain doc bench clean docker-build docker-run

all: build

build:
	cargo build --locked --profile release-lto

test:
	cargo test --locked --workspace --all-features

lint:
	cargo fmt --all -- --check
	cargo clippy --locked --workspace --all-targets --all-features -- -D warnings

verify: lint test smoke
	cargo clippy --locked --no-default-features -- -D warnings

smoke:
	cargo build --locked --bin harness
	HARNESS_BIN="$(CURDIR)/target/debug/harness" bash scripts/smoke_rel01.sh
	python3 scripts/smoke_agent.py target/debug/harness
	python3 scripts/smoke_build.py target/debug/harness
	python3 scripts/smoke_tui.py target/debug/harness
	python3 scripts/test_install.py

web-test:
	cd static && npm ci && npm test

supply-chain:
	cargo deny check

# Generate workspace API documentation.
doc:
	cargo doc --locked --workspace --no-deps

bench:
	cargo bench --locked --workspace

clean:
	cargo clean

docker-build:
	docker build -t harness:latest .

docker-run:
	docker compose up
