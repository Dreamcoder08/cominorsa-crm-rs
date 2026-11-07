CREATE TABLE cases (
    id TEXT PRIMARY KEY NOT NULL,
    lead_id TEXT NOT NULL UNIQUE REFERENCES leads (id),
    client_name TEXT NOT NULL,
    ruc TEXT,
    service TEXT NOT NULL,
    owner TEXT,
    opened_on TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'open'
);

CREATE TABLE case_steps (
    id TEXT PRIMARY KEY NOT NULL,
    case_id TEXT NOT NULL REFERENCES cases (id),
    position INTEGER NOT NULL,
    title TEXT NOT NULL,
    due_on TEXT NOT NULL,
    done_on TEXT,
    UNIQUE (case_id, position)
);
