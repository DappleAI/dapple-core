-- Add new guide block directive types: scope, objective, deliverable, block
INSERT INTO rev.guide_block_directive_types (key) VALUES
    ('scope'),
    ('objective'),
    ('deliverable'),
    ('block')
ON CONFLICT (key) DO NOTHING;
