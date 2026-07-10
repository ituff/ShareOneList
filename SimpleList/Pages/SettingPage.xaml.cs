using Downloader;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using SimpleList.Services;
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Net;
using System.Runtime.InteropServices;
using WinUICommunity;

namespace SimpleList.Pages
{
    public sealed partial class SettingPage : Page
    {
        public SettingPage()
        {
            InitializeComponent();
        }

        private async void CheckUpdate(object sender, RoutedEventArgs e)
        {
            CheckUpdateButton.IsEnabled = false;
            var ver = await UpdateHelper.CheckUpdateAsync("ituff", "SimpleList21V");
            if (ver.IsExistNewVersion)
            {
                // Update App
                IsUpdateAvailable = true;
                var arch = RuntimeInformation.ProcessArchitecture;
                var archMap = new Dictionary<Architecture, string>
                {
                    { Architecture.X64, "x64" },
                    { Architecture.X86, "x86" },
                    { Architecture.Arm64, "arm64" },
                };
                zipballUrl = Array.Find(ver.Assets, asset => asset.Name == $@"ShareOneList-{ver.TagName}-{archMap[arch]}.zip")?.Url;
                StatusInfo.Description = ver.Changelog;
                NewVersion.Text = ver.TagName;
                StatusInfo.Visibility = Visibility.Visible;
                DownloadButton.Visibility = Visibility.Visible;
            }
            else
            {
                StatusInfo.Visibility = Visibility.Visible;
                DownloadButton.Visibility = Visibility.Collapsed;
            }
            CheckUpdateButton.IsEnabled = true;
        }

        private async void DownloadLatestZip(object sender, RoutedEventArgs e)
        {
            //(sender as HyperlinkButton).IsEnabled = false;
            DownloadButton.IsEnabled = false;
            if (!Utils.IsValidUrl(zipballUrl) || !IsUpdateAvailable)
            {
                return;
            }
            var downloadOpt = new DownloadConfiguration()
            {
                ChunkCount = 8,
                ParallelDownload = true,
                RequestConfiguration =
                {
                    Proxy = WebRequest.DefaultWebProxy,
                }
            };
            var downloader = new DownloadService(downloadOpt);
            var file = Path.Combine(Path.GetTempPath(), $@"{Path.GetRandomFileName()}.zip");
            downloader.DownloadFileCompleted += (s, e) =>
            {
                if ((s as IDownloadService).Status == DownloadStatus.Completed)
                {
                    ExtractZip(file);
                }
            };
            await downloader.DownloadFileTaskAsync(zipballUrl, file);
        }

        private static void ExtractZip(string zipFile)
        {
            if (!Path.Exists(zipFile))
            {
                return;
            }
            // Extract Zip using powershell
            var destinationDirectory = AppContext.BaseDirectory;
            string script = $@"Expand-Archive -Path '{zipFile}' -DestinationPath '{destinationDirectory}' -Force";
            Process.Start("PowerShell", script);
        }

        private void UpdateByPowershell(object sender, RoutedEventArgs e)
        {
            if (zipballUrl == null)
            {
                return;
            }
            var zipFile = Path.Combine(Path.GetTempPath(), $@"{Path.GetRandomFileName()}.zip");
            string appDir = AppContext.BaseDirectory;
            string psScript = $@"
                Stop-Process -Name '{Process.GetCurrentProcess().ProcessName}' -Force
                Start-BitsTransfer -Source '{zipballUrl}' -Destination '{zipFile}' -DisplayName 'ShareOneList Update'
                Expand-Archive -Path '{zipFile}' -DestinationPath '{appDir}' -Force
                Remove-Item -Path {zipFile}
                Start-Process '{Path.Combine(appDir, "ShareOneList.exe")}'
                Pause
            ";
            Process.Start("PowerShell", psScript);
        }

        private string zipballUrl;
        public string Version => Utils.GetVersion();
        public bool IsUpdateAvailable { get; set; } = false;
    }
}
