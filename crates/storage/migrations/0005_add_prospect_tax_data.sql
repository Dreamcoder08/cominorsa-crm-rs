ALTER TABLE prospects ADD COLUMN ruc TEXT;
ALTER TABLE prospects ADD COLUMN tax_status TEXT;
ALTER TABLE prospects ADD COLUMN tax_condition TEXT;
ALTER TABLE prospects ADD COLUMN fiscal_address TEXT;
ALTER TABLE prospects ADD COLUMN tax_match_approximate INTEGER;

CREATE INDEX prospects_ruc ON prospects (ruc);
