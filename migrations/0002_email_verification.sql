-- Email verification.
--
-- An account is usable only once its address has been confirmed, which is what stops a
-- stranger signing up with someone else's address (or a throwaway one) and using the
-- instance. `email_verified_at` null means unverified.

alter table users add column email_verified_at timestamptz;

-- Accounts created before this migration predate verification. Treat them as verified
-- rather than silently locking their owners out.
update users set email_verified_at = created_at where email_verified_at is null;

create table email_verifications (
    id          uuid primary key default gen_random_uuid(),
    user_id     uuid not null references users (id) on delete cascade,
    -- SHA-256 of the token, hex encoded. The token itself only ever exists in the email,
    -- so a leaked database backup cannot be used to verify accounts.
    token_hash  text not null,
    expires_at  timestamptz not null,
    consumed_at timestamptz,
    created_at  timestamptz not null default now()
);

create unique index email_verifications_token_hash_idx on email_verifications (token_hash);
create index email_verifications_user_idx on email_verifications (user_id, created_at desc);
