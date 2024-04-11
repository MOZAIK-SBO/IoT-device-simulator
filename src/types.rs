use serde::Serialize;

pub type IngestBatch = Vec<IngestMetricEvent>;

#[derive(Serialize)]
pub struct IngestMetricEvent {
    // pub timestamp: Option<u128>,
    pub metric: String,
    pub value: CipherTextValue,
    pub source: Option<String>,
    // pub tags: Option<Vec<String>>,
    // pub location: Option<Location>,
    // pub elevation: Option<i32>,
}

#[derive(Serialize)]
pub struct GatewayIngestMetricEvent {
    pub timestamp: u128,
    pub metric: String,
    pub value: Vec<u8>,
    pub source: Option<String>,
    // pub tags: Option<Vec<String>>,
    // pub location: Option<Location>,
    // pub elevation: Option<i32>,
}

#[derive(Serialize)]
pub struct Location {
    pub lat: i32,
    pub lng: i32,
}

#[derive(Serialize)]
pub struct CipherTextValue {
    pub c: Vec<u8>,
}
