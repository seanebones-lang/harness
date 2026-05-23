# Manual de usuario de Harness (español — extracto)

Traducción parcial del manual en inglés. Para la documentación canónica completa, véase [`Start Here/USER MANUAL.md`](../../Start%20Here/USER%20MANUAL.md) y [`README.md`](../../README.md).

## Inicio rápido

```bash
export ANTHROPIC_API_KEY=sk-ant-...
harness
```

Harness es un agente de código en Rust con soporte multi-proveedor (Anthropic, OpenAI, xAI, Ollama).

## Comandos útiles

| Comando | Descripción |
|---------|-------------|
| `harness` | TUI interactiva |
| `harness "tarea"` | Una sola instrucción |
| `harness sessions` | Listar sesiones guardadas |
| `harness serve` | Servidor web en `127.0.0.1:8787` |
| `harness doctor` | Diagnóstico de claves y herramientas |

## Atajos TUI

Consulte [`docs/SHORTCUTS.md`](../SHORTCUTS.md) para la lista completa de teclas.

## Contribuir

Véase [`CONTRIBUTING.md`](../../CONTRIBUTING.md), [`TODO.md`](../../TODO.md), [`docs/COOKBOOK.md`](../COOKBOOK.md) y [`docs/BROWSER_CDP.md`](../BROWSER_CDP.md).
