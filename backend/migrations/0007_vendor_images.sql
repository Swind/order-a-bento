CREATE TABLE vendor_images (
  vendor_id TEXT PRIMARY KEY REFERENCES vendors(id) ON DELETE CASCADE,
  revision TEXT NOT NULL,
  content_type TEXT NOT NULL CHECK (content_type = 'image/jpeg'),
  data BLOB NOT NULL CHECK (length(data) BETWEEN 1 AND 5242880),
  width INTEGER NOT NULL CHECK (width BETWEEN 1 AND 1568),
  height INTEGER NOT NULL CHECK (height BETWEEN 1 AND 1568),
  updated_at TEXT NOT NULL
);
