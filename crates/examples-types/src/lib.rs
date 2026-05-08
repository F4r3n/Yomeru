use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExampleEntry {
    pub japanese: String,
    pub english: String,
}
