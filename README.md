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
- **Email + password auth** — server-side sessions, argon2 hashes, per-user data, with
  address verification by email before an account can be used.

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

Then create an account at `/signup`. Signing up sends a confirmation link; **with no
`RESEND_API_KEY` set the link is written to the log instead of emailed**, so local
development needs no Resend account:

```
WARN pricey::server::email: RESEND_API_KEY is not set; logging the verification link
     instead of sending it recipient=you@example.com link=http://127.0.0.1:3000/verify?token=...
```

Open that link, sign in, then add an item and give it a retailer.

### Email verification

An account cannot sign in until its address is confirmed — that is what stops a stranger
registering with a throwaway or someone else's address.

- Signup creates the account but **no session**, and emails a link valid for 24 hours
- `GET /verify?token=…` consumes the token and redirects to `/login?verify=…`; it is a
  plain route, not a Leptos page, so it works in any client that opens the link
- Tokens are single-use, and only their **SHA-256 is stored** — the token itself exists
  only in the email, so a leaked database backup cannot be used to verify accounts
- `/login` offers a "send a new link" form when that is actually the problem. It reports
  the same result for every address and is rate limited, so it is neither an
  account-existence oracle nor a way to have this service repeatedly mail a third party

To send real mail, set `RESEND_API_KEY` and `EMAIL_FROM` (the from-address must be on a
domain verified in Resend), and `APP_BASE_URL` so links point at the right host.

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

Three different problems wear the same status code, and the fix depends on which one it is.

**Every request fails, everywhere.** Sites behind Cloudflare reject requests that carry a
browser `User-Agent` but omit the headers a browser sends with it — the *mismatch* is what
looks automated. The client in `src/server/price.rs` already sends a full navigation header
set (`Sec-Fetch-Dest` is the one that matters in practice).

**Only under load.** That is rate limiting. Sources are grouped by host and each host is
fetched serially with a short gap (`SAME_HOST_DELAY` in `src/server/runner.rs`) so tracking
several products at one shop does not trip it. If a retailer is stricter, raise that value.

**Fails from the deployed host, works locally.** The retailer blocks your host's IP range,
and no header set will change that — the request never gets to be judged on its shape.
Wootware blocks Vercel's egress this way. Use manual entry (below).

Some sites render prices with JavaScript. Those cannot be scraped this way at all, since
only the served HTML is parsed — look for a `<meta itemprop="price">` tag or a JSON-LD
block in the page source instead.

### Retailers you have to enter by hand

Tick **"Enter prices by hand"** on the source and **save** it. It is then skipped by every
refresh — cron, "Refresh all" and the per-item button — so it stops recording a failure
every run, but it still counts toward the item's best price and still draws its line on the
chart. This is separate from unticking "Include this retailer", which drops it out of the
best-price comparison entirely.

Saving is what makes the entry panel appear, and the order matters: until the flag is
stored, cron is still refreshing that retailer, so there would be nothing manual about it
yet.

Two ways to get a price in, both on the source's editor:

- **Paste the page source.** Open the product page in your browser, `Ctrl+U`, `Ctrl+A`,
  `Ctrl+C`, paste, and press *Extract & record*. The CSS selector in the field above runs
  against what you pasted through the same `price::extract` the scraper uses, so the price
  is read and normalised identically — you are only supplying the fetch the server cannot
  make. It is the selector as shown, not as last saved, so you can adjust it and paste
  again without saving in between. Keep one configured for this reason; it is optional in
  manual mode, not useless.
- **Type the price.** `R 1 299,00`, `1299`, whatever the page showed — the same parser
  handles it, and `Enter` records rather than saving the row. This is the practical option
  on a phone, where viewing page source is not.

Both report the number they stored, which is worth reading: the parser takes the first run
of digits it finds, so a page that says "Was R1 999, now R899" records 1999 unless you give
it just the price. Both are stored marked `manual`, and show a **manual** badge in the
history table so a number you supplied is never mistaken for one the tracker measured.

A pasted page is capped at 4 MB, and only `/api/sources/record-html` accepts a body that
large — every other server function keeps Axum's default, so a login endpoint cannot be
made to buffer a multi-megabyte body.

An iframe, for the record, cannot substitute for this: retailers send `X-Frame-Options`
(Wootware sends `SAMEORIGIN`), so the page will not render on this origin at all, and even
if it did, a cross-origin document is unreadable to JavaScript — no selector could reach
into it.

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

The image is defined in **`Dockerfile.vercel`** — Vercel looks for that name, and it is the
only Dockerfile in the repo so the deployed image and the local one cannot drift apart.

Rehearse the container locally before deploying — this builds that same file:

```sh
docker compose --profile full up --build   # http://localhost:8080
```

To build it directly, point at it explicitly, since it is not named `Dockerfile`:

```sh
docker build -f Dockerfile.vercel -t pricey .
```

Environment variables to set on the Vercel project:

| Variable | Notes |
|---|---|
| `PORT` | **Required — set it to `8080`.** See the note below |
| `DATABASE_URL` | Neon's **pooled** (`-pooler`) connection string, with `?sslmode=require` |
| `CRON_SECRET` | Shared secret for `/api/fetch-prices`; Vercel Cron sends it automatically |
| `APP_ENV` | Set to `production` so session cookies are marked `Secure` |
| `APP_BASE_URL` | Public URL of the deployment; verification links are built from it |
| `RESEND_API_KEY` | Resend API key. **Without it, links are logged rather than sent** |
| `EMAIL_FROM` | Sender address, on a domain verified in Resend |

`PORT` is not optional and not injected for you. Vercel routes container traffic to
**port 80** unless the project defines a `PORT` environment variable, and that same variable
is what the container reads to decide where to listen (`src/main.rs`). The image binds 8080
and runs as a non-root user that cannot take a privileged port, so the two only agree when
`PORT=8080` is set on the project. Leave it unset and every route — pages, static assets and
`/api/*` alike — fails with `INTERNAL_FUNCTION_INVOCATION_FAILED`, because nothing is
listening where the router is knocking.

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
