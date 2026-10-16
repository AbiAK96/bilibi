-- K17: when the bootstrap administrator was bound to the receipt's company
-- through an agent Person and affiliation (or found already naming a
-- Person). Set once; a settled receipt is never revisited, so an operator's
-- later change to that identity stands.
ALTER TABLE user_bootstrap ADD COLUMN affiliation_settled_at timestamptz;
