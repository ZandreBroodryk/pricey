# pricey

A personal price tracker. Add things you want, point the tracker at one or more retailer
pages for each, and it records what they cost over time — viewable as a table and a graph.

Built with [Leptos](https://leptos.dev) 0.8 (SSR + hydration) on Axum, backed by Postgres.

## What it does

- **Wishlist CRUD** — items you're tracking, each with an optional target price.
- **Multiple retailers per item** — one product can be watched at several shops, each with
  its own URL and CSS selector. Prices are compared on shared axes.
- **Scheduled price checks** — `GET|POST /api/fetch-prices` refreshes every tracked source
  and records a timestamped snapshot. Vercel Cron calls it daily; the UI can trigger the
  same run on demand.
- **Price history** — a multi-series SVG chart and a filterable table, including failed
  fetches so a broken selector is visible rather than silently absent.
- **Email + password auth** — server-side sessions, argon2 hashes, per-user data.

## Requirements

- Rust (stable; `rust-toolchain.toml` pins it and adds the `wasm32-unknown-unknown` target)
- [`cargo-leptos`](https://github.com/leptos-rs/cargo-leptos) — `cargo install cargo-leptos --locked`
- [`sqlx-cli`](https://github.com/launchbadge/sqlx) — `cargo install sqlx-cli --no-default-features --features rustls,postgres`
- Docker, for the local Postgres

## Getting started

```sh
cp .env.example .env
docker compose up -d db          # Postgres on localhost:5432
cargo sqlx migrate run           # apply migrations/
cargo leptos watch               # http://127.0.0.1:3000
```

Then create an account at `/signup`, add an item, and give it a retailer.

### Adding a retailer

Each source needs a **URL** and a **CSS selector** that points at the price on that page.
Find one with your browser's inspector — `span.price-now`, or `meta[itemprop="price"]`
(the `content` attribute is read automatically, and is usually the most stable choice).

If the selector matches a larger blob of text, add an optional **regex**; capture group 1
is used when present. The **Test** button runs a real fetch and shows what would be
extracted *without* recording it, which is the fast way to get a selector right.

**Prefer a selector anchored to the product block over a positional one.** Product pages
usually repeat the same price class in "related products" carousels, so `span.price` or
`span.price:nth-child(1)` may quietly latch onto a different product if the page layout
shifts — recording a wrong price rather than an error. Anchor to the container instead:

```
.add-to-cart-wrapper .price-box .price     good - tied to the buy box
span.price:nth-child(1)                    fragile - position-dependent
meta[itemprop="price"]                     best, when the page provides it
```

### If a retailer returns HTTP 403

Sites behind Cloudflare reject requests that carry a browser `User-Agent` but omit the
headers a browser sends with it — the *mismatch* is what looks automated. The client in
`src/server/price.rs` therefore sends a full navigation header set (`Sec-Fetch-Dest` is the
one that matters in practice).

A 403 that appears only under load is a different problem: rate limiting. Sources are
grouped by host and each host is fetched serially with a short gap
(`SAME_HOST_DELAY` in `src/server/runner.rs`) so tracking several products at one shop does
not trip it. If a retailer is stricter, raise that value.

Some sites render prices with JavaScript. Those cannot be scraped this way at all, since
only the served HTML is parsed — look for a `<meta itemprop="price">` tag or a JSON-LD
block in the page source instead.

## Database

SQL is checked at compile time by `sqlx`'s macros. To keep the Docker build hermetic, the
query metadata is committed to `.sqlx/` and the build runs with `SQLX_OFFLINE=true`.

**Re-generate the cache after changing any query or migration:**

```sh
cargo sqlx prepare -- --features ssr
```

and verify it is current with:

```sh
cargo sqlx prepare --check -- --features ssr
```

A stale `.sqlx/` breaks the container build with no local signal, so treat that check as
part of committing. Note that `.sqlx/` is deliberately excluded from both `.gitignore` and
`.dockerignore`.

## Tests

```sh
cargo test --features ssr        # price parsing, chart scales, formatting, auth hashing
cargo leptos end-to-end          # Playwright (needs `npm install` in end2end/ first)
```

Checking both compilation targets is worthwhile, since the second catches a server-only
dependency leaking into shared code:

```sh
cargo check --features ssr
cargo check --lib --features hydrate --no-default-features --target wasm32-unknown-unknown
```

## The fetch endpoint

```sh
curl -i -X POST -H "Authorization: Bearer $CRON_SECRET" \
  http://localhost:3000/api/fetch-prices
```

Returns a JSON report of attempted/succeeded/failed. Without a matching bearer token it
returns 401; if `CRON_SECRET` is not configured at all it refuses to run rather than
defaulting to open.

## Deployment

Targets Vercel's Docker deployments with a [Neon](https://neon.tech) database.

Rehearse the container locally before deploying — this is the same image Vercel builds:

```sh
docker compose --profile full up --build   # http://localhost:8080
```

Environment variables to set on the Vercel project:

| Variable | Notes |
|---|---|
| `DATABASE_URL` | Neon's **pooled** (`-pooler`) connection string, with `?sslmode=require` |
| `CRON_SECRET` | Shared secret for `/api/fetch-prices`; Vercel Cron sends it automatically |
| `APP_ENV` | Set to `production` so session cookies are marked `Secure` |

Migrations run automatically at startup, so a fresh Neon branch provisions itself on the
first boot. `vercel.json` schedules the daily price check (the free tier allows one cron
invocation per day).

## Notes and limitations

- **Signup is open.** Anyone who reaches the deployment can create an account. Their data
  is scoped to them, but they consume your database rows and outbound requests. To close
  it, change `signup_allowed()` in `src/server/auth.rs`.
- **One currency per item.** All of an item's sources are assumed to quote the same
  currency; comparing across currencies would need exchange rates.
- **Timestamps display in UTC.**
- Scraping depends on retailers' markup, which changes. Failed fetches are recorded rather
  than discarded so you can see when a selector has gone stale.
