use crate::repo::MonthSummary;
use serde::Serialize;
use time::Month;

/// Timeline entry for JSON serialization (used by timeline scrollbar)
#[derive(Serialize)]
pub struct TimelineEntry {
    pub year: i32,
    pub month: u8,
    pub count: i64,
    pub cumulative_before: i64,
    pub label: String,
}

/// Result of building timeline data from month summaries
pub struct TimelineData {
    pub total_photos: i64,
    pub data_json: String,
}

/// Build timeline data with cumulative counts from month summaries
pub fn build_timeline_data(summaries: Vec<MonthSummary>) -> TimelineData {
    let total_photos: i64 = summaries.iter().map(|s| s.count).sum();
    let mut cumulative: i64 = 0;

    let entries: Vec<TimelineEntry> = summaries
        .into_iter()
        .map(|s| {
            let entry = TimelineEntry {
                year: s.year,
                month: s.month,
                count: s.count,
                cumulative_before: cumulative,
                label: format!(
                    "{} {}",
                    Month::try_from(s.month).map_or_else(|_| "?".to_string(), |m| m.to_string()),
                    s.year
                ),
            };
            cumulative += s.count;
            entry
        })
        .collect();

    let data_json = serde_json::to_string(&entries).unwrap_or_else(|_| "[]".to_string());

    TimelineData {
        total_photos,
        data_json,
    }
}
