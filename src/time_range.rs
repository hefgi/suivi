use crate::{analytics, cli};

/// Resolve `--today/--week/--month/--since/--until/--all` into a pair of
/// RFC3339 bounds `(since, until)`.
///
/// - `since` is an inclusive lower bound on `started_at`.
/// - `until` is an **exclusive** upper bound. `--until 2026-06-21` means
///   "include all turns from local day 2026-06-21", which we encode as
///   `< midnight(2026-06-22 local)`; `query_turns` uses `<` so this stays
///   consistent across DST transitions via `analytics::day_start_in`.
/// - Both are `None` for `--all` and for the default case (the default
///   stats view manages its own windows internally).
///
/// clap enforces the structural constraints on `--until` (requires `--since`,
/// conflicts with `--today/--week/--month/--all`). This function only
/// validates date format and the `until >= since` ordering.
pub fn resolve_time_range(
    args: &cli::StatsArgs,
) -> Result<(Option<String>, Option<String>), anyhow::Error> {
    let now = chrono::Utc::now();

    if args.all {
        return Ok((None, None));
    }

    if let Some(date_str) = &args.since {
        let from = chrono::NaiveDate::parse_from_str(date_str, "%Y-%m-%d").map_err(|_| {
            anyhow::anyhow!("invalid --since date '{}' (expected YYYY-MM-DD)", date_str)
        })?;
        let since_rfc = analytics::local_date_start_rfc3339(from);

        let until_rfc = match &args.until {
            None => None,
            Some(until_str) => {
                let to =
                    chrono::NaiveDate::parse_from_str(until_str, "%Y-%m-%d").map_err(|_| {
                        anyhow::anyhow!(
                            "invalid --until date '{}' (expected YYYY-MM-DD)",
                            until_str
                        )
                    })?;
                if to < from {
                    return Err(anyhow::anyhow!(
                        "--until ({}) is before --since ({})",
                        until_str,
                        date_str
                    ));
                }
                // Inclusive intent → exclusive SQL bound at the next local midnight.
                to.succ_opt().and_then(analytics::local_date_start_rfc3339)
            }
        };
        return Ok((since_rfc, until_rfc));
    }

    if args.today {
        return Ok((analytics::local_today_start_rfc3339(), None));
    }
    if args.week {
        return Ok((Some((now - chrono::Duration::days(7)).to_rfc3339()), None));
    }
    if args.month {
        return Ok((Some((now - chrono::Duration::days(30)).to_rfc3339()), None));
    }
    Ok((None, None))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{OutputFormat, StatsArgs};

    fn args() -> StatsArgs {
        StatsArgs {
            all: false,
            today: false,
            week: false,
            month: false,
            since: None,
            until: None,
            project: None,
            agent: None,
            projects: false,
            history: false,
            graph: false,
            daily: false,
            format: OutputFormat::Text,
        }
    }

    #[test]
    fn test_all_returns_none_none() {
        let mut a = args();
        a.all = true;
        let (since, until) = resolve_time_range(&a).unwrap();
        assert_eq!(since, None);
        assert_eq!(until, None);
    }

    #[test]
    fn test_default_returns_none_none() {
        let (since, until) = resolve_time_range(&args()).unwrap();
        assert_eq!(since, None);
        assert_eq!(until, None);
    }

    #[test]
    fn test_today_sets_since_only() {
        let mut a = args();
        a.today = true;
        let (since, until) = resolve_time_range(&a).unwrap();
        assert!(since.is_some());
        assert_eq!(until, None);
    }

    #[test]
    fn test_week_and_month_set_since_only() {
        let mut a = args();
        a.week = true;
        let (s, u) = resolve_time_range(&a).unwrap();
        assert!(s.is_some());
        assert_eq!(u, None);

        let mut a = args();
        a.month = true;
        let (s, u) = resolve_time_range(&a).unwrap();
        assert!(s.is_some());
        assert_eq!(u, None);
    }

    #[test]
    fn test_since_without_until() {
        let mut a = args();
        a.since = Some("2026-06-15".to_string());
        let (s, u) = resolve_time_range(&a).unwrap();
        assert!(s.is_some());
        assert_eq!(u, None);
    }

    #[test]
    fn test_since_with_until_returns_both() {
        let mut a = args();
        a.since = Some("2026-06-15".to_string());
        a.until = Some("2026-06-21".to_string());
        let (s, u) = resolve_time_range(&a).unwrap();
        assert!(s.is_some(), "since should be set");
        assert!(u.is_some(), "until should be set");
        // until is encoded as next-day midnight; sanity-check that the
        // resolved upper bound is strictly greater than the lower bound.
        assert!(u.as_ref().unwrap() > s.as_ref().unwrap());
    }

    #[test]
    fn test_until_before_since_errors() {
        let mut a = args();
        a.since = Some("2026-06-21".to_string());
        a.until = Some("2026-06-15".to_string());
        let err = resolve_time_range(&a).unwrap_err();
        assert!(err.to_string().contains("before --since"), "got: {}", err);
    }

    #[test]
    fn test_invalid_since_errors() {
        let mut a = args();
        a.since = Some("not-a-date".to_string());
        let err = resolve_time_range(&a).unwrap_err();
        assert!(err.to_string().contains("invalid --since"));
    }

    #[test]
    fn test_invalid_until_errors() {
        let mut a = args();
        a.since = Some("2026-06-15".to_string());
        a.until = Some("bogus".to_string());
        let err = resolve_time_range(&a).unwrap_err();
        assert!(err.to_string().contains("invalid --until"));
    }
}
