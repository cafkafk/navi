use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum NodeState {
    Idle,
    Loading,
    Running(String),
    Success(String),
    Failed(String),
}

impl NodeState {
    pub fn as_str(&self) -> &str {
        match self {
            NodeState::Idle => "",
            NodeState::Loading => "Loading...",
            NodeState::Running(s) => s,
            NodeState::Success(s) => s,
            NodeState::Failed(s) => s,
        }
    }
}
