use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KanjiEntry {
    pub literal: char,
    pub stroke_count: u8,
    pub grade: Option<u8>,
    pub freq: Option<u16>,
    pub jlpt: Option<u8>,
    pub on_readings: Vec<String>,
    pub kun_readings: Vec<String>,
    pub meanings: Vec<String>,
}
