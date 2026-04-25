use std::collections::HashMap;
use std::convert::TryInto;
use std::ffi::OsStr;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use async_trait::async_trait;
use tokio::process::Command;
use tokio::time::sleep;

use super::{key_uploader, CopyDirection, CopyOptions, Host, RebootOptions};
use crate::error::{NaviError, NaviResult};
use crate::job::JobHandle;
use crate::nix::{
    DiskUnlockConfig, Goal, Key, Profile, Provenance, StorePath, CURRENT_PROFILE, SYSTEM_PROFILE,
};
use crate::util::{CommandExecution, CommandExt};

/// A remote machine connected over SSH.
#[derive(Debug, Clone)]
pub struct Ssh {
    /// The username to use to connect.
    user: Option<String>,

    /// The hostname or IP address to connect to.
    host: String,

    /// The port to connect to.
    port: Option<u16>,

    /// Local path to a ssh_config file.
    ssh_config: Option<PathBuf>,

    /// Command to elevate privileges with.
    privilege_escalation_command: Vec<String>,

    /// extra SSH options
    extra_ssh_options: Vec<String>,

    /// Whether to use the experimental `nix copy` command.
    use_nix3_copy: bool,

    provider: Provider,

    /// Force connections through physical interfaces, bypassing overlay networks.
    force_hw_link: bool,

    /// Explicit list of allowed interfaces for hw-link binding.
    hw_link_interfaces: Option<Vec<String>>,

    job: Option<JobHandle>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Provider {
    Ssh,
    Gcp {
        project: Option<String>,
        zone: Option<String>,
        iap: bool,
    },
    // Future: Aws, Oci, etc.
}

/// An opaque boot ID.
#[derive(Debug, Clone, PartialEq, Eq)]
struct BootId(String);

#[async_trait]
impl Host for Ssh {
    async fn copy_closure(
        &mut self,
        closure: &StorePath,
        direction: CopyDirection,
        options: CopyOptions,
    ) -> NaviResult<()> {
        let command = self.nix_copy_closure(closure, direction, options);
        self.run_command(command).await
    }

    async fn write_provenance(&mut self, provenance: &Provenance) -> NaviResult<()> {
        let json = serde_json::to_string(provenance).unwrap();
        // Ensure directory exists with readable permissions
        let mkdir = self.ssh(&["mkdir", "-p", "-m", "0755", "/etc/navi"]);
        self.run_command(mkdir).await?;

        // Write content using 'tee' to avoid shell redirection privilege issues
        let mut cmd = self.ssh(&["tee", "/etc/navi/provenance.json"]);
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let mut child = cmd.spawn()?;

        // Capture logs
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

    async fn connect_serial(&mut self) -> NaviResult<()> {
        match &self.provider {
            Provider::Gcp { project, zone, .. } => {
                let mut cmd = Command::new("gcloud");
                cmd.args(["compute", "connect-to-serial-port", &self.host]);

                if let Some(p) = project {
                    cmd.arg(format!("--project={}", p));
                }
                if let Some(z) = zone {
                    cmd.arg(format!("--zone={}", z));
                }

                tracing::info!("Connecting to serial console via gcloud...");

                let status = cmd
                    .status()
                    .await
                    .map_err(|e| NaviError::IoError { error: e })?;

                if !status.success() {
                    return Err(status.into());
                }
                Ok(())
            }
            _ => Err(NaviError::Unsupported),
        }
    }

    async fn fetch_provenance(&mut self) -> NaviResult<Option<Provenance>> {
        match &self.provider {
            Provider::Gcp { .. } => {
                // For GCP, we need to inject ConnectTimeout.
                // We'll clone self to avoid mutating the original host state
                let mut temp_host = self.clone();
                temp_host.extra_ssh_options.push("-o".to_string());
                temp_host
                    .extra_ssh_options
                    .push("ConnectTimeout=5".to_string());

                let cmd = temp_host.ssh(&["cat", "/etc/navi/provenance.json"]);
                let mut execution = CommandExecution::new(cmd);
                execution.set_quiet(true);

                match execution.capture_output().await {
                    Ok(json) => match serde_json::from_str::<Provenance>(&json) {
                        Ok(p) => Ok(Some(p)),
                        Err(_) => Ok(None),
                    },
                    Err(NaviError::ChildFailure { exit_code: 1, .. }) => Ok(None),
                    Err(e) => Err(e),
                }
            }
            Provider::Ssh => {
                // Manually construct SSH command to inject ConnectTimeout option correctly
                let mut options = self.ssh_options();
                options.push("-o".to_string());
                options.push("ConnectTimeout=5".to_string());

                let mut cmd = Command::new("ssh");
                cmd.args(&options)
                    .arg(self.ssh_target())
                    .arg("--")
                    .arg("cat")
                    .arg("/etc/navi/provenance.json");

                let mut execution = CommandExecution::new(cmd);
                execution.set_quiet(true);

                match execution.capture_output().await {
                    Ok(json) => {
                        match serde_json::from_str::<Provenance>(&json) {
                            Ok(p) => Ok(Some(p)),
                            Err(_) => Ok(None), // parsing failed
                        }
                    }
                    Err(NaviError::ChildFailure { exit_code: 1, .. }) => Ok(None),
                    Err(e) => Err(e),
                }
            }
        }
    }

    async fn realize_remote(&mut self, derivation: &StorePath) -> NaviResult<Vec<StorePath>> {
        let command = self.ssh(&[
            "nix-store",
            "--no-gc-warning",
            "--realise",
            derivation.as_path().to_str().unwrap(),
        ]);

        let mut execution = CommandExecution::new(command);
        execution.set_job(self.job.clone());

        let paths = execution.capture_output().await?;

        paths.lines().map(|p| p.to_string().try_into()).collect()
    }

    fn set_job(&mut self, job: Option<JobHandle>) {
        self.job = job;
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
            let set_profile = self.ssh(&["nix-env", "--profile", SYSTEM_PROFILE, "--set", path]);
            self.run_command(set_profile).await?;
        }

        let activation_command = profile.activation_command(goal).unwrap();
        let mut v: Vec<&str> = activation_command.iter().map(|s| &**s).collect();

        if install_bootloader {
            v.insert(0, "NIXOS_INSTALL_BOOTLOADER=1");
            v.insert(0, "env");
        }

        let command = self.ssh(&v);
        self.run_command(command).await
    }

