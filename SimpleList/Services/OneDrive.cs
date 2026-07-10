using Microsoft.Graph;
using Microsoft.Graph.Drives.Item.Items.Item.Restore;
using Microsoft.Graph.Drives.Item.Items.Item.SearchWithQ;
using Microsoft.Graph.Models;
using Microsoft.Identity.Client;
using Microsoft.Kiota.Abstractions.Authentication;
using SimpleList.Helpers;
using SimpleList.Models;
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using Windows.Storage;

namespace SimpleList.Services;

public class OneDrive : OneDriveServiceBase
{
    public OneDrive(CloudType cloudType)
    {
        CloudType = cloudType;
        PublicClientApp = App.BuildPublicApp(cloudType);
    }

    public OneDrive(string driveId, string homeAccountId, CloudType cloudType)
    {
        DriveId = driveId;
        HomeAccountId = homeAccountId;
        CloudType = cloudType;
        PublicClientApp = App.BuildPublicApp(cloudType);
    }

    public async Task<OneDriveResult<DriveItemCollectionResponse>> GetFiles(string parentId = "Root")
    {
        return await ExecuteAsync(async () =>
        {
            return await graphClient.Drives[DriveId].Items[parentId].Children.GetAsync();
        }, () => ValidateNotEmpty(parentId, nameof(parentId)));
    }

    public async Task<OneDriveResult<ThumbnailSetCollectionResponse>> GetThumbNails(string itemId)
    {
        return await ExecuteAsync(async () =>
        {
            return await graphClient.Drives[DriveId].Items[itemId].Thumbnails.GetAsync();
        }, () => ValidateNotEmpty(itemId, nameof(itemId)));
    }

    public async Task<OneDriveResult<DriveItem>> GetItem(string itemId)
    {
        return await ExecuteAsync(async () =>
        {
            return await graphClient.Drives[DriveId].Items[itemId].GetAsync();
        }, () => ValidateNotEmpty(itemId, nameof(itemId)));
    }

    public async Task<OneDriveResult<Stream>> GetItemContent(string itemId)
    {
        return await ExecuteAsync(async () =>
        {
            return await graphClient.Drives[DriveId].Items[itemId].Content.GetAsync();
        }, () => ValidateNotEmpty(itemId, nameof(itemId)));
    }

    /// <summary>
    /// Get a pre-authenticated preview URL for a file via the Graph preview API.
    /// Returns an embeddable URL that does not require additional authentication.
    /// </summary>
    public async Task<OneDriveResult<ItemPreviewInfo>> GetPreviewUrl(string itemId)
    {
        return await ExecuteAsync(async () =>
        {
            var requestBody = new Microsoft.Graph.Drives.Item.Items.Item.Preview.PreviewPostRequestBody();
            return await graphClient.Drives[DriveId].Items[itemId].Preview.PostAsync(requestBody);
        }, () => ValidateNotEmpty(itemId, nameof(itemId)));
    }

    public async Task<OneDriveResult<DriveItem>> CreateFolder(string parentItemId, string folderName)
    {
        return await ExecuteAsync(async () =>
        {
            var requestBody = new DriveItem
            {
                Name = folderName,
                Folder = new Folder { },
                AdditionalData = new Dictionary<string, object>
                {
                    {
                        "@microsoft.graph.conflictBehavior" , "rename"
                    },
                },
            };
            return await graphClient.Drives[DriveId].Items[parentItemId].Children.PostAsync(requestBody);
        }, () =>
        {
            ValidateNotEmpty(parentItemId, nameof(parentItemId));
            ValidateFileName(folderName, nameof(folderName));
        });
    }

    public async Task<OneDriveResult<DriveItem>> RenameFile(string itemId, string newName)
    {
        return await ExecuteAsync(async () =>
        {
            DriveItem requestBody = new()
            {
                Name = newName,
            };
            return await graphClient.Drives[DriveId].Items[itemId].PatchAsync(requestBody);
        }, () =>
        {
            ValidateNotEmpty(itemId, nameof(itemId));
            ValidateFileName(newName, nameof(newName));
        });
    }

