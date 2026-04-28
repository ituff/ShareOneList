using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media.Imaging;
using Microsoft.UI.Xaml.Navigation;
using SimpleList.Models;
using SimpleList.Services;
using SimpleList.ViewModels;
using System;
using System.IO;
using System.Threading.Tasks;
using Windows.Media.Core;
using Windows.Storage.Streams;

namespace SimpleList.Pages
{
    public sealed partial class PreviewPage : Page
    {
        private FileViewModel _file;

        public PreviewPage()
        {
            InitializeComponent();
        }

        protected override void OnNavigatedTo(NavigationEventArgs e)
        {
            base.OnNavigatedTo(e);
            if (e.Parameter is FileViewModel file)
            {
                _file = file;
                _ = LoadPreviewAsync();
            }
        }

        private async Task LoadPreviewAsync()
        {
            LoadingRing.IsActive = true;

            try
            {
                switch (_file.PreviewFileType)
                {
                    case FileType.Image:
                        await LoadImageAsync();
                        break;
                    case FileType.Markdown:
                    case FileType.Text:
                    case FileType.Code:
                        await LoadTextAsync();
                        break;
                    case FileType.Media:
                        LoadMedia();
                        break;
                    case FileType.Office:
                        await LoadWebPreviewAsync();
                        break;
                }
            }
            finally
            {
                LoadingRing.IsActive = false;
            }
        }

        private async Task LoadImageAsync()
        {
            OneDriveResult<Stream> result = await _file.Drive.Provider.GetItemContent(_file.Id);
            if (result.IsSuccess)
            {
                using Stream stream = result.Data;
                var randomAccessStream = new InMemoryRandomAccessStream();
                await RandomAccessStream.CopyAsync(stream.AsInputStream(), randomAccessStream);
                randomAccessStream.Seek(0);
                BitmapImage img = new();
                await img.SetSourceAsync(randomAccessStream);
                PreviewImage.Source = img;
                ImageContainer.Visibility = Visibility.Visible;
            }
        }

        private async Task LoadTextAsync()
        {
            OneDriveResult<Stream> result = await _file.Drive.Provider.GetItemContent(_file.Id);
            if (result.IsSuccess)
            {
                using Stream stream = result.Data;
                using StreamReader reader = new(stream);
                string text = await reader.ReadToEndAsync();
                MarkdownBlock.Text = text;
                MarkdownContainer.Visibility = Visibility.Visible;
            }
        }

        private void LoadMedia()
        {
            string downloadUrl = _file.DownloadUrl;
            if (!string.IsNullOrEmpty(downloadUrl))
            {
                MediaPlayer.Source = MediaSource.CreateFromUri(new Uri(downloadUrl));
                MediaPlayer.Visibility = Visibility.Visible;
            }
        }

        /// <summary>
        /// Load Office file preview using the Graph preview API.
        /// The preview API returns a pre-authenticated embeddable URL (getUrl)
        /// that works without additional login — achieving SSO.
        /// Falls back to WebUrl?web=1 if the preview API is unavailable.
        /// </summary>
        private async Task LoadWebPreviewAsync()
        {
            // Try the Graph preview API first — returns a pre-authenticated URL
            var previewResult = await _file.Drive.Provider.GetPreviewUrl(_file.Id);
            if (previewResult.IsSuccess && previewResult.Data != null)
            {
                string previewUrl = previewResult.Data.GetUrl;
                if (!string.IsNullOrEmpty(previewUrl))
                {
                    WebViewer.Source = new Uri(previewUrl);
                    WebViewer.Visibility = Visibility.Visible;
                    return;
                }
            }

            // Fallback: use WebUrl with ?web=1
            string webUrl = _file.WebUrl;
            if (!string.IsNullOrEmpty(webUrl))
            {
                string fallbackUrl = webUrl.Contains("?") ? webUrl + "&web=1" : webUrl + "?web=1";
                WebViewer.Source = new Uri(fallbackUrl);
                WebViewer.Visibility = Visibility.Visible;
            }
        }
    }
}
