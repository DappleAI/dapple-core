# Guide Cards, Favorites, and Rendering

> **Audience:** SDK Consumers · API Consumers · Frontend Developers

Guide cards provide a user-facing discovery surface for guides. Favorites let users bookmark guides they use frequently. PDF rendering produces filtered, branded documents from guide content.

> For how to author guide cards (the `[card]` section in `guide.toml` and Fluent card attributes), see the [Journey Planner Guide - guide.toml Reference](../journey-planner-guide/guide-toml-reference.md#guide-cards).

---

## Guide Cards

Guide cards are created automatically during [guide ingest](./ingest-api.md) from the `[card]` section in `guide.toml` and Fluent attributes on the brief-pack message.

### SDK: List All Cards

```rust
let cards = client.guides().cards(None).await?;

for card in &cards {
    println!("{}: {}", card.title, card.category);
}
```

### SDK: Filter by Category

```rust
let career_cards = client.guides()
    .cards_by_category("career", None)
    .await?;
```

### HTTP API

```bash
# All cards
curl "https://api.example.com/api/guide-cards/?query=all" \
  -H "Authorization: Bearer dpl_your_token"

# Cards by category
curl "https://api.example.com/api/guide-cards/?query=by_category&category=career" \
  -H "Authorization: Bearer dpl_your_token"
```

---

## Favorites

Users can mark guides as favorites for quick access.

### SDK Methods

```rust
// Add a guide to favorites
client.guides().favorite(guide_id).await?;

// Remove from favorites
client.guides().unfavorite(guide_id).await?;

// Check if a guide is a favorite
let is_fav = client.guides().is_favorite(guide_id).await?;

// List all favorites
let favorites = client.guides().favorites().await?;
```

### HTTP API

The favorite endpoints take the guide's path id in the URL and need no request
body. `{id}` is the guide path id.

```bash
# Add to favorites (no body; idempotent)
curl -X POST "https://api.example.com/api/guides/019426a1-.../favorite" \
  -H "Authorization: Bearer dpl_your_token"

# Remove from favorites
curl -X DELETE "https://api.example.com/api/guides/019426a1-.../favorite" \
  -H "Authorization: Bearer dpl_your_token"

# Check favorite status → { "status": "...", "is_favorited": true }
curl "https://api.example.com/api/guides/019426a1-.../favorite" \
  -H "Authorization: Bearer dpl_your_token"

# List all favorites
curl "https://api.example.com/api/guides/favorites" \
  -H "Authorization: Bearer dpl_your_token"
```

---

## PDF Rendering

Guides can be rendered as branded PDF documents with audience-based content filtering.

### SDK

```rust
let pdf_bytes = client.guides()
    .render_pdf(guide_id, "consumer")
    .await?;

std::fs::write("guide.pdf", &pdf_bytes)?;
```

### HTTP API

```bash
curl "https://api.example.com/api/guides/019426a1-.../render?profile=consumer" \
  -H "Authorization: Bearer dpl_your_token" \
  -o guide.pdf
```

### Render Profiles

Each render profile defines a tag filter and branding style:

| Profile | Style | Description |
|---------|-------|-------------|
| `consumer` | Consumer branding | End-user facing; clean fonts and colors |
| `technical` | Technical branding | Developer-oriented; code-friendly layout |
| `internal` | Internal branding | Team-internal; all content, minimal styling |

### Tag-Based Filtering

Render profiles include a `tag_filter` that controls which blocks appear in the output:

- Blocks with tags matching the profile's filter are **included**
- **Untagged blocks are always included** regardless of profile
- Valid tags: `agent`, `product`, `coder`, `auditor`, `planner`

For example, a `consumer` profile might filter to `["product"]` tags, excluding `agent` and `coder` content while keeping all untagged blocks.

---

## Next Steps

- [Guide Ingest API](./ingest-api.md) -- Ingesting guides into the platform
- [SDK Overview](./sdk-overview.md) -- Full SDK documentation
