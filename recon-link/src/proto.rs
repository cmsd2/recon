use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LinkMessage {
    pub from: String,
    pub body: Value,
}

impl LinkMessage {
    pub fn new(from: String, body: Value) -> LinkMessage {
        LinkMessage {
            from: from,
            body: body,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MultiplexMessage {
    pub key: String,
    pub body: Value,
}

impl MultiplexMessage {
    pub fn new(key: String, body: Value) -> MultiplexMessage {
        MultiplexMessage {
            key: key,
            body: body,
        }
    }
}
