````markdown
# redis-object-caching.md --- Redis Caching + Event Streaming Architecture for Dapple Data Layer

This document defines how Principal Relational Objects and their Owned
Dependents should be cached in Redis using RedisJSON, and how Redis
Streams will be used as an event queue for downstream services.\
It is written for GitHub Copilot, ChatGPT, and future engineers working
inside the Dapple ecosystem.

The reddis system is dapple-redis.kube.dappleai.dev and should be passed through the DAPPLE_REDIS_HOST env variable. Dotenv is used locally so will be set that way locally and via ENV when deployed.

------------------------------------------------------------------------

# 1. Goals

1.  Provide a read-optimized cache of Principal Objects for:

    -   The web-backend (read-only consumer)
    -   Internal services that do not need direct SQL access

2.  Ensure the cache is always consistent with Postgres using a
    write-through strategy.

3.  Introduce a durable event stream for downstream services:

    -   Search indexers\
    -   Notification processors\
    -   Analytics consumers\
    -   Monitoring and orchestration services

4.  Support deep REST paths such as:

    -   `/dispatch/{id}/attempts`
    -   `/dispatch/attempts`
    -   Other dependent collections under each Principal Object

5.  Enable multiple parallel consumers to process change events.

------------------------------------------------------------------------

# 2. Core Concepts

Dapple models include:

-   **Principal Objects** --- standalone entities with their own
    lifecycle.
-   **Owned Dependents** --- child rows belonging to a Principal (e.g.,
    dispatch attempts).
-   **Relational Objects** --- SQLx representations.
-   **DTOs** --- consumer-facing objects for external services and
    caching.

Redis is used as a **read model**, not a source of truth.

------------------------------------------------------------------------

# 3. Redis Key Structure

## 3.1 Principal Object Cache

Key pattern:

    dto:{type}:{object_id}

Examples:

    dto:playbook:{uuid}
    dto:dispatch:{id}
    dto:user:{id}

Value stored at `$` is the DTO for that object.

------------------------------------------------------------------------

## 3.2 Owned Dependent Cache (per principal)

Two patterns depending on size and update frequency.

### Option A --- Per-dependent document

    dto:{type}_attempts:{principal_id}

Example:

    dto:dispatch_attempts:123

JSON shape:

    {
      "dispatch_id": 123,
      "attempts": [ ...array of attempt DTOs... ]
    }

### Option B --- Full principal document with dependents

    po_full:{type}:{object_id}

Shape:

    {
      "dispatch": { ...principal dto... },
      "attempts": [ ... ]
    }

Option A is recommended for large or unbounded dependents.

------------------------------------------------------------------------

# 4. Write-Through Cache Population

All data changes originate in Postgres.

## 4.1 When a Principal Object is created, updated, or deleted:

1.  Perform SQLx write in a DB transaction.\
2.  Convert the result rows to a DTO.\
3.  Write-through to RedisJSON:\
    `JSON.SET dto:{type}:{id} $ '{dto-json}'`\
4.  Append event to a Redis Stream:\
    `XADD stream:principal_changes * type {type} object_id {id} op {insert|update|delete}`

------------------------------------------------------------------------

## 4.2 When an Owned Dependent is written (e.g.\u00a0dispatch attempt):

1.  SQL insert/update commit completes.\
2.  Generate Attempt DTO.\
3.  Update RedisJSON for dependent:

**Option A:**

    JSON.ARRAPPEND dto:dispatch_attempts:{dispatch_id} $.attempts '{dto}'

**Option B:**

    JSON.ARRAPPEND po_full:dispatch:{dispatch_id} $.attempts '{dto}'

4.  Append event to dependent stream:

```{=html}
<!-- -->
```
    XADD stream:dispatch_attempts * dispatch_id {id} attempt_id {id2} op {insert|update}

------------------------------------------------------------------------

# 5. Read Behavior (API and Web)

## 5.1 API Strategy (dapple-prompt-api)

All read handlers follow the same strategy:

1.  Try RedisJSON for the resource.\
2.  If missing:
    -   Query Postgres\
    -   Convert to DTO\
    -   Populate Redis cache\
    -   Return the DTO

This applies to:

-   `/playbooks/{id}`
-   `/dispatch/{id}`
-   `/dispatch/{id}/attempts` (reads from `dto:dispatch_attempts:{id}`)

## 5.2 Web Backend Access (Read-Only)

Redis ACL configuration:

