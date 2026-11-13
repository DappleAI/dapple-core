-- Junction table linking guides to their derived playbooks.

CREATE TABLE rev.guide_playbooks (
    id UUID PRIMARY KEY DEFAULT uuidv7(),
    guide_id UUID NOT NULL REFERENCES rev.guides(id),
    playbook_id UUID NOT NULL REFERENCES rev.playbooks(id),
    chapter_index SMALLINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (guide_id, playbook_id)
);

CREATE INDEX idx_guide_playbooks_guide_id ON rev.guide_playbooks(guide_id);
CREATE INDEX idx_guide_playbooks_playbook_id ON rev.guide_playbooks(playbook_id);
