# Manual de usuario de Harness (español — extracto)

Traducción parcial del manual en inglés. Para la documentación canónica completa, véase [`Start Here/USER MANUAL.md`](../../Start%20Here/USER%20MANUAL.md) y [`README.md`](../../README.md).

## Inicio rápido

```bash
export ANTHROPIC_API_KEY=sk-ant-...
harness setup
harness
```

Harness es un agente de código en Rust con selección neutral de proveedor y modelo. No recomienda ni prioriza un proveedor, no deduce el orden a partir de las credenciales y no añade Ollama de forma implícita. Durante `harness setup`, el usuario guarda una o más entradas exactas `proveedor:modelo`: la primera es la principal y las demás son alternativas en el orden elegido.

Incluye 18 nombres de proveedor configurados alfabéticamente y permite registrar endpoints HTTP(S) personalizados compatibles con OpenAI Chat Completions. Cada proveedor seleccionado requiere un modelo explícito.

## Comandos útiles

| Comando | Descripción |
|---------|-------------|
| `harness` | TUI interactiva |
| `harness "tarea"` | Una sola instrucción |
| `harness sessions` | Listar sesiones guardadas |
| `harness serve` | Servidor web en `127.0.0.1:8787` |
| `harness doctor` | Diagnóstico de claves y herramientas |
| `harness route show` | Mostrar la ruta guardada y su alcance |
| `harness route set proveedor:modelo [...]` | Reemplazar la ruta y conservar el orden indicado |
| `harness route model proveedor modelo` | Cambiar el modelo explícito de un proveedor |
| `harness route add/remove/move` | Añadir, quitar o reordenar una alternativa |
| `harness route custom` | Registrar un endpoint compatible personalizado |

## Atajos TUI

Consulte [`docs/SHORTCUTS.md`](../SHORTCUTS.md) para la lista completa de teclas.

## Contribuir

Véase [`CONTRIBUTING.md`](../../CONTRIBUTING.md), [`TODO.md`](../../TODO.md), [`docs/COOKBOOK.md`](../COOKBOOK.md) y [`docs/BROWSER_CDP.md`](../BROWSER_CDP.md).