-   Allowed: `JSON.GET`, `GET`, `EXISTS`\
-   Denied: `JSON.SET`, `DEL`, `XADD`, `XREAD`, etc.\
-   Allowed key patterns: `~dto:*`

Web backend may read Redis directly or read through the API.

------------------------------------------------------------------------

# 6. Redis Streams for Event Distribution

## 6.1 Stream Keys

Principal change events:

    stream:principal_changes

Owned dependent events:

    stream:dispatch_attempts

## 6.2 Event Encoding

Principal:

    {
      "type": "dispatch",
      "object_id": "abc-123",
      "op": "update",
      "version": "1.0.0"
    }

Attempt:

    {
      "dispatch_id": "123",
      "attempt_id": "456",
      "op": "insert",
      "status": "pending"
    }

## 6.3 Consumer Groups

Each internal service has its own group:

-   `analytics`
-   `search-indexer`
-   `notification-engine`
-   `monitoring`
-   future ML or journey orchestration services

Consumers use:

    XREADGROUP GROUP {group} {consumer} STREAMS {stream} >

This ensures each group processes all events independently.

------------------------------------------------------------------------

# 7. Optional: Keyspace Notifications

Keyspace notifications are supported for cache invalidation, not for
durable event processing.

Enable:

    notify-keyspace-events "KgxE"

Subscribe to patterns such as:

    __keyspace@0__:dto:dispatch:*

Used for:

-   In-process cache invalidation\
-   UI or task refresh triggers

Not suitable for multi-service workflows.

------------------------------------------------------------------------

# 8. REST Endpoint → Redis Key Mapping

  --------------------------------------------------------------------------------------
  REST Endpoint               Redis Key                     Notes
  --------------------------- ----------------------------- ----------------------------
  `/dispatch/{id}`            `dto:dispatch:{id}`            Principal DTO

  `/dispatch/{id}/attempts`   `dto:dispatch_attempts:{id}`   Owned dependents per
                              or `po_full:*`                principal

  `/dispatch/attempts`        None                          Must be queried from
                                                            Postgres directly

  `/playbook/{id}`            `dto:playbook:{uuid}`          Principal DTO

  Other principals            `dto:{type}:{id}`              Standard principal cache
                                                            pattern
  --------------------------------------------------------------------------------------

------------------------------------------------------------------------

# 9. Rust Implementation Plan

## 9.1 Cache Abstraction

    pub trait PrincipalCache {
        type DataTransferObject;

        async fn get(&self, ty: &str, id: Uuid) -> Result<Option<Self::Dto>, CacheError>;
        async fn set(&self, ty: &str, dto: &Self::DataTransferObject) -> Result<(), CacheError>;
        async fn delete(&self, ty: &str, id: Uuid) -> Result<(), CacheError>;
    }

## 9.2 Integrate Cache into Write Paths

After every successful SQLx write:

1.  Find the point where the DTO has been folded/merged/created before returning the DTO.
2.  `cache.set()` with the DTO at the appropriate address, ReferenceObjects should hold an appropriate address to find the Referred Data Transfer Object.
3.  Emit stream event via publisher.

## 9.4 Event Publisher

Provides:

-   `principal_changed(event)`
-   `dispatch_attempt_changed(event)` #NOTE: this is for a later phase and will not be implemented now.

------------------------------------------------------------------------

# 10. Operational Requirements

1.  Redis Stack (RedisJSON + Streams) must be deployed.\
2.  Redis ACL configured:
    -   `api-service` = full access\
    -   `web-backend` = read-only\
3.  Redis writes occur after DB commit.\
4.  Stream consumers fetch full DTOs from RedisJSON.\
5.  Cache misses fall back to Postgres internally or via REST calls externally.

------------------------------------------------------------------------

# 11. Future Extensions

-   TTLs for rarely accessed objects\
-   Batch writes for high-frequency dependents\
-   Versioned Redis keys\
-   Snapshot streams for projection rebuilds\
-   Derived caches (e.g., search indices)

------------------------------------------------------------------------

# 12. Summary

This plan defines:

-   A write-through RedisJSON cache for Principals\
-   Structured storage of Owned Dependents\
-   Multi-consumer Redis Streams for event distribution\
-   Strong ACL-based separation for web-backend\
-   Full consistency with Postgres as the source of truth

RedisJSON is the read model.\
Redis Streams are the change bus.\
Postgres remains authoritative.

````
