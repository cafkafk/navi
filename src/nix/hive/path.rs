use std::path::{Path, PathBuf};

use crate::error::NaviResult;
use crate::nix::Flake;

#[derive(Debug, Clone)]
pub enum HivePath {
    /// A Nix Flake.
    ///
    /// The flake must contain the `navi` output.
    Flake(Flake),

    /// A regular .nix file
    Legacy(PathBuf),
}

impl HivePath {
    pub async fn from_string(s: &str) -> NaviResult<Self> {
        // TODO: check for escaped colon
        let s = s.to_owned();
        let path = std::path::PathBuf::from(&s);

        if !path.exists() && s.contains(':') {
            // Treat as flake URI
            let flake = Flake::from_uri(&s).await?;

            tracing::info!("Using flake: {}", flake.uri());

            Ok(Self::Flake(flake))
        } else {
            HivePath::from_path(path).await
        }
    }

    pub async fn from_path<P: AsRef<Path>>(path: P) -> NaviResult<Self> {
        let path = path.as_ref();

        if let Some(osstr) = path.file_name() {
            if osstr == "flake.nix" {
                let parent = path.parent().unwrap();
                let flake = Flake::from_dir(parent).await?;
                return Ok(Self::Flake(flake));
            }
        }

        Ok(Self::Legacy(path.canonicalize()?))
    }

    pub fn is_flake(&self) -> bool {
        matches!(self, Self::Flake(_))
    }

    pub fn context_dir(&self) -> Option<PathBuf> {
        match self {
            Self::Legacy(p) => p.parent().map(|d| d.to_owned()),
            Self::Flake(flake) => flake.local_dir().map(|d| d.to_owned()),
        }
    }
}
