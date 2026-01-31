use crate::repo::MonthSummary;
use serde::Serialize;

/// Timeline entry for JSON serialization (used by timeline scrollbar)
#[derive(Serialize)]
pub struct TimelineEntry {
    pub year_month: String,
    pub year: i32,
    pub count: i64,
    pub cumulative_before: i64,
    pub cover_photo_id: i64,
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
            // Derive year_month from max_created_at (e.g. "2024-03-15 10:30:00" -> "2024-03")
            let year_month = &s.max_created_at[..7];
            let year: i32 = year_month[..4].parse().unwrap_or(0);
            let entry = TimelineEntry {
                year_month: year_month.to_string(),
                year,
                count: s.count,
                cumulative_before: cumulative,
                cover_photo_id: s.cover_photo_id,
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
