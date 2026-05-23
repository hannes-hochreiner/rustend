-- NOTE: safe to apply only on databases where revision_parents contains no Merge
-- revisions. Pre-existing rows all receive position=0 and the original ordering
-- cannot be recovered after the fact. For fresh (empty) databases this is fine.
ALTER TABLE revision_parents ADD COLUMN position SMALLINT NOT NULL DEFAULT 0;