    public async Task<OneDriveResult<DriveItem>> UploadFileAsync(StorageFile file, string itemId, IProgress<long> progress = null)
    {
        return await ExecuteAsync(async () =>
        {
            ValidateNotNull(file, nameof(file));
            ValidateNotEmpty(itemId, nameof(itemId));

            Stream stream = await file.OpenStreamForReadAsync();
            if ((await file.GetBasicPropertiesAsync()).Size == 0)
            {
                // Upload an empty file
                await graphClient.Drives[DriveId].Items[itemId].ItemWithPath(file.Name).Content.PutAsync(new MemoryStream());
                return await graphClient.Drives[DriveId].Items[itemId].ItemWithPath(file.Name).GetAsync();
            }
            
            var uploadSessionRequestBody = new Microsoft.Graph.Drives.Item.Items.Item.CreateUploadSession.CreateUploadSessionPostRequestBody
            {
                Item = new DriveItemUploadableProperties
                {
                    AdditionalData = new Dictionary<string, object>
                    {
                        { "@microsoft.graph.conflictBehavior", "replace" },
                    },
                },
            };
            
            UploadSession uploadSession = await graphClient.Drives[DriveId].Items[itemId].ItemWithPath(file.Name).CreateUploadSession.PostAsync(uploadSessionRequestBody);
            int maxChunckSize = 320 * 1024;
            LargeFileUploadTask<DriveItem> fileUploadTask = new(uploadSession, stream, maxChunckSize, graphClient.RequestAdapter);

            var uploadResult = await fileUploadTask.UploadAsync(progress);
            return uploadResult.ItemResponse;
        });
    }

    public async Task<OneDriveResult<DriveItem>> UploadFolderAsync(StorageFolder folder, string itemId, IProgress<long> progress = null)
    {
        return await ExecuteAsync(async () =>
        {
            ValidateNotNull(folder, nameof(folder));
            ValidateNotEmpty(itemId, nameof(itemId));

            ulong totalSize = await Utils.GetFolderSize(folder);
            ulong uploadedSize = 0;
            var files = await folder.GetFilesAsync();
            
            var createFolderResult = await CreateFolder(itemId, folder.Name);
            if (!createFolderResult.IsSuccess)
            {
                throw new Exception($"{"CreateFolderFail".GetLocalized()}: {createFolderResult.ErrorMessage}");
            }


            DriveItem cloudFolder = createFolderResult.Data;

            IEnumerable<Task> uploadTasks = files.Select(async file =>
            {
                var uploadResult = await UploadFileAsync(file, cloudFolder.Id, progress);
                if (!uploadResult.IsSuccess)
                {
                    throw new Exception($"{"UploadFileFail".GetLocalized()}: {uploadResult.ErrorMessage}");
                }
                ulong fileSize = (await file.GetBasicPropertiesAsync()).Size;
                Interlocked.Add(ref uploadedSize, fileSize);
                progress?.Report((long)(uploadedSize / totalSize));
            });
            await Task.WhenAll(uploadTasks);

            IReadOnlyList<StorageFolder> subfolders = await folder.GetFoldersAsync();
            IEnumerable<Task> subfolderTasks = subfolders.Select(async subfolder => 
            {
                var folderResult = await UploadFolderAsync(subfolder, cloudFolder.Id, progress);
                if (!folderResult.IsSuccess)
                {
                    throw new Exception($"{"UploadFolderFail".GetLocalized()}: {folderResult.ErrorMessage}");
                }
            });
            await Task.WhenAll(subfolderTasks);

            return cloudFolder;
        });
    }

    public async Task<OneDriveResult<string>> CreateLink(string itemId, DateTimeOffset? expirationDateTime = null, string password = null, string type = "view")
    {
        return await ExecuteAsync(async () =>
        {
            Microsoft.Graph.Drives.Item.Items.Item.CreateLink.CreateLinkPostRequestBody requestBody = new()
            {
                Type = type,
                Password = password,
                Scope = "anonymous",
                RetainInheritedPermissions = false,
                ExpirationDateTime = expirationDateTime,
            };
            Permission result = await graphClient.Drives[DriveId].Items[itemId].CreateLink.PostAsync(requestBody);
            return result.Link.WebUrl;
        }, () => ValidateNotEmpty(itemId, nameof(itemId)));
    }

    public async Task<OneDriveResult<string>> GetDisplayName()
    {
        return await ExecuteAsync(async () =>
        {
            User user = await graphClient.Me.GetAsync();
            return user.DisplayName;
        });
    }

    public async Task<OneDriveResult<Quota>> GetStorageInfo()
    {
        return await ExecuteAsync(async () =>
        {
            Drive drive = await graphClient.Drives[DriveId].GetAsync();
            return drive.Quota;
        });
    }

    public async Task<OneDriveResult<bool>> ConvertFileFormat(string itemId, StorageFile file, string format = "pdf")
    {
        return await ExecuteAsync(async () =>
        {
            Stream result = await graphClient.Drives[DriveId].Items[itemId].Content.GetAsync(requestConfiguration =>
            {
                // The document is written like this, but there is an error. Upon reviewing the source code, I found that there is no definition for "QueryParameters."
                // requestConfiguration.QueryParameters.Format = format;
                requestConfiguration.Headers.Add("Format", format);
            });
            using Stream fileStream = await file.OpenStreamForWriteAsync();
            if (result.CanSeek)
            {
                result.Seek(0, SeekOrigin.Begin);
            }
            await result.CopyToAsync(fileStream);
            return true;
        }, () =>
        {
            ValidateNotEmpty(itemId, nameof(itemId));
            ValidateNotNull(file, nameof(file));
            ValidateNotEmpty(format, nameof(format));
        });
    }

