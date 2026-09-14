# Container use

The Docker build uses Rust 1.95.0 and Cargo.lock, includes the embedded web UI and build script, and runs the CLI as an unprivileged user. Docker execution is a separate release check; a successful macOS Cargo build does not validate this image.

Build and inspect the image:

```sh
docker build -t harness:local .
docker run --rm harness:local --version
bash scripts/smoke_container.sh harness:local
```

The smoke script checks the unmodified runtime image runs as UID 10001 with writable workspace/state and an unwritable executable. It then adds Python in a disposable test image and runs isolated CLI and synthetic provider/HTTP integration checks as that same user. It does not mount host credentials. Set `DOCKER_CONTEXT` to select a dedicated daemon without changing your global Docker context.

The Compose recipe uses a dedicated Harness state volume and binds this repository at `/workspace`. It does not mount the host's global Harness credentials. Files in a bound workspace must be writable by the container user (UID 10001) when editing is intended.

For a local Ollama route, start Ollama, pull a model you chose, and register its container-network endpoint. Replace `YOUR_MODEL` with that exact model identifier:

```sh
docker compose up -d ollama
docker compose exec ollama ollama pull YOUR_MODEL
docker compose run --rm harness route custom local-container --base-url http://ollama:11434/v1 --model YOUR_MODEL --add --global
docker compose run --rm harness
```

For a hosted service, run `docker compose run --rm harness setup` and pass the provider's credential environment variable explicitly to subsequent `docker compose run` commands. Harness does not choose a provider or model for you.

Ollama's published port binds only to loopback. Optional tools that execute subprocesses remain dependent on what is installed inside the image; the runtime includes Git, TLS certificates and a POSIX shell, not every host development tool.

To run the full Rust workspace suite in Linux using the cached build stage, install Git in the disposable test container. The minimal compilation stage does not need Git to build, but checkpoint and project tests invoke it:

```sh
docker build --target builder -t harness:build .
docker run --rm --workdir /build harness:build sh -ec \
  'apt-get update -qq && apt-get install -y --no-install-recommends git >/dev/null && cargo test --locked --workspace --all-features'
```
