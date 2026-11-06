CREATE TABLE settings (
    key TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
);

CREATE TABLE team_members (
    position INTEGER PRIMARY KEY NOT NULL,
    name TEXT NOT NULL
);
