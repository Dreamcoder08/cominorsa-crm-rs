ALTER TABLE cases ADD COLUMN contract_total_cents INTEGER;

ALTER TABLE leads ADD COLUMN renewal_of_case TEXT REFERENCES cases (id);

CREATE UNIQUE INDEX leads_renewal_of_case ON leads (renewal_of_case)
    WHERE renewal_of_case IS NOT NULL;

CREATE TABLE billing_milestones (
    id TEXT PRIMARY KEY NOT NULL,
    case_id TEXT NOT NULL REFERENCES cases (id),
    position INTEGER NOT NULL,
    title TEXT NOT NULL,
    amount_cents INTEGER NOT NULL,
    due_on TEXT NOT NULL,
    paid_on TEXT,
    UNIQUE (case_id, position)
);
