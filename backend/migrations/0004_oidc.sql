-- no-transaction
-- Preserve data owners and their foreign keys; old identities retain their issuer.
PRAGMA foreign_keys = OFF;
BEGIN;
CREATE TABLE organizers_new (
    id TEXT PRIMARY KEY,
    oidc_issuer TEXT,
    oidc_sub TEXT,
    email TEXT NOT NULL,
    display_name TEXT NOT NULL,
    avatar_url TEXT,
    created_at TEXT NOT NULL,
    last_login_at TEXT NOT NULL,
    password_hash TEXT,
    UNIQUE (oidc_issuer, oidc_sub),
    CHECK ((oidc_issuer IS NOT NULL AND oidc_sub IS NOT NULL AND password_hash IS NULL)
        OR (oidc_issuer IS NULL AND oidc_sub IS NULL AND password_hash IS NOT NULL))
);
INSERT INTO organizers_new
SELECT id, CASE WHEN google_sub IS NULL THEN NULL
    WHEN google_sub LIKE 'dev:%' THEN 'urn:order-a-bento:dev'
    ELSE 'https://accounts.google.com' END,
    google_sub, email, display_name, avatar_url, created_at, last_login_at, password_hash
FROM organizers;
DROP TABLE organizers;
ALTER TABLE organizers_new RENAME TO organizers;
CREATE UNIQUE INDEX idx_organizers_local_email ON organizers(email) WHERE oidc_sub IS NULL;
COMMIT;
PRAGMA foreign_keys = ON;
