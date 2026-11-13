-- Add brief_markdown directive type so guide ingest can insert guide_blocks
-- for :::brief-markdown directives. The brief_units + brief_unit_markdown
-- storage chain already exists; this seeds the directive-type lookup.
INSERT INTO rev.guide_block_directive_types (key) VALUES
    ('brief_markdown')
ON CONFLICT (key) DO NOTHING;
