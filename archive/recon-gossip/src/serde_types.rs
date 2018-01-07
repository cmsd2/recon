use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LinkMessage {
    pub to: Option<String>,
    pub from: String,
    pub body: Value,
}

impl LinkMessage {
    pub fn new(to: Option<String>, from: String, body: Value) -> LinkMessage {
        LinkMessage {
            to: to,
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WatchdogMessage {
    Ping { seq: u32 },
    Pong { seq: u32 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum UpbMessage {
    Broadcast { from: String, seq: u64, value: Value, ttl: u32 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum LpbMessage {
    Data { seq: u64, value: Value },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum LpbLinkMessage {
    Data { from: String, seq: u64, value: Value },
    Request { from: String, msg_from: String, msg_seq: u64, ttl: u32 },
}
