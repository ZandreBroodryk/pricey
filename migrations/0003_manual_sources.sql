-- Manual sources.
--
-- Some retailers block the deployed host's IP outright (Wootware blocks Vercel's egress),
-- which no amount of header tuning fixes. Such a source is marked `manual`: the refresh
-- runner skips it, so it stops recording a failure every cron run, and its prices arrive
-- from the user instead -- either typed in, or extracted from page HTML they paste.

alter table item_sources add column manual boolean not null default false;

-- A scraped source is useless without a selector. A manual one may leave it blank and
-- rely on typed-in prices instead. Existing rows all carry a selector, so this holds.
alter table item_sources
    add constraint item_sources_selector_unless_manual
    check (manual or css_selector <> '');

-- Marks a snapshot the user supplied rather than one the tracker fetched itself, so a
-- typed-in number is distinguishable from a measured one in the history.
alter table price_snapshots add column manual boolean not null default false;
