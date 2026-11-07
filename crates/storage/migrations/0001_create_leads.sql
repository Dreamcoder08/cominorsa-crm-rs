CREATE TABLE leads (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    phone TEXT NOT NULL,
    ruc TEXT,
    service TEXT NOT NULL,
    stage TEXT NOT NULL,
    follow_up_on TEXT NOT NULL
);

CREATE INDEX leads_follow_up_on ON leads (follow_up_on);
