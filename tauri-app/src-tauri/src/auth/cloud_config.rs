use serde::{Deserialize, Serialize};

/// Represents the two supported Microsoft 365 cloud environments.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CloudEnvironment {
    Global,
    China,
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