    async fn get_current_system_profile(&mut self) -> NaviResult<Profile> {
        let paths = self
            .ssh(&["readlink", "-e", CURRENT_PROFILE])
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
        let command = format!(
            "\"readlink -e {} || readlink -e {}\"",
            SYSTEM_PROFILE, CURRENT_PROFILE
        );

        let paths = self.ssh(&["sh", "-c", &command]).capture_output().await?;

        let path = paths
            .lines()
            .next()
            .ok_or(NaviError::FailedToGetCurrentProfile)?
            .to_string()
            .try_into()?;

        Ok(Profile::from_store_path_unchecked(path))
    }

    async fn run_command(&mut self, command: &[&str]) -> NaviResult<()> {
        let command = self.ssh(command);
        self.run_command(command).await
    }

    /// Reboots the host.
    async fn reboot(&mut self, options: RebootOptions) -> NaviResult<()> {
        if !options.wait_for_boot {
            return self.initate_reboot().await;
        }

        let old_id = self.get_boot_id().await?;

        self.initate_reboot().await?;

        if let Some(job) = &self.job {
            job.message("Waiting for reboot".to_string())?;
        }

        let mut unlocked = false;

        // Wait for node to come back up
        loop {
            // Attempt to unlock if configured and not yet unlocked
            if let Some(unlock_config) = &options.unlock {
                if unlock_config.enable && !unlocked {
                    // Create an initrd host configuration
                    let mut initrd_host = self.clone();

                    initrd_host.configure_for_initrd(unlock_config);

                    // Set aggressive timeout for initrd probe since it might not be up yet
                    initrd_host.extra_ssh_options.push("-o".to_string());
                    initrd_host
                        .extra_ssh_options
                        .push("ConnectTimeout=2".to_string());

                    initrd_host.extra_ssh_options.push("-o".to_string());
                    initrd_host
                        .extra_ssh_options
                        .push("ConnectionAttempts=1".to_string());

                    // Try to unlock
                    match initrd_host.unlock_disk(unlock_config).await {
                        Ok(_) => {
                            unlocked = true;
                            if let Some(job) = &self.job {
                                job.message(
                                    "Disk unlocked successfully. Waiting for boot...".to_string(),
                                )?;
                            }
                        }
                        Err(e) => {
                            // Ignore errors, initrd might not be up yet or connection refused
                            // But update status so user knows what's happening
                            if let Some(job) = &self.job {
                                job.message(format!("Waiting for initrd ({})...", e))?;
                            }
                            tracing::debug!("Unlock attempt failed: {}", e);
                        }
                    }
                }
            }

            // check if main system is back online
            if let Ok(new_id) = self.get_boot_id().await {
                if new_id != old_id {
                    break;
                }
            }

            sleep(Duration::from_secs(2)).await;
        }

        // Ensure node has correct system profile
        if let Some(new_profile) = options.new_profile {
            let profile = self.get_current_system_profile().await?;

            if new_profile != profile {
                return Err(NaviError::ActiveProfileUnexpected { profile });
            }
        }

        Ok(())
    }
}

impl Ssh {
    pub fn new(user: Option<String>, host: String) -> Self {
        Self {
            user,
            host,
            port: None,
            ssh_config: None,
            privilege_escalation_command: Vec::new(),
            extra_ssh_options: Vec::new(),
            use_nix3_copy: false,
            provider: Provider::Ssh,
            force_hw_link: false,
            hw_link_interfaces: None,
            job: None,
        }
    }

