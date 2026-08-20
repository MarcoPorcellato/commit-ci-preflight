# Troubleshooting CCP Preflight

## Docker Container Lifecycle
If a preflight run is cancelled via `SIGINT` (Ctrl+C), CCP sends a termination signal to all spawned containers.

### Resource Limits
Configure container memory ceilings using `--memory-limit`:
```bash
ccp run --memory-limit 2G
```
