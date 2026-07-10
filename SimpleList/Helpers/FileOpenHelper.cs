using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using SimpleList.Models;
using SimpleList.Services;
using SimpleList.ViewModels;
using System;
using System.Diagnostics;
using System.IO;
using System.Threading.Tasks;

namespace SimpleList.Helpers
{
    /// <summary>
    /// Shared logic for opening files via double-click:
    /// - Folders: open in current drive view
    /// - Previewable files (Image, Markdown, Text, Media, Office): open in a new preview tab
    /// - Unknown files: confirm with user, download to temp cache, open with system default app
    /// </summary>
    public static class FileOpenHelper
    {
        private static readonly string CacheFolderPath = Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
            "ShareOneList", "FileCache");

        public static async Task OpenFileAsync(FileViewModel file, XamlRoot xamlRoot)
        {
            if (file.IsFolder)
            {
                await file.Drive.OpenFolder(file);
                return;
            }

            FileType fileType = file.PreviewFileType;

            if (fileType != FileType.Unknown)
            {
                // Previewable file: open in a new tab
                var mainWindow = App.StartupWindow as MainWindow;
                mainWindow?.OpenPreviewTab(file);
            }
            else
            {
                // Unknown file type: confirm download, then open locally
                await DownloadAndOpenAsync(file, xamlRoot);
            }
        }

        private static async Task DownloadAndOpenAsync(FileViewModel file, XamlRoot xamlRoot)
        {
            // Ask user for confirmation before downloading
            var dialog = new ContentDialog
            {
                XamlRoot = xamlRoot,
                Title = ResourceHelper.GetLocalized("FileOpen_ConfirmDownload_Title"),
                Content = string.Format(
                    ResourceHelper.GetLocalized("FileOpen_ConfirmDownload_Content"),
                    file.Name),
                PrimaryButtonText = ResourceHelper.GetLocalized("FileOpen_ConfirmDownload_Confirm"),
                CloseButtonText = ResourceHelper.GetLocalized("FileOpen_ConfirmDownload_Cancel"),
                DefaultButton = ContentDialogButton.Primary
            };

            var result = await dialog.ShowAsync();
            if (result != ContentDialogResult.Primary)
            {
                return;
            }

            // Download to local cache
            Directory.CreateDirectory(CacheFolderPath);
            string localPath = Path.Combine(CacheFolderPath, file.Name);

            var downloadResult = await file.Drive.Provider.GetItemContent(file.Id);
            if (!downloadResult.IsSuccess)
            {
                await ShowErrorAsync(xamlRoot, downloadResult.ErrorMessage);
                return;
            }

            try
            {
                using Stream sourceStream = downloadResult.Data;
                using FileStream fileStream = new(localPath, FileMode.Create, FileAccess.Write);
                await sourceStream.CopyToAsync(fileStream);
            }
            catch (Exception ex)
            {
                await ShowErrorAsync(xamlRoot, ex.Message);
                return;
            }

            // Open with system default application
            try
            {
                Process.Start(new ProcessStartInfo
                {
                    FileName = localPath,
                    UseShellExecute = true
                });
            }
            catch (Exception ex)
            {
                await ShowErrorAsync(xamlRoot, ex.Message);
            }
        }

        private static async Task ShowErrorAsync(XamlRoot xamlRoot, string message)
        {
            var errorDialog = new ContentDialog
            {
                XamlRoot = xamlRoot,
                Title = ResourceHelper.GetLocalized("Error"),
                Content = message,
                CloseButtonText = "OK"
            };
            await errorDialog.ShowAsync();
        }
    }
}
