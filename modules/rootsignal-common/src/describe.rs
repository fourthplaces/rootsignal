use serde::{Deserialize, Serialize};

/// Visual building block for handler `describe` output.
///
/// Handlers return `Vec<Block>` — the admin flow UI renders them
/// inline on each handler node. The frontend has one renderer;
/// handlers compose from these shared primitives.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Block {
    Label {
        text: String,
    },
    Counter {
        label: String,
        value: u32,
        total: u32,
    },
    Progress {
        label: String,
        fraction: f32,
    },
    Checklist {
        label: String,
        items: Vec<ChecklistItem>,
    },
    KeyValue {
        key: String,
        value: String,
    },
    Status {
        label: String,
        state: State,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChecklistItem {
    pub text: String,
    pub done: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Waiting,
    Running,
    Done,
    Error,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_variants_round_trip_through_json() {
        let blocks = vec![
            Block::Label {
                text: "hello".into(),
            },
            Block::Counter {
                label: "x".into(),
                value: 3,
                total: 5,
            },
            Block::Progress {
                label: "p".into(),
                fraction: 0.5,
            },
            Block::Checklist {
                label: "c".into(),
                items: vec![
                    ChecklistItem {
                        text: "a".into(),
                        done: true,
                    },
                    ChecklistItem {
                        text: "b".into(),
                        done: false,
                    },
                ],
            },
            Block::KeyValue {
                key: "k".into(),
                value: "v".into(),
            },
            Block::Status {
                label: "s".into(),
                state: State::Running,
            },
        ];
        let json = serde_json::to_string(&blocks).unwrap();
        let parsed: Vec<Block> = serde_json::from_str(&json).unwrap();
        assert_eq!(blocks, parsed);
    }

    #[test]
    fn block_serializes_with_type_tag() {
        let block = Block::Counter {
            label: "done".into(),
            value: 2,
            total: 5,
        };
        let json: serde_json::Value = serde_json::to_value(&block).unwrap();
        assert_eq!(json["type"], "counter");
        assert_eq!(json["label"], "done");
        assert_eq!(json["value"], 2);
        assert_eq!(json["total"], 5);
    }

    #[test]
    fn state_serializes_as_snake_case() {
        assert_eq!(
            serde_json::to_value(State::Waiting).unwrap(),
            serde_json::json!("waiting")
        );
        assert_eq!(
            serde_json::to_value(State::Running).unwrap(),
            serde_json::json!("running")
        );
        assert_eq!(
            serde_json::to_value(State::Done).unwrap(),
            serde_json::json!("done")
        );
        assert_eq!(
            serde_json::to_value(State::Error).unwrap(),
            serde_json::json!("error")
        );
    }
}
