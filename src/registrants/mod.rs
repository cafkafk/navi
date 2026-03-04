use crate::nix::RegistrantsConfig;

pub mod namecheap;
pub mod porkbun;

macro_rules! ok_or_err {
    ($expr:expr) => {
        match $expr {
            Ok(v) => v,
            Err(_) => return Err(()),
        }
    };
}

#[derive(Debug, Clone)]
pub struct DomainInfo {
    pub domain: String,
    pub provider: String,
    pub account: String,
    pub expiry_date: Option<String>,
    pub auto_renew: bool,
}

pub async fn fetch_all(config: &RegistrantsConfig) -> (Vec<DomainInfo>, Vec<String>) {
    let mut domains = Vec::new();
    let mut errors = Vec::new();

    // Fetch Porkbun
    for (name, account) in &config.porkbun {
        // Run both commands
        let api_key = match fetch_raw(&account.api_key_command).await {
            Ok(k) => k,
            Err(e) => {
                errors.push(format!("Porkbun account '{}' API key error: {}", name, e));
                continue;
            }
        };
        let secret = match fetch_raw(&account.secret_api_key_command).await {
            Ok(k) => k,
            Err(e) => {
                errors.push(format!(
                    "Porkbun account '{}' Secret key error: {}",
                    name, e
                ));
                continue;
            }
        };

        let creds = serde_json::json!({
            "apikey": api_key.trim(),
            "secretapikey": secret.trim()
        });

        match porkbun::fetch_domains(&creds).await {
            Ok(porkbun_domains) => {
                for d in porkbun_domains {
                    domains.push(DomainInfo {
                        domain: d.domain,
                        provider: "Porkbun".to_string(),
                        account: name.clone(),
                        expiry_date: d.expire_date,
                        auto_renew: d.auto_renew == 1,
                    });
                }
            }
            Err(e) => {
                errors.push(format!("Porkbun account '{}' API error: {}", name, e));
            }
        }
    }

    // Fetch Namecheap
    for (name, account) in &config.namecheap {
        // Fetch user and key
        let user = match fetch_raw(&account.user_command).await {
            Ok(u) => u.trim().to_string(),
            Err(e) => {
                errors.push(format!(
                    "Namecheap account '{}' user command failed: {}",
                    name, e
                ));
                continue;
            }
        };
        let key = match fetch_raw(&account.api_key_command).await {
            Ok(k) => k.trim().to_string(),
            Err(e) => {
                errors.push(format!(
                    "Namecheap account '{}' api key command failed: {}",
                    name, e
                ));
                continue;
            }
        };

        // Pass to fetcher (it will auto-detect IP)
        match namecheap::fetch_domains(&user, &key).await {
            Ok(nc_domains) => {
                for d in nc_domains {
                    domains.push(DomainInfo {
                        domain: d.name,
                        provider: "Namecheap".to_string(),
                        account: name.clone(),
                        expiry_date: Some(d.expires),
                        auto_renew: d.auto_renew,
                    });
                }
            }
            Err(e) => {
                errors.push(format!("Namecheap account '{}' error: {}", name, e));
            }
        }
    }

    (domains, errors)
}

async fn fetch_raw(command: &str) -> Result<String, String> {
    let parts: Vec<&str> = command.split_whitespace().collect();
    if parts.is_empty() {
        return Err("Empty command".to_string());
    }

    let output = tokio::process::Command::new(parts[0])
        .args(&parts[1..])
        .output()
        .await
        .map_err(|e| format!("Failed to spawn command '{}': {}", parts[0], e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "Command failed ({}): {}",
            output.status,
            stderr.trim()
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

async fn fetch_credentials(command: &str) -> Result<serde_json::Value, ()> {
    let parts: Vec<&str> = command.split_whitespace().collect();
    if parts.is_empty() {
        return Err(());
    }

    let output = ok_or_err!(
        tokio::process::Command::new(parts[0])
            .args(&parts[1..])
            .output()
            .await
    );

    if !output.status.success() {
        return Err(());
    }

    let json: serde_json::Value = ok_or_err!(serde_json::from_slice(&output.stdout));
    Ok(json)
}
