# mini-ai-router-rs

A lightweight local API gateway written in Rust that exposes OpenAI-compatible endpoints and routes requests to multiple AI providers.

## Features

- OpenAI-compatible `/v1/chat/completions` endpoint
- Anthropic `/anthropic/v1/messages` endpoint
- Streaming support for OpenAI-compatible providers
- Configurable model routing via YAML config
- RTK/token compression middleware
- GitHub Copilot adapter placeholder

## Prerequisites

- Rust stable (edition 2021)
- API keys set as environment variables (see config)

## Build

```bash
cargo build --release
```

## Run

```bash
export OPENAI_API_KEY=sk-...
export ANTHROPIC_API_KEY=sk-ant-...
./target/release/mini-ai-router-rs --config config.yaml
```

Or use a custom config path:

```bash
./target/release/mini-ai-router-rs -c /path/to/config.yaml
```

## Example Config

Copy `config.example.yaml` to `config.yaml` and edit:

```yaml
default_model: gpt-4o-mini
server:
  host: 127.0.0.1
  port: 20128
models:
  gpt-4o-mini:
    provider: openai
    api_base: https://api.openai.com/v1
    api_key_env: OPENAI_API_KEY
    model: gpt-4o-mini
  claude-sonnet:
    provider: anthropic
    api_base: https://api.anthropic.com
    api_key_env: ANTHROPIC_API_KEY
    model: claude-sonnet-4-20250514
  copilot:
    provider: copilot
    api_base: https://api.githubcopilot.com
    api_key_env: GITHUB_COPILOT_TOKEN
    model: gpt-4o
rtk:
  enabled: false
  max_message_chars: 8000
  preserve_head_chars: 2000
  preserve_tail_chars: 2000
```

## Usage

### Health check

```bash
curl http://127.0.0.1:20128/health
```

### List models

```bash
curl http://127.0.0.1:20128/v1/models
```

### Chat completion (OpenAI format)

```bash
curl http://127.0.0.1:20128/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-4o-mini",
    "messages": [{"role": "user", "content": "Hello!"}],
    "temperature": 0.7
  }'
```

### Streaming

```bash
curl http://127.0.0.1:20128/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-4o-mini",
    "messages": [{"role": "user", "content": "Count to 5"}],
    "stream": true
  }'
```

### Anthropic endpoint

```bash
curl http://127.0.0.1:20128/anthropic/v1/messages \
  -H "Content-Type: application/json" \
  -d '{
    "model": "claude-sonnet",
    "messages": [{"role": "user", "content": "Hello!"}],
    "max_tokens": 1024
  }'
```

## Using with Cline / Cursor / OpenCode

Set the base URL to `http://127.0.0.1:20128/v1` in your tool settings to route all LLM requests through mini-ai-router-rs.

## Notes

- The GitHub Copilot provider is a placeholder and is not yet implemented.
- Anthropic streaming is not yet implemented (non-streaming works).
- No database, authentication, or multi-user management is included.
