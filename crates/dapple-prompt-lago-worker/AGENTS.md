# Agent Instructions — dapple-prompt-lago-worker

Background worker binary that syncs billing events to the self-hosted Lago instance.

## Architecture

```
PostgreSQL LISTEN/NOTIFY ("lago_event_sync")
         |
  worker.rs: run_worker() loop
         |
  sync.rs: process_pending_syncs()
    1. SELECT from rev.lago_event_sync WHERE sent_at IS NULL
    2. Resolve usage record + billable entity
    3. Build LagoUsageEvent (event_builder.rs)
    4. POST to Lago API (dapple-lago client)
    5. UPDATE rev.lago_event_sync with result
```

## Key Files

| File | Purpose |
|------|---------|
| `src/main.rs` | Binary entry point, DB + Lago client init |
| `src/worker.rs` | LISTEN/NOTIFY loop + periodic sweep |
| `src/event_builder.rs` | Usage records -> LagoUsageEvent conversion |
| `src/sync.rs` | Pending sync processing with retry backoff |

## Environment Variables

| Variable | Required | Purpose |
|----------|----------|---------|
| `DAPPLE_PROMPT_DATABASE_URL` | Yes | PostgreSQL connection |
| `LAGO_API_URL` | Yes | Lago REST API endpoint |
| `LAGO_API_KEY` | Yes | Lago authentication |

## K8s Deployment

Uses `dapple-prompt-engine.yaml` template (no health checks -- background worker).
