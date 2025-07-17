use serde::Serialize;

pub struct EventLog {
    pub event_id: i64,
    pub photo_id: i64,
    pub data: Option<Vec<u8>>,
}

#[derive(Serialize)]
pub struct EventLogNetwork {
    pub photo_id: i64,
    pub data: Option<Vec<u8>>,
}
