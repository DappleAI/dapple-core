Create a database migration for a new render profile with tag filter and style configuration.

## Input

$ARGUMENTS should be: `<profile-name> <tags>` (e.g., `agent-context agent,coder` or `product-facing product`)

## Steps

1. **Parse the input:**
   - Profile name (kebab-case identifier)
   - Comma-separated list of tags to include in the filter
   - Blocks with no tags (universal) are always included regardless of filter

2. **Create the migration file:**
   ```bash
   # Generate timestamp-based filename
   TIMESTAMP=$(date +%Y%m%d%H%M%S)
   MIGRATION_FILE="crates/dapple-data/migrations/${TIMESTAMP}_insert_render_profile_<profile-name>.sql"
   ```

3. **Write the migration SQL:**
   ```sql
   -- Insert render profile: <profile-name>
   -- Tags: <tag-list>
   -- Universal (untagged) blocks are always included.

   INSERT INTO rev.render_profiles (
       object_id,
       name,
       tag_filter,
       style_config,
       created_at
   ) VALUES (
       gen_random_uuid(),
       '<profile-name>',
       '["<tag1>", "<tag2>"]'::jsonb,
       '{}'::jsonb,
       now()
   );
   ```

4. **Optionally add style configuration** — If the user specifies style preferences:
   ```sql
   -- With style config
   INSERT INTO rev.render_profiles (
       object_id,
       name,
       tag_filter,
       style_config,
       created_at
   ) VALUES (
       gen_random_uuid(),
       '<profile-name>',
       '["agent", "coder"]'::jsonb,
       '{"font_size": "12pt", "page_margin": "1in", "code_theme": "monokai"}'::jsonb,
       now()
   );
   ```

5. **Verify the tags are valid** — Must match keys in `rev.guide_tags`:
   - `agent`, `product`, `coder`, `auditor`, `planner`
   - Adding new tags requires a separate migration to `rev.guide_tags`

6. **Run the migration:**
   ```bash
   nu -l -c 'cargo sqlx migrate run'
   ```

7. **Verify:**
   ```bash
   nu -l -c 'cargo sqlx prepare --workspace'
   ```

## Example

Create an "agent-context" profile that includes blocks tagged `agent` and `coder`:

```sql
-- Migration: 20260214120000_insert_render_profile_agent_context.sql

INSERT INTO rev.render_profiles (
    object_id,
    name,
    tag_filter,
    style_config,
    created_at
) VALUES (
    gen_random_uuid(),
    'agent-context',
    '["agent", "coder"]'::jsonb,
    '{}'::jsonb,
    now()
);
```

## Validation

- [ ] Migration file exists in `crates/dapple-data/migrations/`
- [ ] Migration filename follows `YYYYMMDDHHMMSS_insert_render_profile_<name>.sql` convention
- [ ] All tags in `tag_filter` exist in `rev.guide_tags`
- [ ] `style_config` is valid JSON (even if empty `{}`)
- [ ] Migration runs without errors
- [ ] `cargo sqlx prepare` succeeds after migration

## Anti-Patterns

- Do NOT use tags that aren't in `rev.guide_tags` — add them in a separate migration first
- Do NOT forget that universal (untagged) blocks are always included — don't add "universal" to the filter
- Do NOT use `timestamptz(0)` or any precision specifier — use `timestamptz`
- Do NOT use `varchar` — use `text` with CHECK constraints if length-limited
