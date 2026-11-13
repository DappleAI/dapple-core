# Kubernetes Troubleshooting Cookbook

> **Audience:** All Developers

A practical guide to diagnosing and fixing common Kubernetes issues in the Dapple development environment. Each section follows the pattern: **Symptom → Diagnosis → Solution**.

## Quick Diagnostic Commands

Before diving into specific issues, here are the essential commands for investigating any problem:

```nu
# Overall cluster health
dapple status                  # All pods at a glance
dapple events                  # Recent warnings/errors

# Service-specific investigation
dapple status api              # Pod status for api
dapple describe api            # Full details including events
dapple logs api                # Current container logs
dapple logs api -p             # Previous container logs (if crashed)

# Kubernetes-level debugging
kubectl get events -n dapple-sandbox --sort-by='.lastTimestamp'
kubectl get pods -n dapple-sandbox -o wide
kubectl top pods -n dapple-sandbox  # Resource usage
```

---

## Pod Status Issues

### Pod Stuck in `Pending`

**Symptom:**
```
NAME                                 STATUS    AGE
dapple-prompt-api-abc123-xyz        Pending   5m
```

**Diagnosis:**
```nu
dapple describe api
# Look for Events section at the bottom
```

**Common Causes & Solutions:**

| Event Message | Cause | Solution |
|---------------|-------|----------|
| `Insufficient cpu` or `Insufficient memory` | Node resources exhausted | Scale down other pods or restart the cluster |
| `no nodes available to schedule` | All nodes tainted/unavailable | Check node status: `kubectl get nodes` |
| `persistentvolumeclaim not found` | PVC missing | Check PVC: `kubectl get pvc -n dapple-sandbox` |
| `0/1 nodes are available: 1 node(s) had taint` | Node has taints | Check taints: `kubectl describe node` |

### Pod in `ImagePullBackOff`

**Symptom:**
```
NAME                                 STATUS             AGE
dapple-prompt-api-abc123-xyz        ImagePullBackOff   2m
```

**Diagnosis:**
```nu
dapple describe api
# Look for:
# - "Failed to pull image..."
# - "unauthorized" or "not found" errors
```

**Common Causes & Solutions:**

| Error | Cause | Solution |
|-------|-------|----------|
| `unauthorized` | Registry auth issue | Re-run `dapple deploy api` to refresh secrets |
| `certificate signed by unknown authority` | Registry cert not trusted | **macOS / Docker Desktop:** Restart Docker Desktop, then `dapple build api --force`. **Linux (native dockerd):** Check `/etc/docker/certs.d/{registry}/ca.crt` exists; run `k8s setup_docker_registry_cert` if missing |
| `manifest unknown` | Image doesn't exist | Build the image: `dapple build api` |
| `connection refused` | Registry unreachable | Check VPN, verify registry: `curl -k https://registry.kube.dappleai.dev/v2/` |

**Quick Fix for Certificate Issues:**

*macOS (Docker Desktop):*
```bash
# Restart Docker Desktop (picks up new certs)
# Then:
dapple build api --force
dapple patch api
```

*Linux (native dockerd):*
```bash
# Ensure cert is installed (no restart needed)
nu -c 'use k8s; k8s setup_docker_registry_cert $env.KUBE_REGISTRY_HOST $env.KUBE_ROOT_CA_CERT_NAME'
# Verify:
ls /etc/docker/certs.d/$KUBE_REGISTRY_HOST/ca.crt
# Then retry:
dapple build api --force
dapple patch api
```

### Pod in `CrashLoopBackOff`

**Symptom:**
```
NAME                                 STATUS             RESTARTS   AGE
dapple-prompt-api-abc123-xyz        CrashLoopBackOff   5          3m
```

**Diagnosis:**
```nu
# Check why it's crashing
dapple logs api --previous     # Logs from crashed container

# Check for OOM kills
dapple describe api | grep -A5 "Last State"
# Look for: Reason: OOMKilled
```

**Common Causes & Solutions:**

| Log Pattern | Cause | Solution |
|-------------|-------|----------|
| `Database connection refused` | Postgres not ready | Wait or `dapple restart postgres` |
| `ANTHROPIC_API_KEY not set` | Missing secret | Re-run setup: `nu assets/nu/init/setup.nu --environment sandbox` |
| `OOMKilled` in describe | Memory limit exceeded | Increase memory in manifest |
| `panic!` or Rust stacktrace | Application bug | Fix code, redeploy |
| No logs visible | Crash before startup | Check `dapple events` for init container issues |

