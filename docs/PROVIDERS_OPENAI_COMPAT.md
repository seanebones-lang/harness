# Providers: Mistral + OpenAI-compatible (W5.1)

Harness routes through `harness-provider-router`. Cloud OpenAI chat already used the OpenAI HTTP shape; this wave makes **generic OpenAI-compatible endpoints** and **Mistral** first-class.

## Quick start

### Mistral

```bash
export MISTRAL_API_KEY=...
# optional: pin model via config
```

```toml
[providers.mistral]
# api_key omitted → MISTRAL_API_KEY
model = "mistral-large-latest"
# base_url defaults to https://api.mistral.ai/v1
```

Or rely on env-only auto-registration when `MISTRAL_API_KEY` is set (same pattern as OpenAI/Anthropic/xAI).

### Generic OpenAI-compatible proxy / local server

```toml
[providers.openai-compatible]
api_key = "optional-or-empty"
model = "gpt-4o-mini"
base_url = "http://127.0.0.1:8000/v1"   # required
```

Any **custom table name** with `base_url` is also treated as OpenAI-compatible under that name:

```toml
[providers.my-vllm]
api_key = ""
model = "meta-llama/..."
base_url = "http://127.0.0.1:8000/v1"
```

`base_url` must be the API root that accepts `/chat/completions` (usually ends with `/v1`).

### NVIDIA (OpenAI-compatible)

NVIDIA's hosted API [build.nvidia.com](https://build.nvidia.com) exposes an OpenAI-format `/v1` endpoint, so route through the `openai-compatible` provider:

```toml
[providers.openai-compatible]
api_key = "...your-nvidia-key..."    # or NVIDIA_API_KEY
model = "deepseek-ai/deepseek-v4-flash-0731"
base_url = "https://integrate.api.nvidia.com/v1"
```

Then select as the router default:

```toml
[router]
default = "openai-compatible"
```

Useful hosted models: `deepseek-ai/deepseek-v4-flash-0731` (fast + reliable), `nvidia/nemotron-3-super-120b-a12b` (120B MoE reasoning), `nvidia/nemotron-3-ultra-550b-a55b` (flagship, can return 503 under load).

### Stock OpenAI

```toml
[providers.openai]
api_key = "sk-..."   # or OPENAI_API_KEY
model = "gpt-5.5"
# base_url optional override
```

## Router notes

- Kind is the `[providers.<kind>]` key passed to `build_provider`.
- Kinds: `anthropic`, `xai`, `openai`, `mistral`, `openai-compatible` / `compatible`, `ollama`, `mlx`.
- Fallback chain default order includes `mistral` after `openai`.
- Smart default preference: anthropic → xai → openai → **mistral** → ollama → mlx.

## Implementation

| Piece | Path |
|-------|------|
| OpenAI HTTP client + `provider_name` | `crates/harness-provider-openai` |
| `OpenAIConfig::mistral` | same |
| `build_provider` kinds | `crates/harness-provider-router` |
| Env auto-insert `mistral` | `ProviderRouter::from_config` |
| Config examples | `config/default.toml` |

## Tests

```bash
cargo test -p harness-provider-router build_provider
cargo test -p harness-provider-openai
```
