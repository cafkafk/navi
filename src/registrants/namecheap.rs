use reqwest::{Client, Url};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct ApiResponse {
    #[serde(rename = "@Status")]
    status: String,
    #[serde(rename = "CommandResponse")]
    command_response: CommandResponse,
    #[serde(rename = "Errors")]
    errors: Errors,
}

#[derive(Debug, Deserialize)]
struct Errors {
    #[serde(rename = "Error", default)]
    list: Vec<ApiError>,
}

#[derive(Debug, Deserialize)]
struct ApiError {
    #[serde(rename = "$value")]
    text: String,
}

#[derive(Debug, Deserialize)]
struct CommandResponse {
    #[serde(rename = "DomainGetListResult")]
    domain_get_list_result: DomainGetListResult,
}

#[derive(Debug, Deserialize)]
struct DomainGetListResult {
    #[serde(rename = "Domain", default)]
    domains: Vec<NamecheapDomainRaw>,
}

#[derive(Debug, Deserialize)]
struct NamecheapDomainRaw {
    #[serde(rename = "@ID")]
    id: String,
    #[serde(rename = "@Name")]
    name: String,
    #[serde(rename = "@Expires")]
    expires: String,
    #[serde(rename = "@AutoRenew")]
    auto_renew: String,
}

pub struct NamecheapDomain {
    pub name: String,
    pub expires: String,
    pub auto_renew: bool,
}

pub async fn fetch_domains(
    user: &str,
    key: &str,
) -> Result<Vec<NamecheapDomain>, Box<dyn std::error::Error>> {
    // Auto-detect IP
    let client = Client::builder().user_agent("Navi/1.0").build()?;

    // Fetch IP from ipify
    let ip_res = client.get("https://api.ipify.org").send().await?;
    let ip = ip_res.text().await?;

    // Command: namecheap.domains.getList
    let params = vec![
        ("ApiUser", user),
        ("ApiKey", key),
        ("UserName", user),
        ("Command", "namecheap.domains.getList"),
        ("ClientIp", ip.as_str()),
        ("PageSize", "100"),
    ];

    let url = Url::parse_with_params("https://api.namecheap.com/xml.response", &params)?;

    let res = client.get(url).send().await?;

    let text = res.text().await?;

    // Parse XML
    match quick_xml::de::from_str::<ApiResponse>(&text) {
        Ok(response) => {
            if response.status == "OK" {
                let domains = response
                    .command_response
                    .domain_get_list_result
                    .domains
                    .into_iter()
                    .map(|d| NamecheapDomain {
                        name: d.name,
                        expires: d.expires,
                        auto_renew: d.auto_renew.to_lowercase() == "true",
                    })
                    .collect();
                Ok(domains)
            } else {
                let err_msg = response
                    .errors
                    .list
                    .first()
                    .map(|e| e.text.clone())
                    .unwrap_or_else(|| "Unknown API Error".to_string());
                Err(format!("Namecheap API Error (IP: {}): {}", ip, err_msg).into())
            }
        }
        Err(e) => Err(format!(
            "Failed to parse Namecheap XML (IP: {}): {}. Body: {}",
            ip, e, text
        )
        .into()),
    }
}
