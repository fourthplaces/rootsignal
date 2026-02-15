-- Layer 5: Clusters (read-side deduplication grouping)
-- Groups semantically similar listings/entities without modifying source data.
-- "Cluster and link, don't merge" — source data is never deleted or modified.

-- Cluster grouping
CREATE TABLE clusters (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    cluster_type TEXT NOT NULL CHECK (cluster_type IN ('listing', 'entity')),
    representative_id UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_clusters_type ON clusters(cluster_type);
CREATE INDEX idx_clusters_representative ON clusters(representative_id);

-- Polymorphic cluster membership
CREATE TABLE cluster_items (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    cluster_id UUID NOT NULL REFERENCES clusters(id) ON DELETE CASCADE,
    item_id UUID NOT NULL,
    item_type TEXT NOT NULL CHECK (item_type IN ('listing', 'entity')),
    similarity_score FLOAT,
    added_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(item_type, item_id)
);

CREATE INDEX idx_cluster_items_target ON cluster_items(item_type, item_id);
CREATE INDEX idx_cluster_items_cluster ON cluster_items(cluster_id);
