use super::model::ProvenanceStatus;
use crate::nix::{NodeConfig, NodeName, NodeState};
use crate::progress::Message;
use crate::registrants::porkbun::{PorkbunDnsRecord, PorkbunGlueRecord};
use crate::registrants::DomainInfo;
use uuid::Uuid;

pub enum AppEvent {
    Progress(Message),
    ConfigLoaded(NodeName, NodeConfig),
    ProvenanceLoaded(NodeName, ProvenanceStatus),
    RegistrantsLoaded(Vec<DomainInfo>, Vec<String>),
    TreeRebuild,
    DiffComputed(String),
    Ssh(NodeName),
    RamUpdate(u64, u64),
    // GarbageCollect(Vec<NodeName>, Option<String>), // Unused now (direct fn call)
    FetchRegistrantRecords(String, String, String, String), // Provider, Account, Domain, Type (DNS/Glue)
    RegistrantRecordsLoaded {
        domain: String,
        record_type: String, // "DNS" or "Glue"
        dns_records: Option<Vec<PorkbunDnsRecord>>,
        glue_records: Option<Vec<PorkbunGlueRecord>>,
        error: Option<String>,
    },
    TaskStarted(Uuid, String),
    TaskFinished(Uuid),
    NodeStateChanged(NodeName, NodeState),
    MetaLoaded(crate::nix::MetaConfig),
    SaveCache,
}
