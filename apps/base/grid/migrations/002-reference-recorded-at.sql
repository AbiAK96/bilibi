-- A timestamp on reference movements, so lenses show a typed instant (a
-- datetime field and its rollups) beside the zone-free moved_on date.
ALTER TABLE base_grid_ref_movements ADD COLUMN recorded_at timestamptz;
