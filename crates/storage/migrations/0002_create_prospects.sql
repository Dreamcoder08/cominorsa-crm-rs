CREATE TABLE prospects (
    kind TEXT NOT NULL,
    name TEXT NOT NULL,
    profile TEXT NOT NULL,
    concessions INTEGER NOT NULL,
    hectares REAL NOT NULL,
    departments TEXT NOT NULL,
    provinces TEXT NOT NULL,
    substances TEXT NOT NULL,
    concession_states TEXT NOT NULL,
    potential_service TEXT NOT NULL,
    needs_review INTEGER NOT NULL,
    source TEXT NOT NULL,
    PRIMARY KEY (kind, name)
);

CREATE INDEX prospects_ranking ON prospects (needs_review, concessions DESC, name);
