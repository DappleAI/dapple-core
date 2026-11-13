# Issue 2: Slugify Function Migration

**Status:** Draft
**Created:** 2026-02-13
**Dependencies:** None (can be implemented independently)
**Blocks:** Issue 3 (Guide DDL uses the same slugify pattern)

---

## 1. Summary

Currently, slug generation is duplicated across 6+ SQL insert files using the inline expression:

```sql
lower(regexp_replace(args.name, '\\s+', '-', 'g'))
```

This expression lives in `insert_hats.sql`, `insert_tasks.sql`, `insert_steps.sql`, `insert_stages.sql`, `insert_playbooks.sql`, `insert_categories.sql`, and will be needed in `insert_guides.sql`.

This issue creates:
1. A **PostgreSQL function** `rev.slugify(text)` in a migration that centralizes the slug generation logic.
2. A **Rust utility function** `slugify(name: &str) -> String` in `dapple-data` for use in the ingestion pipeline and API validation.
3. Updates to all existing SQL insert files to use `rev.slugify()` instead of the inline expression.

The Rust function is needed because the ingestion pipeline (Phase 3) must compute slugs client-side for lookup-before-insert logic (checking if a guide already exists by slug).

---

## 2. Database Migration

### File

`/home/kenny/src/dapple/dapple-prompt/crates/dapple-data/migrations/20260214000001_create_slugify_function.sql`

### DDL

```sql
-- Create a reusable slugify function in the rev schema.
-- This replaces the inline lower(regexp_replace(...)) pattern used in insert queries.
--
-- The function:
--   1. Converts to lowercase
--   2. Replaces sequences of whitespace with a single hyphen
--   3. Strips leading/trailing hyphens
--   4. Removes characters that are not alphanumeric, hyphens, or underscores
--
-- Examples:
--   rev.slugify('Journey Builder Guide') = 'journey-builder-guide'
--   rev.slugify('  Multiple   Spaces  ') = 'multiple-spaces'
--   rev.slugify('Special! Ch@rs (removed)') = 'special-chrs-removed'

CREATE OR REPLACE FUNCTION rev.slugify(input TEXT) RETURNS TEXT AS $$
BEGIN
    RETURN trim(BOTH '-' FROM
        regexp_replace(
            regexp_replace(
                lower(input),
                '[^a-z0-9\s_-]', '', 'g'    -- Remove non-slug characters
            ),
            '\s+', '-', 'g'                  -- Replace whitespace runs with hyphen
        )
    );
END;
$$ LANGUAGE plpgsql IMMUTABLE;
```

### Why IMMUTABLE

The function is marked `IMMUTABLE` because:
- It depends only on its input argument (no table reads, no session state)
- The same input always produces the same output
- PostgreSQL can optimize calls to immutable functions (e.g., in index expressions)

---

## 3. Rust Utility Function

### File

`/home/kenny/src/dapple/dapple-prompt/crates/dapple-data/src/slugify.rs`

### Implementation

```rust
/// Generate a URL-friendly slug from a name string.
///
/// This function mirrors the behavior of `rev.slugify()` in PostgreSQL
/// so that Rust code and SQL produce identical slugs for the same input.
///
/// Transformations:
/// 1. Convert to lowercase
/// 2. Remove characters that are not alphanumeric, whitespace, hyphens, or underscores
/// 3. Replace sequences of whitespace with a single hyphen
/// 4. Trim leading/trailing hyphens
///
/// # Examples
///
/// ```
/// assert_eq!(slugify("Journey Builder Guide"), "journey-builder-guide");
/// assert_eq!(slugify("  Multiple   Spaces  "), "multiple-spaces");
/// assert_eq!(slugify("Special! Ch@rs (removed)"), "special-chrs-removed");
/// ```
pub fn slugify(input: &str) -> String {
    let lowered = input.to_lowercase();

    // Remove non-slug characters (keep alphanumeric, whitespace, hyphens, underscores)
    let cleaned: String = lowered
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace() || *c == '-' || *c == '_')
        .collect();

    // Replace whitespace runs with a single hyphen
    let mut result = String::with_capacity(cleaned.len());
    let mut prev_was_whitespace = false;

    for c in cleaned.chars() {
        if c.is_whitespace() {
            if !prev_was_whitespace {
                result.push('-');
            }
            prev_was_whitespace = true;
        } else {
            result.push(c);
            prev_was_whitespace = false;
        }
    }

    // Trim leading/trailing hyphens
    result.trim_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_slug() {
        assert_eq!(slugify("Journey Builder Guide"), "journey-builder-guide");
    }

    #[test]
    fn test_multiple_spaces() {
        assert_eq!(slugify("  Multiple   Spaces  "), "multiple-spaces");
    }

    #[test]
    fn test_special_characters() {
        assert_eq!(slugify("Special! Ch@rs (removed)"), "special-chrs-removed");
    }

    #[test]
    fn test_already_slug() {
        assert_eq!(slugify("already-a-slug"), "already-a-slug");
    }

    #[test]
    fn test_underscores_preserved() {
        assert_eq!(slugify("snake_case name"), "snake_case-name");
    }

    #[test]
    fn test_empty_string() {
        assert_eq!(slugify(""), "");
    }

    #[test]
    fn test_single_word() {
        assert_eq!(slugify("Word"), "word");
    }

    #[test]
    fn test_leading_trailing_hyphens() {
        assert_eq!(slugify("- Leading Hyphen -"), "leading-hyphen");
    }
}
```

### Module Registration

Add to `/home/kenny/src/dapple/dapple-prompt/crates/dapple-data/src/lib.rs`:

```rust
pub mod slugify;
pub use slugify::slugify;
```

Add the line after the existing `pub mod traits;` line.

---

## 4. SQL File Updates

Update all existing insert SQL files that use the inline slugify pattern. Each file changes one line.

### Files to Update

| File | Line to Change |
|------|---------------|
| `crates/dapple-data/assets/sql/hats/insert_hats.sql` | Line 37 |
| `crates/dapple-data/assets/sql/tasks/insert_tasks.sql` | Line 41 |
| `crates/dapple-data/assets/sql/steps/insert_steps.sql` | Line 37 |
| `crates/dapple-data/assets/sql/stages/insert_stages.sql` | Line 37 |
| `crates/dapple-data/assets/sql/playbooks/insert_playbooks.sql` | Line 37 |
| `crates/dapple-data/assets/sql/categories/insert_categories.sql` | Line 37 |

### Change Pattern

In each file, replace:

```sql
        lower(regexp_replace(args.name, '\\s+', '-', 'g'))
