use serde::{de, Deserialize, Deserializer, Serialize, Serializer};

/// Represents the two supported Microsoft 365 cloud environments.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CloudEnvironment {
    Global,
    China,
}

impl Serialize for CloudEnvironment {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let value = match self {
            CloudEnvironment::Global => "global",
            CloudEnvironment::China => "china",
        };
        serializer.serialize_str(value)
    }
}

impl<'de> Deserialize<'de> for CloudEnvironment {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        match value.to_lowercase().as_str() {
            "global" => Ok(CloudEnvironment::Global),
            "china" => Ok(CloudEnvironment::China),
            _ => Err(de::Error::custom(format!(
                "Invalid cloud environment '{}'. Expected 'global' or 'china'.",
                value
            ))),
        }
    }
}

/// Configuration for a specific cloud environment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudConfig {
    pub authority: String,
    pub graph_base_url: String,
    pub scopes: Vec<String>,
    pub sharepoint_domain: String,
    pub client_id: String,
}

impl CloudEnvironment {
    /// Returns the cloud-specific configuration for the given client ID.
    pub fn config(&self, client_id: &str) -> CloudConfig {
        match self {
            CloudEnvironment::Global => CloudConfig {
                authority: "https://login.microsoftonline.com/common".into(),
                graph_base_url: "https://graph.microsoft.com/v1.0".into(),
                scopes: vec![
                    "User.Read".into(),
                    "Files.ReadWrite.All".into(),
                    "Sites.Read.All".into(),
                    "Group.Read.All".into(),
                ],
                sharepoint_domain: "sharepoint.com".into(),
                client_id: client_id.into(),
            },
            CloudEnvironment::China => CloudConfig {
                authority: "https://login.partner.microsoftonline.cn/organizations".into(),
                graph_base_url: "https://microsoftgraph.chinacloudapi.cn/v1.0".into(),
                scopes: vec![
                    "https://microsoftgraph.chinacloudapi.cn/User.Read".into(),
                    "https://microsoftgraph.chinacloudapi.cn/Files.ReadWrite.All".into(),
                    "https://microsoftgraph.chinacloudapi.cn/Sites.Read.All".into(),
                    "https://microsoftgraph.chinacloudapi.cn/Group.Read.All".into(),
                ],
                sharepoint_domain: "sharepoint.cn".into(),
                client_id: client_id.into(),
            },
        }
    }
}
