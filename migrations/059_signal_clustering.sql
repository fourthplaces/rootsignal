BEGIN;

-- Step 1: Add cluster_type column to cluster_items (nullable for backfill)
ALTER TABLE cluster_items ADD COLUMN cluster_type TEXT;

-- Step 2: Backfill from parent clusters table
UPDATE cluster_items ci SET cluster_type = c.cluster_type FROM clusters c WHERE c.id = ci.cluster_id;

-- Step 3: Set NOT NULL
ALTER TABLE cluster_items ALTER COLUMN cluster_type SET NOT NULL;

-- Step 4: Update CHECK constraints to include 'signal'
ALTER TABLE clusters DROP CONSTRAINT clusters_cluster_type_check;
ALTER TABLE clusters ADD CONSTRAINT clusters_cluster_type_check CHECK (cluster_type IN ('listing', 'entity', 'signal'));

ALTER TABLE cluster_items DROP CONSTRAINT cluster_items_item_type_check;
ALTER TABLE cluster_items ADD CONSTRAINT cluster_items_item_type_check CHECK (item_type IN ('listing', 'entity', 'signal'));

ALTER TABLE cluster_items ADD CONSTRAINT cluster_items_cluster_type_check CHECK (cluster_type IN ('listing', 'entity', 'signal'));

-- Step 5: Swap unique constraint for multi-dimension support
ALTER TABLE cluster_items DROP CONSTRAINT cluster_items_item_type_item_id_key;
ALTER TABLE cluster_items ADD CONSTRAINT cluster_items_cluster_type_item_type_item_id_key UNIQUE (cluster_type, item_type, item_id);

-- Step 6: Composite FK to enforce denormalization consistency
ALTER TABLE clusters ADD CONSTRAINT clusters_id_cluster_type_unique UNIQUE (id, cluster_type);
ALTER TABLE cluster_items ADD CONSTRAINT cluster_items_cluster_id_cluster_type_fk FOREIGN KEY (cluster_id, cluster_type) REFERENCES clusters (id, cluster_type) ON DELETE CASCADE;

-- Step 7: Migrate existing 'signal' cluster_type values to 'entity' (the correct dimension name)
UPDATE clusters SET cluster_type = 'entity' WHERE cluster_type = 'signal';
UPDATE cluster_items SET cluster_type = 'entity' WHERE cluster_type = 'signal';

COMMIT;
