//! Display helpers shared by the server, the tables and the chart.
//!
//! Dates are formatted with plain integer arithmetic rather than `chrono`, because these
//! run in the wasm bundle where pulling in a date library (and its `js` feature) to print
//! `2026-08-20` would be a poor trade. Everything here is UTC.

const MS_PER_DAY: i64 = 86_400_000;

/// Characters retailers use to group thousands. Always dropped, never a decimal point.
const GROUPERS: [char; 4] = [' ', '\u{00a0}', '\u{202f}', '\''];
/// Characters that may be either a decimal point or a thousands separator.
const AMBIGUOUS: [char; 2] = ['.', ','];

/// Renders cents as a human-readable amount, e.g. `(129900, "ZAR") -> "ZAR 1 299.00"`.
///
/// Shared so the table, the chart tooltips and the wishlist all agree on formatting.
pub fn format_cents(cents: i64, currency: &str) -> String {
    let negative = cents < 0;
    let abs = cents.unsigned_abs();
    let whole = abs / 100;
    let frac = abs % 100;

    // Thousands separators, inserted every three digits from the right.
    let digits = whole.to_string();
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            grouped.push('\u{202f}');
        }
        grouped.push(ch);
    }

    let sign = if negative { "-" } else { "" };
    format!("{currency} {sign}{grouped}.{frac:02}")
}

/// Renders a signed difference, e.g. `-R 40.00` shown as `"-ZAR 40.00"`.
pub fn format_delta(cents: i64, currency: &str) -> String {
    if cents > 0 {
        format!("+{}", format_cents(cents, currency))
    } else {
        format_cents(cents, currency)
    }
}

