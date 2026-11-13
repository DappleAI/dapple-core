# guide.toml Reference

> **Audience:** Guide Authors · Prompt Engineers

This chapter is the complete field reference for `guide.toml`, the metadata file at the root of every guide directory. It defines the guide's identity, versioning, promotional card, and deprecation state.

---

## Quick Facts

- **`title` is the only required field.** Everything else is optional.
- **No `locale` field.** The guide structure is locale-independent. All localizable text lives in `.ftl` files under `locales/`.
- **Card subtitle and category are NOT in guide.toml.** They are Fluent attributes on the brief-pack message in your `.ftl` files.
- **Unknown fields are silently ignored.** Legacy fields like `locale`, `subtitle`, or `category` will not cause parse errors -- they are simply discarded. This ensures backwards compatibility when fields are removed or moved.

---

## Full Example

```toml
title = "Essay Helper"
description = "Guides users through the essay writing process"
author = "Dapple Team"
classification = "workflow"
version = "1.2.0"

[card]
image = "assets/images/card.jpg"
color = "#2D6A4F"
accent = "#95D5B2"

[deprecation]
level = "deprecated"
reason = "Replaced by essay-helper-v2"
```

---

## Minimal Example

```toml
title = "My First Guide"
```

This is a valid `guide.toml`. All optional fields default to `None` / absent.

---

## Card Example

```toml
title = "Resume Builder"
description = "Build a professional resume step by step"

[card]
image = "assets/images/card.jpg"
color = "#FF6B35"
accent = "#004E89"
```

The card section controls the visual appearance of the guide's promotional card in the UI. The image path is relative to the guide directory root.

Note that the card's **subtitle** and **category** text are localizable and therefore live in a Fluent `.ftl` file, not here. For example, in `locales/en.ftl`:

```ftl
resume-builder-pack =
    Build a professional resume step by step.
    .card-subtitle = Ready to build your resume?
    .card-category = career
```

---

## Field Reference

### Top-Level Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `title` | String | **Yes** | The guide's display title. Used as the path-level title across all versions. |
| `description` | String | No | A brief description of the guide's purpose. |
| `author` | String | No | The guide author or team name. |
| `classification` | String | No | Free-form classification string (e.g., `"workflow"`, `"tutorial"`, `"reference"`). |
| `version` | String | No | Semantic version string. Must be valid semver when present (e.g., `"1.2.0"`, `"0.1.0-dev"`). |

