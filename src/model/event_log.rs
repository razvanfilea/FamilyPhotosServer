use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct EventLog {
    pub photo_id: i64,
    pub data: Option<Vec<u8>>,
}

#[derive(Debug, Serialize)]
pub struct EventLogs {
    pub event_log_id: i64,
    pub events: Vec<EventLog>,
}
