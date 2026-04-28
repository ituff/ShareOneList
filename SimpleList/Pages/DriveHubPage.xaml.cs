using Microsoft.Graph.Models;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Navigation;
using SimpleList.Models;
using SimpleList.Services;
using SimpleList.ViewModels;
using System;
using System.Collections.Generic;
using System.Linq;
using WinUICommunity;

using ResourceHelper = SimpleList.Helpers.ResourceHelper;

namespace SimpleList.Pages
{
    public sealed partial class DriveHubPage : Page
    {
        private OneDrive _provider;
        private string _displayName;
        private List<Site> _sites;

        public DriveHubPage()
        {
            InitializeComponent();
        }

        protected override void OnNavigatedTo(NavigationEventArgs e)
        {
            base.OnNavigatedTo(e);
            if (e.Parameter is DriveViewModel drive)
            {
                _provider = drive.Provider;
                _displayName = drive.DisplayName;
                PageTitle.Text = _displayName;
            }
        }

        private async void OneDrive_DoubleTapped(object sender, Microsoft.UI.Xaml.Input.DoubleTappedRoutedEventArgs e)
        {
            LoadingBar.Visibility = Visibility.Visible;
            var result = await _provider.GetMyDrive();
            LoadingBar.Visibility = Visibility.Collapsed;

            if (result.IsSuccess && result.Data != null)
            {
                _provider.SetDriveId(result.Data.Id);
                DriveViewModel driveVm = new(_provider, _displayName);
                (App.StartupWindow as MainWindow).Navigate(typeof(DrivePage), driveVm);
            }
            else
            {
                Growl.Warning(new GrowlInfo
                {
                    Title = ResourceHelper.GetLocalized("DriveHubPage_NoOneDrive"),
                    Message = result.IsSuccess
                        ? ResourceHelper.GetLocalized("DriveHubPage_NoOneDriveDesc")
                        : result.ErrorMessage,
                    StaysOpen = false,
                    Token = "HubGrowl"
                });
            }
        }

        private async void SharePoint_DoubleTapped(object sender, Microsoft.UI.Xaml.Input.DoubleTappedRoutedEventArgs e)
        {
            LoadingBar.Visibility = Visibility.Visible;
            var result = await _provider.GetSharePointSites();
            LoadingBar.Visibility = Visibility.Collapsed;

            if (result.IsSuccess && result.Data?.Value != null)
            {
                _sites = result.Data.Value;
                if (_sites.Count == 0)
                {
                    Growl.Warning(new GrowlInfo
                    {
                        Title = ResourceHelper.GetLocalized("DriveHubPage_NoSharePoint"),
                        Message = ResourceHelper.GetLocalized("DriveHubPage_NoSharePointDesc"),
                        StaysOpen = false,
                        Token = "HubGrowl"
                    });
                    return;
                }
                SitesList.ItemsSource = _sites;
                ServicePanel.Visibility = Visibility.Collapsed;
                SharePointPanel.Visibility = Visibility.Visible;
            }
            else
            {
                Growl.Error(new GrowlInfo
                {
                    Title = ResourceHelper.GetLocalized("Error"),
                    Message = result.ErrorMessage,
                    StaysOpen = false,
                    Token = "HubGrowl"
                });
            }
        }

        private async void Site_DoubleTapped(object sender, Microsoft.UI.Xaml.Input.DoubleTappedRoutedEventArgs e)
        {
            if (SitesList.SelectedItem is not Site site) return;

            LoadingBar.Visibility = Visibility.Visible;
            var result = await _provider.GetSiteDrives(site.Id);
            LoadingBar.Visibility = Visibility.Collapsed;

            if (result.IsSuccess && result.Data?.Value != null)
            {
                var drives = result.Data.Value;
                if (drives.Count == 1)
                {
                    // Only one drive, go directly to it
                    _provider.SetDriveId(drives[0].Id);
                    DriveViewModel driveVm = new(_provider, drives[0].Name);
                    (App.StartupWindow as MainWindow).Navigate(typeof(DrivePage), driveVm);
                }
                else
                {
                    DrivesList.ItemsSource = drives;
                    SharePointPanel.Visibility = Visibility.Collapsed;
                    DrivesPanel.Visibility = Visibility.Visible;
                }
            }
            else
            {
                Growl.Error(new GrowlInfo
                {
                    Title = ResourceHelper.GetLocalized("Error"),
                    Message = result.ErrorMessage,
                    StaysOpen = false,
                    Token = "HubGrowl"
                });
            }
        }

        private void Drive_DoubleTapped(object sender, Microsoft.UI.Xaml.Input.DoubleTappedRoutedEventArgs e)
        {
            if (DrivesList.SelectedItem is not Drive drive) return;

            _provider.SetDriveId(drive.Id);
            DriveViewModel driveVm = new(_provider, drive.Name);
            (App.StartupWindow as MainWindow).Navigate(typeof(DrivePage), driveVm);
        }

        private void BackToHub(object sender, RoutedEventArgs e)
        {
            SharePointPanel.Visibility = Visibility.Collapsed;
            DrivesPanel.Visibility = Visibility.Collapsed;
            ServicePanel.Visibility = Visibility.Visible;
        }

        private void BackToSites(object sender, RoutedEventArgs e)
        {
            DrivesPanel.Visibility = Visibility.Collapsed;
            SharePointPanel.Visibility = Visibility.Visible;
        }
    }
}