    pub fn set_provider(&mut self, provider: Provider) {
        self.provider = provider;
    }

    pub fn set_port(&mut self, port: u16) {
        self.port = Some(port);
    }

    pub fn set_override_address(&mut self, addr: String) {
        self.host = addr;
    }

    pub fn set_user(&mut self, user: String) {
        self.user = Some(user);
    }

    pub fn set_ssh_config(&mut self, ssh_config: PathBuf) {
        self.ssh_config = Some(ssh_config);
    }

    pub fn set_privilege_escalation_command(&mut self, command: Vec<String>) {
        self.privilege_escalation_command = command;
    }

    pub fn set_extra_ssh_options(&mut self, options: Vec<String>) {
        self.extra_ssh_options = options;
    }

    pub fn set_use_nix3_copy(&mut self, enable: bool) {
        self.use_nix3_copy = enable;
    }

    pub fn set_force_hw_link(&mut self, enable: bool) {
        self.force_hw_link = enable;
    }

    pub fn set_hw_link_interfaces(&mut self, interfaces: Option<Vec<String>>) {
        self.hw_link_interfaces = interfaces;
    }

    pub fn upcast(self) -> Box<dyn Host> {
        Box::new(self)
    }

    /// Applies hardware-link interface binding to this SSH host's options.
    ///
    /// Detects an appropriate physical interface (e.g. `enp*`, `wlp*`) and adds
    /// a `ProxyCommand` using `socat` with `bindtodevice` to bypass overlay
    /// networks like Tailscale.
    ///
    /// If `explicit_interfaces` is provided, only those interface prefixes are
    /// allowed. Otherwise, the default prefixes `enp` and `wlp` are used.
    pub fn apply_hw_link_binding(&mut self, explicit_interfaces: Option<&[String]>) {
        let target_host = self.host.clone();

        let allowed_patterns: Vec<String> = if let Some(explicit) = explicit_interfaces {
            explicit.to_vec()
        } else {
            vec!["enp".to_string(), "wlp".to_string()]
        };

        let matches =
            |iface: &str| -> bool { allowed_patterns.iter().any(|p| iface.starts_with(p)) };

        // Try route lookup
        let best_interface = std::process::Command::new("ip")
            .args(&["route", "get", &target_host, "table", "main"])
            .output()
            .ok()
            .and_then(|output| {
                let s = String::from_utf8_lossy(&output.stdout);
                s.split_whitespace()
                    .skip_while(|&part| part != "dev")
                    .nth(1)
                    .map(|s| s.to_string())
            });

        let selected_interface = if let Some(iface) = best_interface.filter(|i| matches(i)) {
            Some(iface)
        } else {
            tracing::warn!("Route lookup failed or returned disallowed interface. Scanning available interfaces...");
            std::process::Command::new("ip")
                .args(&["-o", "link", "show", "up"])
                .output()
                .ok()
                .and_then(|output| {
                    String::from_utf8_lossy(&output.stdout)
                        .lines()
                        .find_map(|line| {
                            let parts: Vec<&str> = line.split_whitespace().collect();
                            if parts.len() >= 2 {
                                let name = parts[1].trim_end_matches(':');
                                if matches(name) {
                                    return Some(name.to_string());
                                }
                            }
                            None
                        })
                })
        };

        if let Some(iface) = selected_interface {
            tracing::info!("Binding connection to physical interface: {}", iface);
            self.extra_ssh_options.push("-o".to_string());
            self.extra_ssh_options.push(format!(
                "ProxyCommand=sudo socat - TCP:%h:%p,bindtodevice={}",
                iface
            ));
        } else {
            tracing::error!(
                "Failed to find allowed physical interface! Blocking connection."
            );
            self.extra_ssh_options.push("-o".to_string());
            self.extra_ssh_options
                .push("ProxyCommand=false".to_string());
        }
    }

