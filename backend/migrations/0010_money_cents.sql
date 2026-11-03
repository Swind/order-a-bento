-- Preserve dollar values, including immutable order/settlement snapshots.
-- Refuse corrupted/oversized legacy values instead of SQLite promoting overflow to REAL.
CREATE TABLE money_migration_guard (ok INTEGER NOT NULL CHECK (ok = 1));
INSERT INTO money_migration_guard SELECT CASE WHEN EXISTS (
    SELECT 1 FROM menu_items WHERE typeof(price) <> 'integer'
    UNION ALL SELECT 1 FROM offered_items WHERE typeof(price) <> 'integer' OR price NOT BETWEEN 0 AND 92233720368547758
    UNION ALL SELECT 1 FROM submission_lines WHERE typeof(unit_price) <> 'integer' OR unit_price NOT BETWEEN 0 AND 92233720368547758
    UNION ALL SELECT 1 FROM settlements WHERE paid_amount IS NOT NULL AND (typeof(paid_amount) <> 'integer' OR paid_amount NOT BETWEEN -92233720368547758 AND 92233720368547758)
) THEN 0 ELSE 1 END;
DROP TABLE money_migration_guard;

-- menu_items is a leaf table (snapshots intentionally have no FK back to it).
CREATE TABLE menu_items_cents (
  id TEXT PRIMARY KEY,
  vendor_id TEXT NOT NULL REFERENCES vendors(id) ON DELETE CASCADE,
  name TEXT NOT NULL, name_key TEXT NOT NULL,
  price INTEGER NOT NULL CHECK (typeof(price) = 'integer' AND price BETWEEN 1 AND 10000000),
  description TEXT, category TEXT, sort_order INTEGER NOT NULL DEFAULT 0,
  archived INTEGER NOT NULL DEFAULT 0, updated_at TEXT NOT NULL,
  UNIQUE (vendor_id, name_key)
);
INSERT INTO menu_items_cents SELECT id,vendor_id,name,name_key,price*100,description,category,sort_order,archived,updated_at FROM menu_items;
DROP TABLE menu_items;
ALTER TABLE menu_items_cents RENAME TO menu_items;
UPDATE offered_items SET price=price*100;
UPDATE submission_lines SET unit_price=unit_price*100;
UPDATE settlements SET paid_amount=paid_amount*100 WHERE paid_amount IS NOT NULL;
-- Proposal/draft/tool-call JSON stays in external dollars; the serde boundary converts it.
