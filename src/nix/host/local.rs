use std::collections::HashMap;
use std::convert::TryInto;
use std::process::Stdio;

use async_trait::async_trait;
use tokio::process::Command;

use super::{key_uploader, CopyDirection, CopyOptions, Host};
use crate::error::{NaviError, NaviResult};
use crate::job::JobHandle;
use crate::nix::{
    Goal, Key, NixFlags, Profile, Provenance, StorePath, CURRENT_PROFILE, SYSTEM_PROFILE,
};
use crate::util::{CommandExecution, CommandExt};

/// The local machine running Navi.
///
/// It may not be capable of realizing some derivations
/// (e.g., building Linux derivations on macOS).
#[derive(Debug)]
pub struct Local {
    job: Option<JobHandle>,
    nix_options: NixFlags,
    privilege_escalation_command: Option<Vec<String>>,
}

impl Local {
    pub fn new(nix_options: NixFlags) -> Self {
        Self {
            job: None,
            nix_options,
            privilege_escalation_command: None,
        }
    }
}

#[async_trait]
impl Host for Local {
    async fn copy_closure(
        &mut self,
        _closure: &StorePath,
        _direction: CopyDirection,
        _options: CopyOptions,
    ) -> NaviResult<()> {
        Ok(())
    }

    async fn realize_remote(&mut self, derivation: &StorePath) -> NaviResult<Vec<StorePath>> {
        let mut command = Command::new("nix-store");

        command.args(self.nix_options.to_nix_store_args());
        command
            .arg("--no-gc-warning")
            .arg("--realise")
            .arg(derivation.as_path());

        let mut execution = CommandExecution::new(command);

        execution.set_job(self.job.clone());

        execution.run().await?;
        let (stdout, _) = execution.get_logs();

        stdout
            .unwrap()
            .lines()
            .map(|p| p.to_string().try_into())
            .collect()
    }

    async fn upload_keys(
        &mut self,
        keys: &HashMap<String, Key>,
        require_ownership: bool,
    ) -> NaviResult<()> {
        for (name, key) in keys {
            self.upload_key(name, key, require_ownership).await?;
        }

        Ok(())
    }

    async fn write_provenance(&mut self, provenance: &Provenance) -> NaviResult<()> {
        let json = serde_json::to_string(provenance).unwrap();

        // Use CommandExecution for mkdir to ensure output is captured
        let mut mkdir = CommandExecution::new(self.make_privileged_command(&[
            "mkdir",
            "-p",
            "-m",
            "0755",
            "/etc/navi",
        ]));
        mkdir.set_job(self.job.clone());
        mkdir.run().await?;

        // Manual spawning for cat/tee to handle stdin, but we MUST capture stdout/stderr manually
        let mut cmd =
            self.make_privileged_command(&["sh", "-c", "cat > /etc/navi/provenance.json"]);
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let mut child = cmd.spawn()?;

        // Capture logs manually using util helper
        if let Some(stdout) = child.stdout.take() {
            let reader = tokio::io::BufReader::new(stdout);
            let job = self.job.clone();
            tokio::spawn(async move {
                let _ = crate::util::capture_stream(reader, job, false, false).await;
            });
        }
        if let Some(stderr) = child.stderr.take() {
            let reader = tokio::io::BufReader::new(stderr);
            let job = self.job.clone();
            tokio::spawn(async move {
                let _ = crate::util::capture_stream(reader, job, true, false).await;
            });
        }

        if let Some(mut stdin) = child.stdin.take() {
            use tokio::io::AsyncWriteExt;
            stdin.write_all(json.as_bytes()).await?;
        }

        let status = child.wait().await?;
        if !status.success() {
            return Err(status.into());
        }
        Ok(())
    }

    async fn fetch_provenance(&mut self) -> NaviResult<Option<Provenance>> {
        let output = Command::new("cat")
            .arg("/etc/navi/provenance.json")
            .capture_output()
            .await;

        match output {
            Ok(json) => match serde_json::from_str::<Provenance>(&json) {
                Ok(p) => Ok(Some(p)),
                Err(_) => Ok(None),
            },
            Err(_) => Ok(None),
        }
    }