    /// Prepares this host instance for connecting to initrd (e.g. for unlocking).
    pub fn configure_for_initrd(&mut self, config: &DiskUnlockConfig) {
        // Override connection details for initrd
        if let Some(h) = &config.host {
            self.set_override_address(h.clone());
        }
        self.set_port(config.port);
        if let Some(u) = &config.user {
            self.set_user(u.clone());
        }

        // Reset options to avoid inheriting config from main host
        self.extra_ssh_options.clear();

        // Handle interface binding
        if config.force_hw_link || config.interfaces.is_some() {
            self.apply_hw_link_binding(config.interfaces.as_deref());
        }

        self.extra_ssh_options.extend(config.ssh_options.clone());

        if config.ignore_ssh_config {
            self.set_ssh_config(PathBuf::from("/dev/null"));
        }

        if config.ignore_host_key_check {
            // Ignore host key checking for initrd
            // Note: StrictHostKeyChecking=no might be overridden by accept-new in options(),
            // but UserKnownHostsFile=/dev/null ensures we accept the "new" key regardless.
            self.extra_ssh_options.push("-o".to_string());
            self.extra_ssh_options
                .push("UserKnownHostsFile=/dev/null".to_string());
            self.extra_ssh_options.push("-o".to_string());
            self.extra_ssh_options
                .push("StrictHostKeyChecking=no".to_string());
        }
    }

    /// Returns a Tokio Command to run an arbitrary command on the host.
    pub fn ssh(&self, command: &[&str]) -> Command {
        let options = self.ssh_options();
        let options_str = options.join(" ");
        let privilege_escalation_command = if self.user.as_deref() != Some("root") {
            self.privilege_escalation_command.as_slice()
        } else {
            &[]
        };

        // TODO: remove this when confirmed working
        tracing::debug!("SSH options: {:?}", options);

        let mut cmd = Command::new("ssh");

        cmd.args(&options)
            .arg(self.ssh_target())
            .arg("--")
            .args(privilege_escalation_command)
            .args(command)
            .env("NIX_SSHOPTS", options_str);

        cmd
    }

    /// Unlocks the disk.
    pub async fn unlock_disk(&mut self, config: &super::DiskUnlockConfig) -> NaviResult<()> {
        if !config.enable {
            return Ok(());
        }

        use tokio::io::AsyncWriteExt;

        let password = if let Some(cmd_str) = &config.password_command {
            if let Some(job) = &self.job {
                job.message(format!("Retrieving password using: {}", cmd_str))?;
            }

            let output = tokio::process::Command::new("sh")
                .arg("-c")
                .arg(cmd_str)
                .output()
                .await
                .map_err(|e| NaviError::Unknown {
                    message: format!("Failed to run password command: {}", e),
                })?;

            if !output.status.success() {
                return Err(NaviError::Unknown {
                    message: format!(
                        "Password command failed: {}",
                        String::from_utf8_lossy(&output.stderr)
                    ),
                });
            }

            Some(output.stdout)
        } else {
            None
        };

        if let Some(job) = &self.job {
            job.message("Unlocking disk...".to_string())?;
        }

        // Construct raw SSH command for initrd
        let options = self.ssh_options();

        // Log the command for debugging purposes (especially for verification of IAP usage)
        tracing::info!(
            "Exec: \"ssh\" {}",
            options
                .iter()
                .map(|o| format!("\"{}\"", o))
                .collect::<Vec<_>>()
                .join(" ")
        );

        let mut cmd = Command::new("ssh");
        cmd.args(&options)
            .arg(self.ssh_target())
            .arg("--")
            .arg(&config.remote_command);

        if password.is_some() {
            cmd.stdin(Stdio::piped());
        }

        // Capture output for error reporting
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| NaviError::Unknown {
            message: format!("Failed to spawn SSH process: {}", e),
        })?;

