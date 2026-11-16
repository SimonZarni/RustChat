-- Email verification for the optional recovery address.
--
-- An address becomes verified when its owner follows an emailed link. Account
-- recovery only ever goes to a verified address.
ALTER TABLE users
    ADD COLUMN email_verified_at timestamptz,
    ADD CONSTRAINT users_email_verified_has_email
        CHECK (email_verified_at IS NULL OR email IS NOT NULL);

-- Uniqueness now applies to verified addresses only. Otherwise anyone could
-- block an address from its real owner by registering with it unverified, and
-- registration would reveal which addresses are in use.
DROP INDEX users_email_key;
CREATE UNIQUE INDEX users_verified_email_key ON users (email) WHERE email_verified_at IS NOT NULL;
CREATE INDEX users_email_idx ON users (email) WHERE email IS NOT NULL;

-- Tokens now have a purpose. A verification token is bound to the address it
-- verifies, so changing the address invalidates it.
ALTER TABLE account_tokens
    ADD COLUMN purpose text NOT NULL DEFAULT 'password_reset'
        CONSTRAINT account_tokens_purpose_known CHECK (purpose IN ('password_reset', 'email_verification')),
    ADD COLUMN email text,
    ADD CONSTRAINT account_tokens_email_matches_purpose
        CHECK ((purpose = 'email_verification') = (email IS NOT NULL));