**Version format:** When the `version` field is present, it is validated as a [semver](https://semver.org/) string. Pre-release identifiers are allowed (e.g., `"1.0.0-beta"`, `"0.1.0-dev"`). Build metadata is allowed (e.g., `"1.0.0+build.123"`). Invalid version strings produce a parse error.

**Version omission:** If `version` is omitted, the guide can still be ingested. Version tracking falls back to the database-level patch versioning (each PATCH increments the patch number automatically).

### [card] Section

The `[card]` section defines locale-independent visual properties for the guide's promotional card. All fields within the section are optional. The section itself is optional.

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `image` | String | No | Relative path to a card image file within the guide directory (e.g., `"assets/images/card.jpg"`). |
| `color` | String | No | Primary hex color for the card background (e.g., `"#2D6A4F"`). |
| `accent` | String | No | Accent hex color for card highlights (e.g., `"#95D5B2"`). |

An empty `[card]` section is valid:

```toml
title = "Empty Card Guide"

[card]
```

This creates a card with no image, color, or accent -- all fields default to `None`.

### [deprecation] Section

The `[deprecation]` section declares that the version being ingested is deprecated, retired, or quarantined. **Absence of this section means the guide is active.** There is no `"active"` deprecation level -- omit the section entirely for active guides.

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `level` | String | **Yes** (within section) | The deprecation level. Must be one of the valid levels listed below. |
| `reason` | String | No | Human-readable explanation of why the guide is deprecated. |

**Valid deprecation levels:**

| Level | Meaning |
|-------|---------|
| `deprecated` | Soft deprecation. The guide is still accessible but discouraged for new use. |
| `retired` | The guide has been removed from active use. |
| `quarantined` | The guide is flagged for review. Access may be restricted. |

**Invalid levels:**

- `"active"` is explicitly rejected. The parser returns an error: *"deprecation level 'active' is invalid; omit the [deprecation] section for active guides"*.
- Any string not in the valid levels list is rejected. The parser returns an error listing the valid options.

**Examples:**

```toml
# Deprecated guide with reason
[deprecation]
level = "deprecated"
reason = "Replaced by essay-helper-v2"
```

```toml
# Retired guide, no reason given
[deprecation]
level = "retired"
```

```toml
# Quarantined for compliance review
[deprecation]
level = "quarantined"
reason = "Under review for compliance"
```

---

## Guide Cards

Guide cards are promotional summary views displayed in the UI for guide discovery. They are created automatically during ingest from two sources:

1. **`[card]` section in guide.toml** -- Locale-independent visual properties (image, color, accent)
2. **Fluent attributes on the brief-pack message** -- Localizable text (subtitle, category)

### Card Data Sources

| Field | Source | Example |
|-------|--------|---------|
| Title | `guide.toml` `title` field | `"Resume Builder"` |
| Image | `[card].image` (uploaded as asset during ingest) | `"assets/images/card.jpg"` |
| Color | `[card].color` (hex) | `"#FF6B35"` |
| Accent | `[card].accent` (hex) | `"#004E89"` |
| Subtitle | Fluent `.card-subtitle` attribute | `"Ready to build your resume?"` |
| Category | Fluent `.card-category` attribute | `"career"` |

### Fluent Card Attributes

The subtitle and category text are localizable and defined as Fluent attributes on the `:::brief_pack` message, not in `guide.toml`:

```ftl
# locales/en.ftl
resume-builder-pack =
    Build a professional resume step by step.
    .card-subtitle = Ready to build your resume?
    .card-category = career
```

```ftl
# locales/pt-BR.ftl
resume-builder-pack =
    Construa um curriculo profissional passo a passo.
    .card-subtitle = Pronto para construir seu curriculo?
    .card-category = carreira
```

### Tag-Based Rendering

Cards link to guides that support tag-based PDF rendering. Each render profile has a `tag_filter` that controls which blocks appear in the output:

| Profile | Style | Description |
|---------|-------|-------------|
| `consumer` | Consumer branding | End-user facing; clean fonts and colors |
| `technical` | Technical branding | Developer-oriented; code-friendly layout |
| `internal` | Internal branding | Team-internal; all content, minimal styling |

Untagged blocks are always included. Valid tags: `agent`, `product`, `coder`, `auditor`, `planner`.

> For SDK and HTTP methods for querying cards, managing favorites, and rendering PDFs, see the [API Consumer Guide - Guide Cards, Favorites, and Rendering](../api-consumer-guide/guide-cards-and-favorites.md).

---

## Legacy Compatibility

### book.toml Fallback

If `guide.toml` is not found, the parser checks for a `book.toml` file (the mdBook convention). If present, only the `title` field is extracted. All other metadata fields (`description`, `author`, `classification`, `version`, `card`, `deprecation`) are unavailable in legacy mode. A deprecation warning is logged.

If neither `guide.toml` nor `book.toml` exists, the parser returns a `MissingGuideToml` error.

### Ignored Legacy Fields

The following fields may appear in older `guide.toml` files but are silently ignored:

| Legacy Field | Where It Moved |
|-------------|----------------|
| `locale` | Removed. Guide structure is locale-independent. |
| `[card] subtitle` | Fluent attribute `.card-subtitle` on the brief-pack message |
| `[card] category` | Fluent attribute `.card-category` on the brief-pack message |

Because the parser does not use `#[serde(deny_unknown_fields)]`, any unrecognized TOML keys are silently discarded. This ensures that older guide directories continue to parse without errors as the schema evolves.

---

## Validation Summary

| Rule | Error Produced |
|------|---------------|
| `title` field missing | TOML deserialization error (field is required) |
| `version` present but not valid semver | `InvalidGuideToml: invalid semver version "..."` |
| `deprecation.level = "active"` | `InvalidGuideToml: deprecation level "active" is invalid` |
| `deprecation.level` is unknown string | `InvalidGuideToml: unknown deprecation level "..."; valid levels are: deprecated, retired, quarantined` |
| No `guide.toml` or `book.toml` in directory | `MissingGuideToml` |