        if let Some(pass_bytes) = password {
            if let Some(mut stdin) = child.stdin.take() {
                // Determine if we need to write the password
                // Some unlocking commands read from stdin (zfs load-key), some don't.
                // But generally piping it doesn't hurt if not consumed, unless the buffer fills.
                // 'zfs load-key' reads from stdin if -L prompt is used or implicit.
                // The provided default 'zfs load-key -a' attempts to load all keys.
                // If keys are 'prompt', it reads from stdin.

                // We write the password and close stdin immediately
                if let Err(e) = stdin.write_all(&pass_bytes).await {
                    tracing::warn!(
                        "Failed to write password to stdin (remote might not need it?): {}",
                        e
                    );
                }
            }
        }

        let output = child
            .wait_with_output()
            .await
            .map_err(|e| NaviError::Unknown {
                message: format!("Failed to wait for SSH process: {}", e),
            })?;

        if output.status.success() {
            if let Some(job) = &self.job {
                job.message("Disk unlock command completed successfully.".to_string())?;
            }
            Ok(())
        } else {
            // If the command failed, it might be due to `killall` failing.
            if config.remote_command.contains("killall") {
                tracing::warn!("The remote command exited with an error. If 'zfs load-key' succeeded, this error might be due to 'killall' failing to find a process, which is harmless.");
            }

            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::error!("SSH stderr: {}", stderr);

            Err(NaviError::Unknown {
                message: format!(
                    "Remote command exited with status: {}. Stderr: {}",
                    output.status,
                    stderr.trim()
                ),
            })
        }
    }

    async fn run_command(&mut self, command: Command) -> NaviResult<()> {
        let mut execution = CommandExecution::new(command);
        execution.set_job(self.job.clone());

        execution.run().await
    }

    fn ssh_target(&self) -> String {
        match &self.user {
            Some(n) => format!("{}@{}", n, self.host),
            None => self.host.clone(),
        }
    }

    fn nix_copy_closure(
        &self,
        path: &StorePath,
        direction: CopyDirection,
        options: CopyOptions,
    ) -> Command {
        let ssh_options = self.ssh_options();
        // We need to quote options because they are passed via NIX_SSHOPTS env var,
        // which might be split by shell or nix's tokenizer.
        let ssh_options_str = ssh_options
            .iter()
            .map(|s| shell_quote(s))
            .collect::<Vec<_>>()
            .join(" ");

        // We use nix3_copy significantly for ProxyCommand support because `nix` (binary)
        // parses NIX_SSHOPTS properly with quotes, whereas `nix-copy-closure` (script)
        // is very fragile with spaces in env vars.
        let use_nix3 = self.use_nix3_copy;

        let mut command = if use_nix3 {
            // experimental `nix copy` command with ssh-ng://
            let mut command = Command::new("nix");

            command.args([
                "--extra-experimental-features",
                "nix-command",
                "copy",
                "--no-check-sigs",
            ]);

            if options.use_substitutes {
                command.args([
                    "--substitute-on-destination",
                    // needed due to UX bug in ssh-ng://
                    "--builders-use-substitutes",
                ]);
            }

            if let Some("drv") = path.extension().and_then(OsStr::to_str) {
                command.arg("--derivation");
            }

            match direction {
                CopyDirection::ToRemote => {
                    command.arg("--to");
                }
                CopyDirection::FromRemote => {
                    command.arg("--from");
                }
            }

            let mut store_uri = format!("ssh-ng://{}", self.ssh_target());
            if options.gzip {
                store_uri += "?compress=true";
            }
            command.arg(store_uri);

            command.arg(path.as_path());

            command
        } else {
            // nix-copy-closure (ssh://)
            let mut command = Command::new("nix-copy-closure");

            match direction {
                CopyDirection::ToRemote => {
                    command.arg("--to");
                }
                CopyDirection::FromRemote => {
                    command.arg("--from");
                }
            }

            // FIXME: Host-agnostic abstraction
            if options.include_outputs {
                command.arg("--include-outputs");
            }
            if options.use_substitutes {
                command.arg("--use-substitutes");
            }
            if options.gzip {
                command.arg("--gzip");
            }

            command.arg(&self.ssh_target()).arg(path.as_path());

            command
        };

        command.env("NIX_SSHOPTS", ssh_options_str);

        command
    }