    public async Task<OneDriveResult<bool>> DeleteItem(string itemId)
    {
        return await ExecuteAsync(async () =>
        {
            await graphClient.Drives[DriveId].Items[itemId].DeleteAsync();
            return true;
        }, () => ValidateNotEmpty(itemId, nameof(itemId)));
    }

    public async Task<OneDriveResult<bool>> PermanentDeleteItem(string itemId)
    {
        return await ExecuteAsync(async () =>
        {
            await graphClient.Drives[DriveId].Items[itemId].PermanentDelete.PostAsync();
            return true;
        }, () => ValidateNotEmpty(itemId, nameof(itemId)));
    }

    public async Task<OneDriveResult<DriveItem>> RestoreItem(string itemId)
    {
        return await ExecuteAsync(async () =>
        {
            // Restore the original name by default.
            RestorePostRequestBody requestBody = new()
            {
                ParentReference = new ItemReference
                {
                    Id = itemId,
                },
            };
            return await graphClient.Drives[DriveId].Items[itemId].Restore.PostAsync(requestBody);
        }, () => ValidateNotEmpty(itemId, nameof(itemId)));
    }

    public async Task<OneDriveResult<SearchWithQGetResponse>> SearchLocalItems(string query, string itemId)
    {
        return await ExecuteAsync(async () =>
        {
            // According to code, Microsoft.Graph.Drives.Item.Items.Item.SearchWithQ.SearchWithQResponse
            // is same as Microsoft.Graph.Drives.Item.SearchWithQ.SearchWithQResponse, so why Microsoft do this?
            return await graphClient.Drives[DriveId].Items[itemId].SearchWithQ(query).GetAsSearchWithQGetResponseAsync();
        }, () =>
        {
            ValidateNotEmpty(query, nameof(query));
            ValidateNotEmpty(itemId, nameof(itemId));
        });
    }

    public async Task<OneDriveResult<Microsoft.Graph.Drives.Item.SearchWithQ.SearchWithQGetResponse>> SearchGlobalItems(string query)
    {
        return await ExecuteAsync(async () =>
        {
            return await graphClient.Drives[DriveId].SearchWithQ(query).GetAsSearchWithQGetResponseAsync();
        }, () => ValidateNotEmpty(query, nameof(query)));
    }

    private class TokenProvider : IAccessTokenProvider
    {
        private readonly Func<string[], Task<string>> getTokenDelegate;
        private readonly string[] scopes;

        public TokenProvider(Func<string[], Task<string>> getTokenDelegate, string[] scopes)
        {
            this.getTokenDelegate = getTokenDelegate;
            this.scopes = scopes;
        }

        public Task<string> GetAuthorizationTokenAsync(Uri uri, Dictionary<string, object> additionalAuthenticationContext = default,
            CancellationToken cancellationToken = default)
        {
            return getTokenDelegate(scopes);
        }

        public AllowedHostsValidator AllowedHostsValidator { get; }
    }

    public async Task<OneDriveResult<bool>> Login()
    {
        try
        {
            await LoginInternal();
            return OneDriveResult<bool>.Success(true);
        }
        catch (Exception ex)
        {
            return OneDriveResult<bool>.Failure($"{"LoginFail".GetLocalized()}: {ex.Message}", OneDriveErrorType.Authentication, ex);
        }
    }

    private async Task LoginInternal()
    {
        string[] currentScopes = CloudTypeConfig.GetScopes(CloudType);

        // Acquire token eagerly so the login actually happens now
        await AcquireTokenAsync(currentScopes);

        TokenProvider tokenProvider = new(async Task<string> (string[] scopes) =>
        {
            // For subsequent token requests, try silent first then interactive
            await AcquireTokenAsync(scopes);
            return authResult?.AccessToken;
        }, currentScopes);
        BaseBearerTokenAuthenticationProvider authProvider = new(tokenProvider);
        string graphBaseUrl = CloudTypeConfig.GetGraphBaseUrl(CloudType);
        graphClient = new(authProvider, graphBaseUrl);
        SaveTokenCache();
    }

