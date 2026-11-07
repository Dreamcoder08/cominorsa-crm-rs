-- Where each lead came from and whether it agreed to be contacted. Leads stored
-- before this migration get origin 'other' and no consent record.
ALTER TABLE leads ADD COLUMN origin TEXT NOT NULL DEFAULT 'other';
ALTER TABLE leads ADD COLUMN consent_given INTEGER;
ALTER TABLE leads ADD COLUMN consent_on TEXT;
ALTER TABLE leads ADD COLUMN consent_source TEXT;
