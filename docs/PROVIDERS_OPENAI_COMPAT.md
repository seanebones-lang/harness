# Provider-neutral routing and OpenAI-compatible endpoints

Harness treats provider selection as user configuration, not product policy. It never ranks vendors, infers a preferred model, or appends an implicit fallback. The saved route is authoritative:

```bash
harness route set anthropic:claude-sonnet-4-6 openai:gpt-5.5 ollama:qwen3-coder:30b
harness route show
```

The first entry is primary. Remaining entries are attempted in the exact order shown. A one-provider route is valid. If multiple providers are available but no route is saved, Harness stops with a setup error instead of choosing for the user.

## Built-in provider names

The setup catalogue is alphabetical and includes:

- `anthropic` — `ANTHROPIC_API_KEY`
- `bedrock` — standard AWS credentials and region
- `cerebras` — `CEREBRAS_API_KEY`
- `deepseek` — `DEEPSEEK_API_KEY`
- `fireworks` — `FIREWORKS_API_KEY`
- `gemini` — `GEMINI_API_KEY` or `GOOGLE_API_KEY`
- `groq` — `GROQ_API_KEY`
- `huggingface` — `HF_TOKEN`
- `mistral` — `MISTRAL_API_KEY`
- `mlx` — local MLX server
- `nvidia` — `NVIDIA_API_KEY`
- `ollama` — local Ollama server
- `openai` — `OPENAI_API_KEY`
- `openrouter` — `OPENROUTER_API_KEY`
- `perplexity` — `PERPLEXITY_API_KEY`
- `sambanova` — `SAMBANOVA_API_KEY`
- `together` — `TOGETHER_API_KEY`
- `xai` — `XAI_API_KEY`

Cerebras, DeepSeek, Fireworks, Groq, Hugging Face, NVIDIA, OpenRouter, Perplexity, SambaNova, and Together use Harness's OpenAI-compatible transport with provider-specific base URLs. Model IDs are always selected by the user and are not pinned by the router.

## Route commands

```bash
# Replace everything. First entry is primary.
harness route set groq:llama-3.3-70b-versatile openai:gpt-5.5

# Change a model without changing order.
harness route model groq llama-3.3-70b-versatile

# Add, remove, or move a fallback (positions are one-based).
harness route add ollama:qwen3-coder:30b
harness route add anthropic:claude-sonnet-4-6 --position 1
harness route move ollama 2
harness route remove openai

# Target a specific scope. Without a flag, Harness edits the active config.
harness route show --global
harness route set --project ollama:qwen3-coder:30b
```

Project `.harness/config.toml` is authoritative when present; it does not merge with the global file. The explicit `--global` and `--project` flags avoid accidentally editing both.

## Custom and future providers

Any service exposing an OpenAI-format chat-completions endpoint can be registered without adding a Rust adapter:

```bash
harness route custom my-cloud \
  --base-url https://example.com/v1 \
  --model vendor/model-id \
  --api-key-env MY_CLOUD_API_KEY \
  --add
```

Equivalent TOML:

```toml
[providers.my-cloud]
kind = "openai-compatible"
base_url = "https://example.com/v1"
model = "vendor/model-id"
api_key_env = "MY_CLOUD_API_KEY"

[router]
default = "my-cloud"
fallback = []
```

`base_url` is the API root to which Harness appends `/chat/completions`. API keys should remain in environment variables. `api_key` is retained only for compatibility with existing configurations.

For an unauthenticated local server (for example, a development-only loopback endpoint), omit `--api-key-env`. Harness validates that custom base URLs are absolute HTTP(S) URLs. Authentication schemes other than an optional bearer token require a native adapter.

Account-specific services such as Cloudflare Workers AI should use this custom-provider path because their base URLs cannot be represented by one global preset. Providers requiring different authentication headers or request schemas still need a native adapter.

## Direct TOML example

```toml
[providers.nvidia]
model = "deepseek-ai/deepseek-v4-flash-0731"
api_key_env = "NVIDIA_API_KEY"

[providers.mistral]
model = "mistral-large-latest"
api_key_env = "MISTRAL_API_KEY"

[providers.ollama]
model = "qwen3-coder:30b"

[router]
default = "nvidia"
fallback = ["mistral", "ollama"]
```

Harness will neither reorder this route nor insert another configured provider into it.

## Verification

```bash
cargo test -p harness-provider-router
cargo test --bin harness cli::commands::route
./target/debug/harness route show --project
```

Implementation lives in `crates/harness-provider-router/src/lib.rs` and `src/cli/commands/route.rs`.
