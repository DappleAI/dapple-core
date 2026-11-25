# dapple-prompt-api memory leak under request load

**Status:** RESOLVED — deployed image was stale; rebuild with `--no-cache` and redeploy fixed it
**Created:** 2026-05-08 (overnight debug session)
**Resolved:** 2026-05-08 morning — Brian's `docker build --no-cache` + push + rollout restart confirmed the fix

**Branch:** `mac_orbstack` (PR #448), but the bug was image staleness during deploy, not OrbStack-related

## Resolution summary

After a clean `docker build --no-cache` + push + rollout restart:

- Idle memory: **9 Mi** (was 1100+ Mi before — over 100x reduction)
- After 5 requests: **8 Mi** — no growth
- All requests returned 200 with proper JSON response
- CPU stayed at 0 m

This matches the PR #445 commit message's measurements ("~50 MiB resident at idle, ~6 MiB growth across a 10-request burst") within noise. The Rust code was correct all along; the previously-deployed binary was a `docker build` cache hit on a pre-PR-#445 layer.

The remaining open item is figuring out **why** the cache didn't invalidate after PR #445 landed (the cargo-chef pattern should pick up Cargo.lock + src changes). See "Real follow-up" below.

## Actual root cause and fix

After investigation it wasn't a docker layer cache issue at all. `assets/nu/k8s/build_image.nu` computes a `content_hash` from the source tree and uses it as a registry tag. If the registry has a `<service>:<content_hash>` image, the script **skips the build** and just retags+pushes the existing image as `:dapple-latest`. PR #445's source changes WOULD have produced a new content_hash and triggered a rebuild — but the deploy script was simply never re-run after the main-merge brought in those changes. So the registry kept serving the pre-#445 binary tagged as `:dapple-latest`.

This morning's manual `docker build` + `docker push` + `kubectl rollout restart` worked because it bypassed the content-hash logic entirely.

### Latent hash bug (now fixed in `d73a7245`)

While diagnosing, found that `content_hash` was hashing only `crates/<deployment>/` + `Cargo.lock`. Cargo path dependencies don't bump versions, so a change to a workspace dependency crate (e.g. `dapple-data`) leaves both paths unchanged — and the hash silently stayed the same. That'd produce the same "stale binary" symptom for any future workspace-dep change.

Fixed by widening the hash inputs to mirror what the Dockerfile actually consumes:

```
crates/        (all workspace source, transitive deps included)
Cargo.toml     (workspace manifest, profiles)
Cargo.lock     (exact dep versions)
Dockerfile     (build instructions)
.dockerignore  (what reaches the build daemon)
.sqlx/         (sqlx offline metadata read at compile time)
```

Trade-off: a change to any workspace crate now invalidates the cache for every service. Slower than per-service hashing, but the alternative (silent stale binary) is much worse. Same change tightened the `is_dirty` check to mirror these inputs.

### Practical guidance going forward

- **Always re-run `nu assets/nu/k8s/deploy_dapple_core.nu` after pulling source changes.** The script's content-hash logic now correctly invalidates on any workspace input.
- **`docker build --no-cache` is no longer needed as a workaround.** With the wider hash, the script either rebuilds (correct) or skips (cache hit IS valid).
- **If a deploy completes suspiciously fast and "skips" services, double-check git status.** A clean tree on a known commit means the registry image was built from that exact source — trustworthy.

---

## Original diagnosis (kept for reference)


## TL;DR

`dapple-prompt-api` leaks roughly 250 MB of resident memory per HTTP request and OOMs at the 2 GB limit after ~4 requests. The UI's auto-retry loop (`useRecentProjects`, `useHats` polling) keeps the pod in a permanent CrashLoopBackOff. **Scaling `dapple-ui` to 0 replicas keeps the api stable**, which is the current workaround.

This is unrelated to the OrbStack migration — it would happen on Multipass too, since the bug is in the deployed binary code.

## Symptom (what the user saw)

Loading `https://dapple-ui.kube.dappleai.dev/project/...` produces 500s on:
- `/api/journeys/recent` (called from `useRecentProjects.ts:5`)
- `/api/hats` (called from `useHats.ts:5`)

Each request takes ~20s before failing — the duration matches the api's ECONNREFUSED window during pod restart.

The dapple-ui logs show:
```
[InternalApiClient] Error exchanging token for JWT: TypeError: fetch failed
  cause: Error: connect ECONNREFUSED 10.152.183.225:8001
```

That's the UI calling `${api}/api/get-service-token` and finding the api unreachable mid-restart.

## Diagnosis

### Pod state during the failure mode

```
$ kubectl get pods -n dapple-sandbox -l app=dapple-prompt-api
NAME                              READY  STATUS                RESTARTS
dapple-prompt-api-...             0/1    CrashLoopBackOff      4 (90s ago)
# or, mid-cycle:                  1/1    Running               4
```

`Last State: Terminated, Reason: OOMKilled, Exit Code: 137` confirms the kernel kills the container at the 2 GiB limit (`assets/k8s/registry/dapple-prompt-api.yaml`).

### Memory growth pattern (validated empirically)

With `dapple-ui` scaled to 0 and the api freshly restarted:

| Time after start | CPU      | Memory | Notes |
|------------------|----------|--------|-------|
| 0–16 s           | 720 m    | 1240 Mi | Startup spike (OpenAPI generation, route setup) |
| 16 s             | 0 m      | 1149 Mi | Settled to idle |
| After 1 curl health_check + 1 curl get-service-token | 29 m | 1669 Mi | **+520 MB from 2 trivial requests** |
| Next 1–2 requests | spikes high | 2047 Mi | OOM kill, restart cycle |

So the per-request growth is roughly 250 MB, even for an empty-body POST to `/api/get-service-token`.

### What we ruled out

- **OrbStack VM resource constraints.** VM has 32 GB RAM available; the limit is enforced by k8s, not by OrbStack.
- **Database state.** prompt-db is healthy, all 4 schemas present (public/web/rev/auth), 30 sqlx migrations applied successfully.
- **Conduit / flow dependency outage.** Both reachable; conduit logs show normal startup.
- **Service IP / DNS.** Service endpoint resolves correctly (`10.152.183.225:8001` → pod IP). The pod IS listening on 8001 (verified via `/proc/net/tcp` — state 0A LISTEN).
- **The Arc<OpenApi> perf fix being missing from source.** Verified: all 7 commits from PR #445 (Arc wrap, MALLOC_ARENA_MAX=2, resource rightsize) are present in HEAD with intact content. Only post-#445 code change in this crate is `abb9ffb6` updating one doc-comment in `schema/guides.rs` (no behavioral effect).

  HOWEVER: that doesn't prove the **deployed binary** has the fix. The image tag is `dapple-latest` (moving), and a `docker build` cache hit could mean the binary was built before the Arc-wrap landed. Worth trying `docker build --no-cache` + redeploy as the first investigation step — if the leak disappears, the bug was image staleness, not Rust code.

### What's still suspect

The Arc fix wraps the OpenAPI spec but the leak persists. Suspect:
1. **A different per-request clone** somewhere — perhaps a JWT signing key / config / DataLogic instance is being deep-cloned per request. The 250 MB-per-request growth needs a specific large allocation source — likely a single-threaded large structure being moved into each request task and not freed.
2. **An axum middleware** holding request-level state in an Arc that doesn't drop. CORS layers, tracing layers, the AppState — any of these in the wrong order can pin allocations.
3. **A tokio task spawn that leaks its scope.** Especially the auth-token issuance path; `get-service-token` likely signs a JWT which involves opening + reading a key, which if not cached could re-load per request.
4. **MALLOC_ARENA_MAX=2 is set** but glibc's heap fragmentation under heavy concurrent allocation can still grow RSS. Worth comparing against `jemalloc` or `mimalloc`.

### How sockets behave during the hang

While the binary is hot-spinning post-leak, `/proc/net/tcp` shows:
- Port 8001 LISTEN ✓
- Multiple connections in `CLOSE_WAIT` (state 08) — server-side hasn't closed them
- Some in `FIN_WAIT2` (state 05) — peer closed, server hasn't acked

CLOSE_WAITs accumulating means the request handler is stuck before it gets to drop the connection. Combined with 990m CPU usage, the binary is in a busy loop in handler code, not blocked on I/O.

## Current cluster state

To leave things stable for the morning:

- `dapple-ui` is scaled to 0 replicas (`kubectl scale deployment -n dapple-sandbox dapple-ui --replicas=0`). Re-enable with `--replicas=1` once the leak is fixed.
- `dapple-prompt-api` is alive but in restart-loop because something — the conduit's TaskContextClient health check, or some other in-cluster caller — is still hitting it occasionally. Even those occasional hits leak the 250 MB.
- All other pods are Running normally.

## Recommended investigation order

0. **Force-rebuild the deployed image first.** Cheapest test for "is the bug actually fixed in source but the image is stale":
   ```
   nu -c '^docker build --no-cache -t registry.kube.dappleai.dev/dapple-prompt-api:dapple-latest -f Dockerfile --build-arg BIN=dapple-prompt-api .'
   ^docker push registry.kube.dappleai.dev/dapple-prompt-api:dapple-latest
   kubectl rollout restart deployment/dapple-prompt-api -n dapple-sandbox
   ```
   Then re-run the reproduction below. If memory stays flat under load, the source had the fix all along and `docker build` was caching a pre-Arc-wrap binary. If memory still grows, proceed to step 1.

1. **Reproduce with a minimal repro** — `kubectl port-forward svc/dapple-prompt-api 8001:8001` from a quiet cluster and `curl` the api 5–10 times while watching `kubectl top pod` and `kubectl exec ... cat /proc/<pid>/status | grep VmRSS`. Confirms the per-request leak exists in isolation.

2. **Profile with heaptrack or DHAT.** Build a debug image with `heaptrack` available, run for 10 requests, inspect the per-request allocation report. The 250 MB allocation source will dominate the report.

3. **Check the auth/JWT path specifically.** `/api/get-service-token` is what the UI calls and where the leak appeared. Inspect:
   - `crates/dapple-prompt-api/src/handler/auth.rs` (or wherever get-service-token is implemented)
   - JWT signing — is the private key being re-loaded per request?
   - Is the AUTH_TOKEN_ALLOWED_SIGNERS secret being parsed per request?

4. **Try jemalloc.** Add `tikv-jemallocator` as a global allocator to see if the leak is glibc heap fragmentation rather than a true Rust-level leak. If RSS stops growing under heavy traffic, glibc fragmentation is confirmed culprit.

5. **Audit Layer/Extension order in `lib.rs:lines 380–410`.** A common cause of per-request memory growth is a `.layer()` middleware before `Extension(Arc::new(state))` that captures and clones state.

## Reproduction recipe (if needed)

Paste-ready for interactive nushell:

```
# 1. Confirm api is healthy with no traffic
kubectl scale deployment -n dapple-sandbox dapple-ui --replicas=0
kubectl delete pod -n dapple-sandbox -l app=dapple-prompt-api
sleep 30sec
kubectl top pod -n dapple-sandbox -l app=dapple-prompt-api  # should be ~1100Mi idle

# 2. Send a few requests; watch memory grow
for i in 1..5 {
    ^curl -sk -m 5 https://dapple-prompt-api.kube.dappleai.dev/api/health_check
}

# 3. Memory should now be at or near 2Gi limit
kubectl top pod -n dapple-sandbox -l app=dapple-prompt-api
```

## Quick mitigation if needed before fixing

Bump the memory limit to something obscene (e.g., 16 GB) — buys time per pod restart, but doesn't fix the leak. Update `assets/k8s/registry/dapple-prompt-api.yaml`:

```yaml
limits: { cpu: "1000m", memory: "16Gi" }
```

…and note that this masks the leak rather than solving it.
