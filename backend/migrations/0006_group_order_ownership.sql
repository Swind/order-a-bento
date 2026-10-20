-- Existing orders deliberately remain personal, regardless of snapshot sources.
ALTER TABLE group_orders ADD COLUMN sharing_group_id TEXT REFERENCES sharing_groups(id);
CREATE INDEX group_orders_sharing_group ON group_orders(sharing_group_id);

-- Only ownership is immutable; normal lifecycle and snapshot edits remain allowed.
CREATE TRIGGER group_orders_ownership_immutable
BEFORE UPDATE OF organizer_id, sharing_group_id ON group_orders
WHEN NEW.organizer_id IS NOT OLD.organizer_id
  OR NEW.sharing_group_id IS NOT OLD.sharing_group_id
BEGIN
    SELECT RAISE(ABORT, 'group order ownership is immutable');
END;