/// Splits epoch milliseconds into a UTC calendar date.
///
/// This is Howard Hinnant's `civil_from_days`, which is exact for the whole range we care
/// about and needs no lookup tables or leap-year special cases.
fn civil_from_millis(ms: i64) -> (i64, u32, u32) {
    let days = ms.div_euclid(MS_PER_DAY);

    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;

    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn time_from_millis(ms: i64) -> (u32, u32) {
    let ms_of_day = ms.rem_euclid(MS_PER_DAY);
    (
        (ms_of_day / 3_600_000) as u32,
        (ms_of_day / 60_000 % 60) as u32,
    )
}

/// `2026-08-20`
pub fn format_date(ms: i64) -> String {
    let (y, m, d) = civil_from_millis(ms);
    format!("{y:04}-{m:02}-{d:02}")
}

/// `2026-08-20 14:32`
pub fn format_datetime(ms: i64) -> String {
    let (hour, minute) = time_from_millis(ms);
    format!("{} {hour:02}:{minute:02}", format_date(ms))
}

/// Parses a human-written price into integer cents.
///
/// Handles the separator conventions that actually show up on retail sites:
/// `R 1 299,00`, `$1,299.00`, `EUR 1.299,00`, `1299`, `Now R899`.
///
/// The rule for telling a decimal point from a thousands separator: a space (or
/// apostrophe) is *always* grouping and must be followed by exactly three digits; a `.`
/// or `,` is a decimal point only when one or two digits follow it, because a group is
/// always three digits wide.
pub fn parse_price(input: &str) -> Option<i64> {
    let chars: Vec<char> = input.chars().collect();
    let start = chars.iter().position(|c| c.is_ascii_digit())?;

    // Walk the number, accepting separators only where they are unambiguous.
    let mut token = String::new();
    let mut i = start;
    while i < chars.len() && chars[i].is_ascii_digit() {
        token.push(chars[i]);
        i += 1;
    }

    while i < chars.len() {
        let separator = chars[i];
        let is_grouper = GROUPERS.contains(&separator);
        let is_ambiguous = AMBIGUOUS.contains(&separator);
        if !is_grouper && !is_ambiguous {
            break;
        }

        let digits: String = chars[i + 1..]
            .iter()
            .take_while(|c| c.is_ascii_digit())
            .collect();

        // A grouping separator must introduce exactly three digits; `.`/`,` may introduce
        // one to three. Anything else means the number ended before this character.
        let acceptable = if is_grouper {
            digits.len() == 3
        } else {
            (1..=3).contains(&digits.len())
        };
        if !acceptable {
            break;
        }

        if is_ambiguous {
            token.push(separator);
        }
        token.push_str(&digits);
        i += 1 + digits.len();
    }

    // Decide whether the final `.`/`,` was a decimal point.
    let (whole, frac) = match token.rfind(AMBIGUOUS) {
        Some(pos) => {
            let tail = &token[pos + 1..];
            if tail.len() <= 2 {
                (token[..pos].to_string(), tail.to_string())
            } else {
                (token.clone(), String::new())
            }
        }
        None => (token.clone(), String::new()),
    };

    let whole: String = whole.chars().filter(char::is_ascii_digit).collect();
    if whole.is_empty() && frac.is_empty() {
        return None;
    }

    let units: i64 = if whole.is_empty() {
        0
    } else {
        whole.parse().ok()?
    };
    let cents: i64 = match frac.len() {
        0 => 0,
        1 => frac.parse::<i64>().ok()? * 10,
        _ => frac.parse().ok()?,
    };

    units.checked_mul(100)?.checked_add(cents)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_retail_formats() {
        assert_eq!(parse_price("R 1 299,00"), Some(129_900));
        assert_eq!(parse_price("$1,299.00"), Some(129_900));
        assert_eq!(parse_price("\u{20ac}1.299,00"), Some(129_900));
        assert_eq!(parse_price("1299"), Some(129_900));
        assert_eq!(parse_price("Now R899"), Some(89_900));
        assert_eq!(parse_price("1,299"), Some(129_900));
        assert_eq!(parse_price("abc"), None);
    }
    #[test]
    fn treats_short_tails_as_decimals_and_triples_as_groups() {
        assert_eq!(parse_price("99.5"), Some(9_950), "one decimal digit");
        assert_eq!(parse_price("99.95"), Some(9_995), "two decimal digits");
        assert_eq!(
            parse_price("1.299"),
            Some(129_900),
            "three digits is a group"
        );
        assert_eq!(parse_price("12 345,67"), Some(1_234_567));
        assert_eq!(parse_price("1'299.00"), Some(129_900), "Swiss grouping");
    }
    #[test]
    fn stops_at_text_that_is_not_part_of_the_number() {
        // A space only continues the number when exactly three digits follow it,
        // so an unrelated trailing count is not swallowed.
        assert_eq!(parse_price("R899 2 left in stock"), Some(89_900));
        assert_eq!(parse_price("Save 15% - now R1 049,99 each"), Some(1_500));
        assert_eq!(parse_price("1299 ZAR"), Some(129_900));
    }
    #[test]
    fn handles_edges() {
        assert_eq!(parse_price(""), None);
        assert_eq!(parse_price("R"), None);
        assert_eq!(parse_price("0"), Some(0));
        assert_eq!(parse_price("0,00"), Some(0));
        assert_eq!(parse_price("1 000 000,00"), Some(100_000_000));
    }

    #[test]
    fn formats_cents_with_grouping() {
        assert_eq!(format_cents(129_900, "ZAR"), "ZAR 1\u{202f}299.00");
        assert_eq!(format_cents(899, "ZAR"), "ZAR 8.99");
        assert_eq!(format_cents(0, "USD"), "USD 0.00");
        assert_eq!(
            format_cents(100_000_000, "USD"),
            "USD 1\u{202f}000\u{202f}000.00"
        );
        assert_eq!(format_cents(-1050, "EUR"), "EUR -10.50");
    }

    #[test]
    fn formats_deltas_with_an_explicit_sign() {
        assert_eq!(format_delta(500, "ZAR"), "+ZAR 5.00");
        assert_eq!(format_delta(-500, "ZAR"), "ZAR -5.00");
    }

    #[test]
    fn converts_known_epochs_to_dates() {
        assert_eq!(format_date(0), "1970-01-01");
        assert_eq!(format_datetime(0), "1970-01-01 00:00");
        // 2026-08-20T14:32:00Z
        assert_eq!(format_datetime(1_787_236_320_000), "2026-08-20 14:32");
        // Leap day, which a naive implementation gets wrong.
        assert_eq!(format_date(1_709_164_800_000), "2024-02-29");
    }

    #[test]
    fn handles_dates_before_the_epoch() {
        assert_eq!(format_date(-1), "1969-12-31");
        assert_eq!(format_datetime(-1), "1969-12-31 23:59");
    }
}
