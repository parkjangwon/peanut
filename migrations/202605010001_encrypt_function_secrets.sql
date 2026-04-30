ALTER TABLE function_version_secrets ADD COLUMN secret_ciphertext TEXT;
ALTER TABLE function_version_secrets ADD COLUMN encryption_version INTEGER;
