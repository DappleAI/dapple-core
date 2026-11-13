-- Add new guide block directive types for brief structure definitions
INSERT INTO rev.guide_block_directive_types (key) VALUES
    ('brief_input'),
    ('brief_notice'),
    ('brief_output'),
    ('brief_group'),
    ('value_slot'),
    ('brief')
ON CONFLICT (key) DO NOTHING;
