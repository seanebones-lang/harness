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
