use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Provenance {
    pub commit: String,
    pub flake_uri: String,
    pub timestamp: String,
    pub deployed_by: String,
}
