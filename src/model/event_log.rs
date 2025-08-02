use serde::Serialize;

#[derive(Serialize)]
pub struct EventLog {
    pub photo_id: i64,
    pub data: Option<Vec<u8>>,
}

#[derive(Serialize)]
pub struct EventLogs {
    pub event_log_id: i64,
    pub events: Vec<EventLog>,
}
