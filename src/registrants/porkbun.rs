use reqwest::Client;
use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub struct PorkbunDomain {
    pub domain: String,
    #[serde(rename = "expireDate")]
    pub expire_date: Option<String>,
    #[serde(rename = "autoRenew", deserialize_with = "deserialize_string_or_int")]
    pub auto_renew: i32,
    #[serde(rename = "status")]
    pub status: Option<String>,
}

fn deserialize_string_or_int<'de, D>(deserializer: D) -> Result<i32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrInt {
        String(String),
        Int(i32),
    }

    match StringOrInt::deserialize(deserializer)? {
        StringOrInt::String(s) => s.parse().map_err(serde::de::Error::custom),
        StringOrInt::Int(i) => Ok(i),
    }
}

#[derive(Deserialize, Debug)]
struct PorkbunListResponse {
    pub status: String,
    pub domains: Option<Vec<PorkbunDomain>>,
}

pub async fn fetch_domains(
    credentials: &serde_json::Value,
) -> Result<Vec<PorkbunDomain>, Box<dyn std::error::Error>> {
    let client = Client::builder().user_agent("Navi/1.0").build()?;

    // Credentials should contain apikey and secretapikey
    // We just pass the whole object as payload
    let res = client
        .post("https://api.porkbun.com/api/json/v3/domain/listAll")
        .header("Content-Type", "application/json")
        .json(credentials)
        .send()
        .await?;

    let text_body = res.text().await?;

    // Try to parse the successful response
    match serde_json::from_str::<PorkbunListResponse>(&text_body) {
        Ok(resp_json) => {
            if resp_json.status == "SUCCESS" {
                Ok(resp_json.domains.unwrap_or_default())
            } else {
                Err(format!("Porkbun API Error: {}", text_body).into())
            }
        }
        Err(e) => {
            // If parsing fails, return the raw body/error so we can see it
            Err(format!(
                "Failed to parse Porkbun response: {}. Body: {}",
                e, text_body
            )
            .into())
        }
    }
}

#[derive(Deserialize, Debug, Clone)]
pub struct PorkbunDnsRecord {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub record_type: String,
    pub content: String,
    pub ttl: String,
    pub prio: Option<String>,
    pub notes: Option<String>,
}

#[derive(Deserialize, Debug)]
struct PorkbunDnsResponse {
    pub status: String,
    pub records: Option<Vec<PorkbunDnsRecord>>,
}

pub async fn fetch_dns_records(
    domain: &str,
    credentials: &serde_json::Value,
) -> Result<Vec<PorkbunDnsRecord>, Box<dyn std::error::Error>> {
    let client = Client::builder().user_agent("Navi/1.0").build()?;

    let url = format!(
        "https://api.porkbun.com/api/json/v3/dns/retrieve/{}",
        domain
    );
    let res = client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(credentials)
        .send()
        .await?;

    let text_body = res.text().await?;

    match serde_json::from_str::<PorkbunDnsResponse>(&text_body) {
        Ok(resp_json) => {
            if resp_json.status == "SUCCESS" {
                Ok(resp_json.records.unwrap_or_default())
            } else {
                Err(format!("Porkbun API Error: {}", text_body).into())
            }
        }
        Err(e) => Err(format!(
            "Failed to parse Porkbun DNS response: {}. Body: {}",
            e, text_body
        )
        .into()),
    }
}

#[derive(Deserialize, Debug, Clone)]
pub struct PorkbunGlueIps {
    #[serde(default)]
    pub v4: Vec<String>,
    #[serde(default)]
    pub v6: Vec<String>,
}

#[derive(Deserialize, Debug)]
struct PorkbunGlueResponse {
    pub status: String,
    pub hosts: Option<Vec<(String, PorkbunGlueIps)>>,
}

#[derive(Debug, Clone)]
pub struct PorkbunGlueRecord {
    pub host: String,
    pub ips: PorkbunGlueIps,
}

pub async fn fetch_glue_records(
    domain: &str,
    credentials: &serde_json::Value,
) -> Result<Vec<PorkbunGlueRecord>, Box<dyn std::error::Error>> {
    let client = Client::builder().user_agent("Navi/1.0").build()?;

    let url = format!(
        "https://api.porkbun.com/api/json/v3/domain/getGlue/{}",
        domain
    );
    let res = client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(credentials)
        .send()
        .await?;

    let text_body = res.text().await?;

    match serde_json::from_str::<PorkbunGlueResponse>(&text_body) {
        Ok(resp_json) => {
            if resp_json.status == "SUCCESS" {
                let records = resp_json
                    .hosts
                    .unwrap_or_default()
                    .into_iter()
                    .map(|(host, ips)| PorkbunGlueRecord { host, ips })
                    .collect();
                Ok(records)
            } else {
                Err(format!("Porkbun API Error: {}", text_body).into())
            }
        }
        Err(e) => Err(format!(
            "Failed to parse Porkbun Glue response: {}. Body: {}",
            e, text_body
        )
        .into()),
    }
}
