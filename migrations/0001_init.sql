-- Pricey initial schema.
--
-- A "wishlist item" is the *product* you want. An "item source" is one place you can
-- buy it (a retailer's product page) together with the rule for extracting its price.
-- Price snapshots hang off the source, so one product can be tracked across many shops.

create extension if not exists "pgcrypto";

create table users (
    id            uuid primary key default gen_random_uuid(),
    email         text not null,
    password_hash text not null,
    created_at    timestamptz not null default now()
);

-- Emails are compared case-insensitively; the index is the only thing enforcing that.
create unique index users_email_lower_idx on users (lower(email));

create table sessions (
    id         uuid primary key default gen_random_uuid(),
    user_id    uuid not null references users (id) on delete cascade,
    expires_at timestamptz not null,
    created_at timestamptz not null default now()
);

create index sessions_user_idx on sessions (user_id);

-- The product being tracked.
--
-- `currency` lives here rather than on the source: comparing retailers on one graph only
-- makes sense within a single currency, and doing otherwise would need exchange rates.
create table wishlist_items (
    id                 uuid primary key default gen_random_uuid(),
    user_id            uuid not null references users (id) on delete cascade,
    name               text not null,
    currency           text not null default 'ZAR',
    target_price_cents bigint,
    notes              text,
    active             boolean not null default true,
    created_at         timestamptz not null default now(),
    updated_at         timestamptz not null default now()
);

create index wishlist_items_user_idx on wishlist_items (user_id);

-- One retailer page to check the product's price at.
create table item_sources (
    id           uuid primary key default gen_random_uuid(),
    item_id      uuid not null references wishlist_items (id) on delete cascade,
    label        text not null,
    url          text not null,
    css_selector text not null,
    price_regex  text,
    active       boolean not null default true,
    created_at   timestamptz not null default now(),
    updated_at   timestamptz not null default now()
);

create index item_sources_item_idx on item_sources (item_id);

-- The same page must not be added to one product twice.
create unique index item_sources_item_url_idx on item_sources (item_id, url);

-- Failed fetches are recorded too (ok = false, price_cents null), so a retailer that
-- silently breaks its selector shows up as an error rather than just missing data.
create table price_snapshots (
    id          uuid primary key default gen_random_uuid(),
    source_id   uuid not null references item_sources (id) on delete cascade,
    price_cents bigint,
    ok          boolean not null,
    error       text,
    fetched_at  timestamptz not null default now(),

    constraint price_snapshots_ok_has_price check (not ok or price_cents is not null)
);

create index price_snapshots_source_time_idx on price_snapshots (source_id, fetched_at desc);
