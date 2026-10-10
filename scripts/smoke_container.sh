#!/usr/bin/env bash
# Verify an already-built runtime image without mounting user state or credentials.
# DOCKER_CONTEXT may select an isolated daemon without changing the global context.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
IMAGE="${1:-harness:local}"
TEST_IMAGE="harness-runtime-smoke:run-$$"
cleanup() { docker image rm "$TEST_IMAGE" >/dev/null 2>&1 || true; }
trap cleanup EXIT

docker run --rm "$IMAGE" --version
# Check the actual release image before adding test-only Python tooling.
docker run --rm --entrypoint sh "$IMAGE" -ec '
  test "$(id -u)" = 10001
  test -w /workspace
  test -w /home/harness/.harness
  test ! -w /usr/local/bin/harness
  harness doctor
'
docker build --build-arg "RUNTIME_IMAGE=$IMAGE" -t "$TEST_IMAGE" -f - "$ROOT" <<'DOCKERFILE'
ARG RUNTIME_IMAGE
FROM ${RUNTIME_IMAGE}
USER root
RUN apt-get update && apt-get install -y --no-install-recommends python3 && rm -rf /var/lib/apt/lists/*
COPY scripts/smoke_runtime.py scripts/smoke_agent.py scripts/smoke_work.py scripts/test_document_extract.py /opt/harness-smoke/
COPY assets/workflows/documents/scripts/extract.py /opt/assets/workflows/documents/scripts/extract.py
USER harness
ENTRYPOINT ["sh", "-ec"]
CMD ["python3 /opt/harness-smoke/smoke_runtime.py /usr/local/bin/harness && python3 /opt/harness-smoke/smoke_agent.py /usr/local/bin/harness && python3 /opt/harness-smoke/smoke_work.py /usr/local/bin/harness"]
DOCKERFILE
docker run --rm "$TEST_IMAGE"
printf '%s\n' 'CONTAINER_RUNTIME_SMOKE_OK'
