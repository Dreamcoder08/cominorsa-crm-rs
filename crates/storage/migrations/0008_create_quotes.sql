-- One row per year. The counter survives anything, so a number is never reused.
CREATE TABLE quote_numbers (
    year INTEGER PRIMARY KEY NOT NULL,
    last_sequence INTEGER NOT NULL
);

CREATE TABLE quotes (
    id TEXT PRIMARY KEY NOT NULL,
    lead_id TEXT NOT NULL REFERENCES leads (id),
    year INTEGER NOT NULL,
    sequence INTEGER NOT NULL,
    version INTEGER NOT NULL,
    supersedes TEXT REFERENCES quotes (id),
    client_name TEXT NOT NULL,
    client_ruc TEXT,
    service TEXT NOT NULL,
    issued_on TEXT NOT NULL,
    valid_until TEXT NOT NULL,
    notes TEXT NOT NULL,
    status TEXT NOT NULL,
    igv_basis_points INTEGER NOT NULL,
    company_name TEXT NOT NULL,
    company_ruc TEXT NOT NULL,
    company_address TEXT NOT NULL,
    company_phone TEXT NOT NULL,
    company_email TEXT NOT NULL,
    UNIQUE (year, sequence, version)
);

CREATE INDEX quotes_lead ON quotes (lead_id);

CREATE UNIQUE INDEX quotes_supersedes ON quotes (supersedes)
    WHERE supersedes IS NOT NULL;

CREATE TABLE quote_lines (
    quote_id TEXT NOT NULL REFERENCES quotes (id),
    position INTEGER NOT NULL,
    description TEXT NOT NULL,
    quantity INTEGER NOT NULL,
    unit_price_cents INTEGER NOT NULL,
    PRIMARY KEY (quote_id, position)
);