### Pod in `Terminating` Forever

**Symptom:**
```
NAME                                 STATUS        AGE
dapple-prompt-api-abc123-xyz        Terminating   15m
```

**Diagnosis:**
```nu
dapple describe api
# Look for finalizers or deletion timestamp
```

**Solution:**
```nu
# Normal cleanup
dapple cleanup

# Force delete (if stuck for >5 min)
dapple cleanup --force
```

### Pod `Running` but Not Working

**Symptom:**
Pod shows as Running but service doesn't respond.

**Diagnosis:**
```nu
# Check readiness
dapple status api
# Look for READY column: 0/1 means readiness probe failing

# Check liveness/readiness probes
dapple describe api | grep -A10 "Liveness"

# Check logs for errors
dapple logs api
```

**Common Causes & Solutions:**

| Symptom | Cause | Solution |
|---------|-------|----------|
| READY 0/1 | Readiness probe failing | Check probe endpoint, logs |
| Logs show "binding to 0.0.0.0:8001" | Service running on wrong address | Check env vars |
| Connection reset | Service crashing on requests | Check logs during request |

---

## Networking Issues

### Cannot Connect to Service from Outside

**Symptom:**
```bash
curl https://dapple-prompt-api.kube.dappleai.dev/health
# Connection refused or timeout
```

**Diagnosis:**
```nu
# Check pod is running
dapple status api

# Check service exists
dapple services api

# Check ingress
kubectl get ingress -n dapple-sandbox
```

**Solutions:**

| Issue | Solution |
|-------|----------|
| Pod not running | `dapple deploy api` |
| Service missing | `dapple deploy api` (applies manifests) |
| Ingress issue | Check ingress controller: `kubectl get pods -n ingress` |
| DNS not resolving | Check VPN, try: `nslookup dapple-prompt-api.kube.dappleai.dev` |

### Service-to-Service Communication Failing

**Symptom:**
API can't reach Postgres or Conduit.

**Diagnosis:**
```nu
# Shell into the pod
dapple shell api

# Test DNS resolution
nslookup prompt-db
nslookup dapple-prompt-conduit

# Test connectivity
wget -O- http://dapple-prompt-conduit:8081/health
```

**Solutions:**

| Issue | Solution |
|-------|----------|
| DNS not resolving | Check CoreDNS: `kubectl get pods -n kube-system | grep dns` |
| Connection refused | Target service not running |
| Timeout | Network policy blocking traffic |

### Port Forward Not Working

**Symptom:**
```nu
dapple port-forward postgres
# Error or connection refused on localhost
```

**Diagnosis:**
```bash
# Check if port is already in use
lsof -i :5432

# Check pod is running
dapple status postgres
```

**Solutions:**

| Issue | Solution |
|-------|----------|
| Port in use | Kill existing process or use different port: `dapple port-forward postgres 5433` |
| Pod not running | `dapple restart postgres` |

---

## Build & Deploy Issues

### Docker Build Fails

**Symptom:**
```
dapple build api
# Error during build
```

**Diagnosis:**
```bash
# Run docker build manually for more detail
cd /path/to/dapple-prompt
docker build --build-arg BIN=dapple-prompt-api --build-arg PROFILE=dev .
```

**Common Causes & Solutions:**

| Error | Cause | Solution |
|-------|-------|----------|
| `cargo build failed` | Compile error | Fix code, check `cargo build` locally |
| `SQLX_OFFLINE` error | Query cache stale | Run `dapple sqlx prepare` (raw: `cargo sqlx prepare --workspace -- --all-targets`) |
| `network is unreachable` | Docker network issue | Restart Docker Desktop |
| `no space left on device` | Docker disk full | `docker system prune -a` |

### Push to Registry Fails

**Symptom:**
```
dapple build api
# Build succeeds but push fails
```

**Diagnosis:**
```bash
# Test registry connectivity
curl -k https://registry.kube.dappleai.dev/v2/

# Check Docker certs (Docker Desktop)
ls ~/.docker/certs.d/registry.kube.dappleai.dev/

# Check Docker certs (native dockerd on Linux)
ls /etc/docker/certs.d/registry.kube.dappleai.dev/
```

