# Plan: demo-studio Single Connection Point

## Objective

**demo-studio connects ONLY to dapple-prompt-api. No other services.**

```
┌─────────────────┐         ┌─────────────────────┐
│  demo-studio    │────────►│  dapple-prompt-api  │
└─────────────────┘         └─────────────────────┘
        │                              │
        │                              ▼
        │                   ┌─────────────────────┐
        │                   │  conduit, postgres, │
        │                   │  redis, etc.        │
        │                   └─────────────────────┘
        │
        ╳ NO direct connections to anything else
```

## Requirements

### 1. Single Environment Variable

demo-studio configuration should have exactly ONE URL:

```env
# ONLY this
DAPPLE_API_URL=http://localhost:8080

# REMOVE all of these
CONDUIT_URL=...
DATABASE_URL=...
REDIS_URL=...
TTS_*=...
```

### 2. Remove All Non-API Connections

Search and remove any code that connects to:
- Conduit (TTS service)
- PostgreSQL directly
- Redis directly
- Any other backend service

### 3. All Features Go Through API

| Feature | Flow |
|---------|------|
| TTS | Dispatch → workflow runs `prompt send "voice"` → asset created → dispatch result has `asset_object_id` |
| Asset content | `GET /api/assets/{id}/content` (stream audio/images/etc.) |
| Asset metadata | `GET /api/assets/{id}` |
| Journeys | `GET/POST /api/journeys/...` |
| Dispatch | `POST /api/dispatch/...` |
| Everything else | `/api/...` |

**Important:** demo-studio does NOT call `/api/synthesize` directly. TTS happens inside dispatch workflows. Demo-studio only:
1. Triggers dispatch (via journey/playbook APIs)
2. Receives dispatch result with asset references
3. Fetches asset content when needed for playback/display

## Search Patterns

```bash
# Find any non-API URLs
grep -rE "(localhost|127\.0\.0\.1):[0-9]+" --include="*.ts" --include="*.tsx" --include="*.env*"

# Find service-specific references
grep -ri "conduit\|redis\|postgres\|database" --include="*.ts" --include="*.tsx" --include="*.env*"

# Find hardcoded ports (API should be the only one)
grep -rE ":(8081|5432|6379)" --include="*.ts" --include="*.tsx"
```

## Acceptance Criteria

- [ ] demo-studio config has only `DAPPLE_API_URL` (or equivalent single API URL)
- [ ] No imports or code referencing conduit, redis, postgres, or other services
- [ ] All functionality works through dapple-prompt-api
- [ ] Grep searches above return no results

## TTS Flow (for clarity)

```
┌─────────────────┐                      ┌─────────────────────┐
│  demo-studio    │──── trigger ────────►│  dapple-prompt-api  │
│                 │     dispatch         │                     │
│                 │                      │  ┌───────────────┐  │
│                 │                      │  │ dispatch      │  │
│                 │                      │  │ runner        │  │
│                 │                      │  └───────┬───────┘  │
│                 │                      │          │          │
│                 │                      │          ▼          │
│                 │                      │  ┌───────────────┐  │
│                 │                      │  │ nushell       │  │
│                 │                      │  │ prompt send   │  │
│                 │                      │  │ "narrator"    │  │
│                 │                      │  └───────┬───────┘  │
│                 │                      │          │          │
│                 │                      │          ▼          │
│                 │                      │  ┌───────────────┐  │
│                 │                      │  │ conduit       │  │
│                 │                      │  │ TTS service   │  │
│                 │                      │  └───────┬───────┘  │
│                 │                      │          │          │
│                 │                      │          ▼          │
│                 │                      │  draft → chunks     │
│                 │                      │  → asset            │
│                 │                      │                     │
│                 │◄── dispatch result ──│  { asset_object_id }│
│                 │                      │                     │
│                 │                      │                     │
│                 │── GET /assets/{id} ─►│                     │
│                 │      /content        │                     │
│                 │◄── audio stream ─────│                     │
└─────────────────┘                      └─────────────────────┘
```

Demo-studio never talks to conduit. It only:
1. Triggers dispatch
2. Reads dispatch results (which contain asset references)
3. Fetches asset content via `/api/assets/{id}/content`

## Authentication Flow

Demo-studio authenticates with dapple-prompt-api using signed tokens:

```
┌─────────────────┐                      ┌─────────────────────┐
│  demo-studio    │                      │  dapple-prompt-api  │
│                 │                      │                     │
│  1. Sign token  │                      │                     │
│     with        │                      │                     │
│     private key │                      │                     │
│                 │                      │                     │
│                 │── POST /api/get-auth-token ──►             │
│                 │   { signed_token, address }   │            │
│                 │                      │  2. Verify:         │
│                 │                      │     - signature     │
│                 │                      │       matches addr  │
│                 │                      │     - well-formed   │
│                 │                      │     - not expired   │
│                 │◄── JWT ──────────────│                     │
│                 │                      │                     │
│  3. Use JWT for │                      │                     │
│     all requests│                      │                     │
│                 │── GET /api/assets/{id}/content ──►         │
│                 │   Authorization: Bearer <JWT>  │           │
│                 │◄── audio stream ─────│                     │
└─────────────────┘                      └─────────────────────┘
```

**Key points:**
- User signs token with private key, provides address
- API verifies signature matches address
- API issues JWT on successful verification
- JWT included in all subsequent requests (dispatch, asset content, etc.)
- Asset access checked via journey/dispatch ownership

## Notes

### Out of Scope

- **Cost tracking**: TTS compute costs tracked separately.