    async fn activate(
        &mut self,
        profile: &Profile,
        goal: Goal,
        install_bootloader: bool,
    ) -> NaviResult<()> {
        if !goal.requires_activation() {
            return Err(NaviError::Unsupported);
        }

        if goal.should_switch_profile() {
            let path = profile.as_path().to_str().unwrap();
            // Switched to CommandExecution to capture output
            let mut cmd = CommandExecution::new(self.make_privileged_command(&[
                "nix-env",
                "--profile",
                SYSTEM_PROFILE,
                "--set",
                path,
            ]));
            cmd.set_job(self.job.clone());
            cmd.run().await?;
        }

        let command = {
            let activation_command = profile.activation_command(goal).unwrap();
            let mut cmd_vec = activation_command.clone();

            if install_bootloader {
                cmd_vec.insert(0, "NIXOS_INSTALL_BOOTLOADER=1".to_string());
                cmd_vec.insert(0, "env".to_string());
            }

            self.make_privileged_command(&cmd_vec)
        };

        let mut execution = CommandExecution::new(command);

        execution.set_job(self.job.clone());

        execution.run().await
    }

    async fn get_current_system_profile(&mut self) -> NaviResult<Profile> {
        let paths = Command::new("readlink")
            .args(["-e", CURRENT_PROFILE])
            .capture_output()
            .await?;

        let path = paths
            .lines()
            .next()
            .ok_or(NaviError::FailedToGetCurrentProfile)?
            .to_string()
            .try_into()?;

        Ok(Profile::from_store_path_unchecked(path))
    }

    async fn get_main_system_profile(&mut self) -> NaviResult<Profile> {
        let paths = Command::new("sh")
            .args([
                "-c",
                &format!(
                    "readlink -e {} || readlink -e {}",
                    SYSTEM_PROFILE, CURRENT_PROFILE
                ),
            ])
            .capture_output()
            .await?;

        let path = paths
            .lines()
            .next()
            .ok_or(NaviError::FailedToGetCurrentProfile)?
            .to_string()
            .try_into()?;

        Ok(Profile::from_store_path_unchecked(path))
    }

    fn set_job(&mut self, job: Option<JobHandle>) {
        self.job = job;
    }
}

impl Local {
    pub fn set_privilege_escalation_command(&mut self, command: Option<Vec<String>>) {
        self.privilege_escalation_command = command;
    }

    pub fn upcast(self) -> Box<dyn Host> {
        Box::new(self)
    }

    /// "Uploads" a single key.
    async fn upload_key(
        &mut self,
        name: &str,
        key: &Key,
        require_ownership: bool,
    ) -> NaviResult<()> {
        if let Some(job) = &self.job {
            job.message(format!("Deploying key {}", name))?;
        }

        let path = key.path();
        let key_script = format!(
            "'{}'",
            key_uploader::generate_script(key, path, require_ownership)
        );

        let mut command = self.make_privileged_command(&["sh", "-c", &key_script]);
        command.stdin(Stdio::piped());
        command.stderr(Stdio::piped());
        command.stdout(Stdio::piped());

        let uploader = command.spawn()?;
        key_uploader::feed_uploader(uploader, key, self.job.clone()).await
    }

    /// Constructs a command with privilege escalation.
    fn make_privileged_command<S: AsRef<str>>(&self, command: &[S]) -> Command {
        let mut full_command = Vec::new();
        if let Some(esc) = &self.privilege_escalation_command {
            full_command.extend(esc.iter().map(|s| s.as_str()));
        }
        full_command.extend(command.iter().map(|s| s.as_ref()));

        let mut result = Command::new(full_command[0]);
        if full_command.len() > 1 {
            result.args(&full_command[1..]);
        }

        result
    }
}