**Solutions:**

| Error | Solution |
|-------|----------|
| `certificate signed by unknown authority` | **Docker Desktop:** Restart Docker Desktop. **Native dockerd (Linux):** Run `k8s setup_docker_registry_cert` — no restart needed |
| `unauthorized` | Check credentials in Docker config |
| `connection refused` | Check VPN connection |

### Rollout Stuck

**Symptom:**
```
dapple patch api
# "Waiting for rollout..." hangs
```

**Diagnosis:**
```nu
# Check rollout status
kubectl rollout status deployment/dapple-prompt-api -n dapple-sandbox

# Check new pods
dapple status api
# Look for new pods (different age than old ones)

# Check events
dapple events
```

**Solutions:**

| Issue | Solution |
|-------|----------|
| New pods in ImagePullBackOff | See ImagePullBackOff section |
| New pods crashing | See CrashLoopBackOff section |
| Old pods not terminating | `dapple cleanup --force` |
| Rollback | `kubectl rollout undo deployment/dapple-prompt-api -n dapple-sandbox` |

---

## Database Issues

### Cannot Connect to Postgres

**Symptom:**
```
dapple port-forward postgres
psql -h localhost -p 5432 -U dapple dapple
# Connection refused or auth failed
```

**Diagnosis:**
```nu
# Check postgres is running
dapple status postgres
dapple logs postgres
```

**Solutions:**

| Issue | Solution |
|-------|----------|
| Postgres pod not running | `dapple restart postgres` |
| Auth failed | Check credentials in `.env.dapple` |
| Database doesn't exist | Run migrations: see deploy script |

### Migrations Failed

**Symptom:**
Service won't start, logs show database schema errors.

**Diagnosis:**
```nu
dapple logs api
# Look for "relation does not exist" or schema errors
```

**Solution:**
```bash
# Connect to database
dapple port-forward postgres &
psql -h localhost -p 5432 -U dapple dapple

# Check current state
\dt web.*  -- List tables

# Run migrations manually if needed
cd crates/dapple-data
sqlx migrate run
```

---

## Resource Issues

### Out of Memory (OOMKilled)

**Symptom:**
Pod keeps restarting, describe shows `OOMKilled`.

**Diagnosis:**
```nu
dapple describe api | grep -A5 "Last State"
# Look for: Reason: OOMKilled

# Check memory usage
kubectl top pods -n dapple-sandbox
```

**Solutions:**
1. Check for memory leaks in application
2. Increase memory limits in deployment manifest
3. Scale down other services if cluster constrained

### Disk Full

**Symptom:**
Builds fail, pods can't write logs.

**Diagnosis:**
```bash
# Check Docker disk usage
docker system df

# Check node disk
kubectl describe node | grep -A5 "Allocated resources"
```

**Solutions:**
```bash
# Clean Docker
docker system prune -a

# Clean old images
docker image prune -a
```

---

## Diagnostic Reference

### Essential Commands Cheatsheet

```nu
# Status overview
dapple status                    # All pods
dapple status api -w             # Service with node info
dapple events                    # Cluster events

# Logs
dapple logs api                  # Live logs
dapple logs api -p               # Previous container
dapple logs api --since "1h"     # Last hour

# Investigation
dapple describe api              # Full pod details
dapple shell api                 # Shell into container

# Recovery
dapple restart api               # Rolling restart
dapple cleanup                   # Remove bad pods
dapple cleanup --force           # Force remove stuck pods

# Rebuild
dapple build api --force         # Force rebuild image
dapple deploy api --force        # Full redeploy
```

### When to Escalate

If you've tried the above and the issue persists:

1. **Collect diagnostics:**
   ```nu
   dapple describe api > /tmp/api-describe.txt
   dapple logs api > /tmp/api-logs.txt
   dapple events > /tmp/events.txt
   kubectl get pods -n dapple-sandbox -o yaml > /tmp/pods.yaml
   ```

2. **Check if cluster-wide:**
   ```nu
   kubectl get nodes
   kubectl describe nodes | grep -A20 "Conditions"
   ```

3. **Share in #platform-engineering** with:
   - What you were trying to do
   - When it started failing
   - Diagnostic output from above