```

With:

```sql
        rev.slugify(args.name)
```

No other changes needed. The function signature matches the inline expression's behavior exactly.

### Additional Files That May Need Updates

Check for the pattern in patch/update SQL files as well:

```bash
grep -r "regexp_replace.*\\\\s.*-.*g" crates/dapple-data/assets/sql/
```

---

## 5. Verification Steps

### Database Verification

After running the migration:

```sql
-- Verify the function exists and works
SELECT rev.slugify('Journey Builder Guide');
-- Expected: 'journey-builder-guide'

SELECT rev.slugify('  Multiple   Spaces  ');
-- Expected: 'multiple-spaces'

SELECT rev.slugify('Special! Ch@rs (removed)');
-- Expected: 'special-chrs-removed'
```

### Rust Verification

```bash
cargo test -p dapple-data slugify
```

### SQL File Verification

After updating the SQL files, verify that the existing test suite passes:

```bash
cargo test --test user-lifecycle --package dapple-prompt-api
```

This confirms that `rev.slugify()` produces identical results to the old inline expression.

### Equivalence Verification

To verify Rust and SQL produce identical slugs, the test suite should include a trial that:

1. Creates an object via POST with a known name
2. Computes `dapple_data::slugify(&name)` in Rust
3. Asserts the returned `slug` field matches the Rust-computed slug

This is already implicitly tested by existing lifecycle tests (names are generated, slugs are returned and verified), but adding an explicit assertion for a known input strengthens confidence.

---

## 6. Risk Assessment

**Risk: Behavioral difference between old and new slugify.**

The old inline expression `lower(regexp_replace(args.name, '\\s+', '-', 'g'))` does NOT:
- Remove special characters
- Trim leading/trailing hyphens

The new `rev.slugify()` function DOES both of these additional cleanups.

**Mitigation:** Since all existing data was created with the old expression, no existing slugs contain special characters (the `name` fields are typically alphanumeric + spaces). The additional cleanup in the new function only affects future inserts with unusual characters.

**If strict backward compatibility is required**, simplify `rev.slugify()` to match the old behavior exactly:

```sql
CREATE OR REPLACE FUNCTION rev.slugify(input TEXT) RETURNS TEXT AS $$
BEGIN
    RETURN lower(regexp_replace(input, '\s+', '-', 'g'));
END;
$$ LANGUAGE plpgsql IMMUTABLE;
```

This is the **recommended approach for initial implementation** -- keep the function simple and matching the current behavior. Enhanced slug normalization can be added later if needed.

---

## 7. Notes for Implementation Agent

- **Create the migration first**, run it, then update the SQL files.
- **The Rust function must match the SQL function exactly.** Run the unit tests to verify.
- **Do not update SQL files that don't contain the pattern.** Only files with the inline `regexp_replace` need updating.
- **Run `cargo sqlx prepare --workspace`** after updating the SQL files to refresh the offline cache.
- **The migration date prefix `20260214000001`** should be adjusted to sort after the latest existing migration (`20260208000000`). Use `20260214000001` as specified in the architecture plan.
