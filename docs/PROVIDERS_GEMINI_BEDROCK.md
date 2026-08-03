# Providers — Gemini & Bedrock (W5.2)

## Gemini

Uses Google’s **OpenAI-compatible** endpoint:

- Base: `https://generativelanguage.googleapis.com/v1beta/openai`
- Env: `GEMINI_API_KEY` or `GOOGLE_API_KEY`
- Default model: `gemini-2.0-flash`
- Router kind: `gemini`
- Crate: `harness-provider-gemini` (thin wrapper) + `OpenAIConfig::gemini`

```toml
[providers.gemini]
# api_key from env preferred
model = "gemini-2.0-flash"
```

```bash
export GEMINI_API_KEY=...
harness models
harness --model gemini-2.0-flash "ping"
```

## Bedrock

AWS Bedrock Runtime **Converse** (SigV4):

- Env: `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, optional `AWS_SESSION_TOKEN`
- Region: `AWS_REGION` / `AWS_DEFAULT_REGION` (default `us-east-1`)
- Model: `BEDROCK_MODEL_ID` or config `model`
- Router kind: `bedrock`
- Config `base_url` field is reused as **region override** for bedrock entries
- Test helper: `api_key = "ACCESS:SECRET"` when AWS env unset

```toml
[providers.bedrock]
model = "anthropic.claude-3-5-sonnet-20241022-v2:0"
# base_url = "us-west-2"   # region override
```

```bash
export AWS_ACCESS_KEY_ID=...
export AWS_SECRET_ACCESS_KEY=...
export AWS_REGION=us-east-1
export BEDROCK_MODEL_ID=anthropic.claude-3-5-sonnet-20241022-v2:0
```

## Verification (no live keys required)

```bash
cargo test -p harness-provider-gemini
cargo test -p harness-provider-bedrock
cargo test -p harness-provider-router build_provider_gemini
cargo test -p harness-provider-router build_provider_bedrock
```
