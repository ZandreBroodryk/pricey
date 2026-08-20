//! Fetching and extracting a price from a retailer's product page.
//!
//! Extraction is deliberately dumb and configurable per source: fetch the page, run a
//! CSS selector, optionally narrow with a regex, then normalise whatever text falls out
//! into integer cents. Site-specific knowledge lives in the database, not in this file.

use std::time::Duration;

use crate::fmt::parse_price;

/// What a fetch produced. Failures are values, not errors: they get recorded too.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FetchOutcome {
    pub price_cents: Option<i64>,
    /// Raw text the selector matched. Surfaced by the "test" button when parsing fails.
    pub matched_text: Option<String>,
    pub error: Option<String>,
}

impl FetchOutcome {
    fn failed(error: impl Into<String>) -> Self {
        Self {
            price_cents: None,
            matched_text: None,
            error: Some(error.into()),
        }
    }
}

/// Builds the shared HTTP client. One client per refresh run, so connections are reused.
///
/// The headers matter as much as the user agent. Retailers behind Cloudflare (Wootware,
/// among others) return **403 with `cf-mitigated: challenge`** when a request carries a
/// browser user agent but omits the headers a browser always sends alongside it -- the
/// mismatch is what looks automated, not the user agent itself. `Sec-Fetch-Dest: document`
/// is the one that clears it in practice; the rest are sent because a real navigation
/// sends them together and a partial set is what got flagged in the first place.
///
/// `Accept-Encoding` is deliberately absent: reqwest sets and *decodes* it automatically
/// from the `gzip`/`brotli` features, and declaring it by hand would mean receiving
/// compressed bytes that never get decompressed.
pub fn client() -> reqwest::Client {
    use reqwest::header::{
        HeaderMap, HeaderName, HeaderValue, ACCEPT, ACCEPT_LANGUAGE, UPGRADE_INSECURE_REQUESTS,
    };

    let mut headers = HeaderMap::new();
    headers.insert(
        ACCEPT,
        HeaderValue::from_static(
            "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8",
        ),
    );
    headers.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("en-ZA,en;q=0.9"));
    headers.insert(UPGRADE_INSECURE_REQUESTS, HeaderValue::from_static("1"));
    for (name, value) in [
        ("sec-fetch-dest", "document"),
        ("sec-fetch-mode", "navigate"),
        ("sec-fetch-site", "none"),
        ("sec-fetch-user", "?1"),
    ] {
        headers.insert(
            HeaderName::from_static(name),
            HeaderValue::from_static(value),
        );
    }

    reqwest::Client::builder()
        .user_agent(
            "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) \
             Chrome/126.0.0.0 Safari/537.36",
        )
        .default_headers(headers)
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .expect("HTTP client config is static and valid")
}

/// Fetches `url` and extracts a price using `selector` (and optionally `price_regex`).
pub async fn fetch_price(
    client: &reqwest::Client,
    url: &str,
    selector: &str,
    price_regex: Option<&str>,
) -> FetchOutcome {
    // Validate the selector before spending a request on it.
    let parsed_selector = match scraper::Selector::parse(selector) {
        Ok(s) => s,
        Err(_) => return FetchOutcome::failed("invalid CSS selector"),
    };

    let compiled_regex = match price_regex.filter(|r| !r.trim().is_empty()) {
        Some(r) => match regex::Regex::new(r) {
            Ok(re) => Some(re),
            Err(e) => return FetchOutcome::failed(format!("invalid price regex: {e}")),
        },
        None => None,
    };

    let response = match client.get(url).send().await {
        Ok(r) => r,
        Err(e) => return FetchOutcome::failed(format!("request failed: {e}")),
    };

    if !response.status().is_success() {
        return FetchOutcome::failed(format!("HTTP {}", response.status()));
    }

    let body = match response.text().await {
        Ok(b) => b,
        Err(e) => return FetchOutcome::failed(format!("could not read response body: {e}")),
    };

    extract(&body, &parsed_selector, compiled_regex.as_ref())
}

/// Selector -> optional regex -> cents. Split out from the network so it can be tested.
pub fn extract(
    html: &str,
    selector: &scraper::Selector,
    price_regex: Option<&regex::Regex>,
) -> FetchOutcome {
    let document = scraper::Html::parse_document(html);

    let Some(raw) = document.select(selector).find_map(candidate_text) else {
        return FetchOutcome::failed("selector matched nothing on the page");
    };

    // The regex narrows the matched text; capture group 1 if present, else the whole match.
    let narrowed = match price_regex {
        Some(re) => match re.captures(&raw) {
            Some(caps) => caps
                .get(1)
                .or_else(|| caps.get(0))
                .map(|m| m.as_str().to_string())
                .unwrap_or_default(),
            None => {
                return FetchOutcome {
                    price_cents: None,
                    matched_text: Some(raw),
                    error: Some("price regex did not match the selected text".into()),
                }
            }
        },
        None => raw.clone(),
    };

    match parse_price(&narrowed) {
        Some(price_cents) => FetchOutcome {
            price_cents: Some(price_cents),
            matched_text: Some(raw),
            error: None,
        },
        None => FetchOutcome {
            price_cents: None,
            matched_text: Some(raw.clone()),
            error: Some(format!("could not read a price out of {narrowed:?}")),
        },
    }
}

/// Text worth trying from a matched element.
///
/// Prefers the `content` attribute so that `<meta itemprop="price" content="1299.00">`
/// -- the most reliable price marker on a well-marked-up page -- works as a selector target.
fn candidate_text(element: scraper::ElementRef<'_>) -> Option<String> {
    if let Some(content) = element.value().attr("content") {
        let content = content.trim();
        if !content.is_empty() {
            return Some(content.to_string());
        }
    }

    let text: String = element.text().collect::<String>();
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    (!text.is_empty()).then_some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sel(s: &str) -> scraper::Selector {
        scraper::Selector::parse(s).unwrap()
    }

    #[test]
    fn extracts_from_element_text() {
        let html = r#"<html><body><span class="price">R 1 299,00</span></body></html>"#;
        let out = extract(html, &sel("span.price"), None);
        assert_eq!(out.price_cents, Some(129_900));
        assert_eq!(out.error, None);
    }

    #[test]
    fn prefers_the_content_attribute_for_meta_markup() {
        let html = r#"<html><head><meta itemprop="price" content="1299.00"></head></html>"#;
        let out = extract(html, &sel(r#"meta[itemprop="price"]"#), None);
        assert_eq!(out.price_cents, Some(129_900));
    }

    #[test]
    fn reports_a_selector_that_matches_nothing() {
        let out = extract("<html><body></body></html>", &sel(".nope"), None);
        assert_eq!(out.price_cents, None);
        assert!(out.error.unwrap().contains("matched nothing"));
    }

    #[test]
    fn narrows_with_a_regex_capture_group() {
        let html = r#"<div id="p">Was R1 999,00 now R1 299,00</div>"#;
        let re = regex::Regex::new(r"now R([\d\s,]+)").unwrap();
        let out = extract(html, &sel("#p"), Some(&re));
        assert_eq!(out.price_cents, Some(129_900));
    }

    #[test]
    fn keeps_the_matched_text_when_parsing_fails() {
        let html = r#"<span class="price">Out of stock</span>"#;
        let out = extract(html, &sel("span.price"), None);
        assert_eq!(out.price_cents, None);
        assert_eq!(out.matched_text.as_deref(), Some("Out of stock"));
        assert!(out.error.is_some());
    }
}