    pub fn ssh_options(&self) -> Vec<String> {
        // TODO: Allow configuation of SSH parameters

        let mut options = self.extra_ssh_options.clone();

        // Apply hardware-link binding if configured and not already set
        // (configure_for_initrd handles its own binding, so we check for ProxyCommand)
        if self.force_hw_link {
            let already_has_proxy = options.windows(2).any(|w| {
                w[0] == "-o" && w[1].starts_with("ProxyCommand")
            });
            if !already_has_proxy {
                let mut binding_host = self.clone();
                binding_host.force_hw_link = false; // prevent recursion
                binding_host.extra_ssh_options.clear();
                binding_host.apply_hw_link_binding(self.hw_link_interfaces.as_deref());
                options.extend(binding_host.extra_ssh_options);
            }
        }

        options.extend(
            [
                "-o",
                "StrictHostKeyChecking=accept-new",
                "-o",
                "BatchMode=yes",
                "-T",
            ]
            .iter()
            .map(|s| s.to_string()),
        );

        if let Some(port) = self.port {
            options.push("-p".to_string());
            options.push(port.to_string());
        }

        let mut config_file_arg = None;

        if let Provider::Gcp { project, zone, iap } = &self.provider {
            if *iap {
                // Use gcloud IAP tunnel via ProxyCommand.
                // We use a temporary SSH configuration file to avoid issues with quoting
                // spaces in NIX_SSHOPTS (which are not handled well by nix-copy-closure or ssh-ng).
                let mut proxy_args = vec![
                    "gcloud".to_string(),
                    "compute".to_string(),
                    "start-iap-tunnel".to_string(),
                    "%h".to_string(),
                    "%p".to_string(),
                    "--listen-on-stdin".to_string(),
                ];

                if let Some(p) = project {
                    proxy_args.push(format!("--project={}", p));
                }
                if let Some(z) = zone {
                    proxy_args.push(format!("--zone={}", z));
                }

                proxy_args.push("--quiet".to_string());

                let proxy_cmd = proxy_args.join(" ");
                // Create a deterministic path for this host's config
                let config_path = format!("/tmp/navi-ssh-iap-{}.conf", self.host);
                // We must use 'Host *' or specific host match. Since we pass this file specifically
                // for this connection, 'Host *' is safe and easy.
                let config_content = format!("Host *\n  ProxyCommand {}\n", proxy_cmd);

                // Best effort write
                if let Ok(_) = std::fs::write(&config_path, config_content) {
                    config_file_arg = Some(config_path);
                } else {
                    tracing::error!("Failed to write SSH config file for IAP connection");
                }
            }
        }

        // Fallback to configured ssh_config if not set by IAP
        if config_file_arg.is_none() {
            if let Some(ssh_config) = self.ssh_config.as_ref() {
                config_file_arg = Some(ssh_config.to_str().unwrap().to_string());
            }
        }

        if let Some(path) = config_file_arg {
            options.push("-F".to_string());
            options.push(path);
        }

        options
    }

    /// Uploads a single key.
    async fn upload_key(
        &mut self,
        name: &str,
        key: &Key,
        require_ownership: bool,
    ) -> NaviResult<()> {
        if let Some(job) = &self.job {
            job.message(format!("Uploading key {}", name))?;
        }

        let path = key.path();
        let key_script = key_uploader::generate_script(key, path, require_ownership);

        let mut command = self.ssh(&["sh", "-c", &key_script]);

        command.stdin(Stdio::piped());
        command.stderr(Stdio::piped());
        command.stdout(Stdio::piped());

        let uploader = command.spawn()?;
        key_uploader::feed_uploader(uploader, key, self.job.clone()).await
    }

    /// Returns the current Boot ID.
    async fn get_boot_id(&mut self) -> NaviResult<BootId> {
        let boot_id = self
            .ssh(&["cat", "/proc/sys/kernel/random/boot_id"])
            .capture_output()
            .await?;

        Ok(BootId(boot_id))
    }

    /// Initiates reboot.
    async fn initate_reboot(&mut self) -> NaviResult<()> {
        match self.run_command(self.ssh(&["reboot"])).await {
            Ok(()) => Ok(()),
            Err(e) => {
                if let NaviError::ChildFailure { exit_code: 255, .. } = e {
                    // Assume it's "Connection closed by remote host"
                    Ok(())
                } else {
                    Err(e)
                }
            }
        }
    }
}

fn shell_quote(s: &str) -> String {
    if s.contains(' ') || s.contains('\'') {
        format!("'{}'", s.replace('\'', "'\\''"))
    } else {
        s.to_string()
    }
}
