namespace SimpleList.Models;

public enum CloudType
{
    Global,
    China
}

public static class CloudTypeConfig
{
    public static string GetAuthority(CloudType cloudType)
    {
        return cloudType switch
        {
            CloudType.China => "https://login.partner.microsoftonline.cn/organizations",
            _ => "https://login.microsoftonline.com/common"
        };
    }

    public static string GetGraphBaseUrl(CloudType cloudType)
    {
        return cloudType switch
        {
            CloudType.China => "https://microsoftgraph.chinacloudapi.cn/v1.0",
            _ => "https://graph.microsoft.com/v1.0"
        };
    }

    public static string[] GetScopes(CloudType cloudType)
    {
        return cloudType switch
        {
            CloudType.China => [
                "https://microsoftgraph.chinacloudapi.cn/User.Read",
                "https://microsoftgraph.chinacloudapi.cn/Files.ReadWrite.All",
                "https://microsoftgraph.chinacloudapi.cn/Sites.Read.All"
            ],
            _ => ["User.Read", "Files.ReadWrite.All", "Sites.Read.All"]
        };
    }

    public static string GetSharePointDomain(CloudType cloudType)
    {
        return cloudType switch
        {
            CloudType.China => "sharepoint.cn",
            _ => "sharepoint.com"
        };
    }
}
