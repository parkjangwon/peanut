use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct DeliveryExtras {
    #[serde(default)]
    pub data: Option<Value>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub badge: Option<String>,
    #[serde(default)]
    pub priority: Option<String>,
}

impl DeliveryExtras {
    pub fn from_json(raw: Option<&str>) -> Option<Self> {
        raw.and_then(|value| serde_json::from_str(value).ok())
    }
}

#[derive(Debug, Clone)]
pub struct DeliveryMessage {
    pub title: String,
    pub body: String,
    pub extras: Option<DeliveryExtras>,
}
