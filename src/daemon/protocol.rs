use crate::command::tui::model::{DeploySettings, ProvenanceStatus};
use crate::nix::{NodeName, NodeState};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Request {
    GetState,
    Deploy {
        nodes: Vec<NodeName>,
        settings: DeploySettings,
        parallel: usize,
    },
    Diff {
        node: NodeName,
    },
    GarbageCollect {
        nodes: Vec<NodeName>,
        interval: Option<String>,
    },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Response {
    State(DaemonStateSnapshot),
    Event(DaemonEvent),
    Ok,
    Error(String),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum DaemonEvent {
    TaskStarted(Uuid, String),
    TaskFinished(Uuid),
    NodeStateChanged(NodeName, NodeState),
    Log(String),
    NodeLog(NodeName, String),
    DiffComputed(String),
    ProvenanceLoaded(NodeName, ProvenanceStatus),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DaemonStateSnapshot {
    pub node_states: HashMap<NodeName, NodeState>,
    pub active_tasks: HashMap<Uuid, String>,
    pub logs: Vec<String>,
}
