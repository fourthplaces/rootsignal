-- Cross-source link graph: outbound links from page snapshots with rich context
CREATE TABLE page_snapshot_links (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    source_snapshot_id UUID NOT NULL REFERENCES page_snapshots(id) ON DELETE CASCADE,
    target_url TEXT NOT NULL,
    target_snapshot_id UUID REFERENCES page_snapshots(id) ON DELETE SET NULL,
    anchor_text TEXT,
    surrounding_text TEXT,
    section TEXT,
    UNIQUE(source_snapshot_id, target_url)
);

CREATE INDEX idx_page_snapshot_links_source ON page_snapshot_links(source_snapshot_id);
CREATE INDEX idx_page_snapshot_links_target_url ON page_snapshot_links(target_url);
CREATE INDEX idx_page_snapshot_links_target_snapshot ON page_snapshot_links(target_snapshot_id)
    WHERE target_snapshot_id IS NOT NULL;
CREATE INDEX idx_page_snapshot_links_unresolved ON page_snapshot_links(target_url)
    WHERE target_snapshot_id IS NULL;
