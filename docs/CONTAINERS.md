# Container use

The Docker build uses Rust 1.95.0 and Cargo.lock, includes the embedded web UI and build script, and runs the CLI as an unprivileged user. Docker execution is a separate release check; a successful macOS Cargo build does not validate this image.

Build and inspect the image:

```sh
docker build -t harness:local .
docker run --rm harness:local --version
```

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