    private async Task AcquireTokenAsync(string[] scopes)
    {
        IEnumerable<IAccount> accounts = await PublicClientApp.GetAccountsAsync().ConfigureAwait(false);

        try
        {
            authResult = await PublicClientApp
                            .AcquireTokenSilent(scopes, accounts.FirstOrDefault(account => account.HomeAccountId.Identifier == HomeAccountId))
                            .ExecuteAsync();
        }
        catch (Exception exception) when (exception is MsalUiRequiredException || exception is InvalidOperationException)
        {
            IntPtr hwnd = WinRT.Interop.WindowNative.GetWindowHandle(App.StartupWindow);
            authResult = await PublicClientApp.AcquireTokenInteractive(scopes)
                            .WithParentActivityOrWindow(hwnd)
                            .ExecuteAsync();
        }

        if (authResult != null)
        {
            IsAuthenticated = true;
            HomeAccountId = authResult.Account.HomeAccountId.Identifier;
        }
    }

    /// <summary>
    /// Get the current user's OneDrive. Returns null data if user has no OneDrive license.
    /// </summary>
    public async Task<OneDriveResult<Drive>> GetMyDrive()
    {
        return await ExecuteAsync(async () =>
        {
            return await graphClient.Me.Drive.GetAsync();
        });
    }

    /// <summary>
    /// Get SharePoint sites the user has access to.
    /// For 21Vianet (China): gets sites via user's joined M365 groups (since search=* and followedSites are not reliably supported).
    /// For Global: uses /sites?search=*.
    /// </summary>
    public async Task<OneDriveResult<List<Site>>> GetSharePointSites()
    {
        if (CloudType == CloudType.China)
        {
            return await ExecuteAsync(async () =>
            {
                // Get user's M365 groups, each has an associated SharePoint site
                var groups = await graphClient.Me.MemberOf.GetAsync();
                var sites = new List<Site>();
                if (groups?.Value != null)
                {
                    foreach (var obj in groups.Value)
                    {
                        if (obj is Group group && group.GroupTypes?.Contains("Unified") == true)
                        {
                            try
                            {
                                var site = await graphClient.Groups[group.Id].Sites["root"].GetAsync();
                                if (site != null)
                                {
                                    sites.Add(site);
                                }
                            }
                            catch
                            {
                                // Skip groups without a SharePoint site
                            }
                        }
                    }
                }
                return sites;
            });
        }
        return await ExecuteAsync(async () =>
        {
            var result = await graphClient.Sites.GetAsync(config =>
            {
                config.QueryParameters.Search = "*";
            });
            return result?.Value ?? new List<Site>();
        });
    }

    /// <summary>
    /// Get all drives (document libraries) for a given SharePoint site.
    /// </summary>
    public async Task<OneDriveResult<DriveCollectionResponse>> GetSiteDrives(string siteId)
    {
        return await ExecuteAsync(async () =>
        {
            return await graphClient.Sites[siteId].Drives.GetAsync();
        }, () => ValidateNotEmpty(siteId, nameof(siteId)));
    }

    /// <summary>
    /// Discover SharePoint document libraries the user has access to via sharedWithMe.
    /// Requires the user to have a OneDrive license.
    /// Extracts unique driveIds from shared items and resolves each to a Drive object.
    /// </summary>
    public async Task<OneDriveResult<List<Drive>>> GetSharedDrives()
    {
        return await ExecuteAsync(async () =>
        {
            // First get the user's drive id - this requires OneDrive license
            var myDrive = await graphClient.Me.Drive.GetAsync();
            var sharedItems = await graphClient.Drives[myDrive.Id].SharedWithMe.GetAsSharedWithMeGetResponseAsync();
            var driveIds = new HashSet<string>();
            var drives = new List<Drive>();

            if (sharedItems?.Value != null)
            {
                foreach (var item in sharedItems.Value)
                {
                    string remoteDriveId = item.RemoteItem?.ParentReference?.DriveId;
                    if (!string.IsNullOrEmpty(remoteDriveId) && driveIds.Add(remoteDriveId))
                    {
                        try
                        {
                            var drive = await graphClient.Drives[remoteDriveId].GetAsync();
                            if (drive != null)
                            {
                                drives.Add(drive);
                            }
                        }
                        catch
                        {
                            // Skip drives we can't access
                        }
                    }
                }
            }
            return drives;
        });
    }

    public void SetDriveId(string driveId)
    {
        DriveId = driveId;
    }

    public static void SaveTokenCache()
    {
        // Cache registration is now handled in App.BuildPublicApp
    }

    private IPublicClientApplication PublicClientApp;
    private static AuthenticationResult authResult;
    private GraphServiceClient graphClient;

    public string DriveId;
    public bool IsAuthenticated = false;
    public string ClientId;
    // used for identify account for now
    public string HomeAccountId;
    public CloudType CloudType;

    protected override async Task EnsureAuthenticatedAsync()
    {
        if (!IsAuthenticated)
        {
            await LoginInternal();
        }
    }
}
