use serde::Serialize;

#[derive(Serialize)]
pub struct IngestMetricEvent {
    pub timestamp: Option<i32>,
    pub metric: String,
    pub value: Vec<u8>,
    pub source: Option<String>,
    pub tags: Option<Vec<String>>,
    pub location: Option<Location>,
    pub elevation: Option<i32>,
}

#[derive(Serialize)]
pub struct Location {
    pub lat: i32,
    pub lng: i32,
}
