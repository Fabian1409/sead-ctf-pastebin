#!/bin/sh

sqlite3 db/clipboard.db <<'END_SQL'
.timeout 2000
CREATE TABLE entries (
  id TEXT PRIMARY KEY NOT NULL,
  content TEXT NOT NULL,
  encrypted INT NOT NULL,
  key TEXT
);
END_SQL