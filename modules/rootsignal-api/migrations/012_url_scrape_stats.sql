CREATE TABLE url_scrape_stats (
    url         TEXT    PRIMARY KEY,
    fetch_count BIGINT  NOT NULL DEFAULT 0,
    yield_count BIGINT  NOT NULL DEFAULT 0
);
