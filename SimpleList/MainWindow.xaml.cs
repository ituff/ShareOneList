using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media.Animation;
using SimpleList.Pages;
using System;

namespace SimpleList
{
    public sealed partial class MainWindow : Window
    {
        private DriveTabPage _driveTabPage;

        public MainWindow()
        {
            InitializeComponent();
            ExtendsContentIntoTitleBar = true;
            SetTitleBar(AppTitleBar);
            contentFrame.Navigated += ContentFrame_Navigated;
        }

        private void NavigationView_SelectionChanged(NavigationView sender, NavigationViewSelectionChangedEventArgs args)
        {
            if (args.IsSettingsSelected)
            {
                contentFrame.Navigate(typeof(SettingPage));
            }
            else
            {
                var selectedItem = (NavigationViewItem)args.SelectedItem;
                string selectedItemTag = ((string)selectedItem.Tag);

                // "Files" nav item now goes to DriveTabPage (which hosts CloudPage as its first tab)
                if (selectedItemTag == "CloudPage")
                {
                    NavigateToDriveTabPage();
                    return;
                }

                string pageName = "SimpleList.Pages." + selectedItemTag;
                Type pageType = Type.GetType(pageName);
                contentFrame.Navigate(pageType);
            }
        }

        public void Navigate(Type pageType, object targetPageArguments = null, NavigationTransitionInfo navigationTransitionInfo = null)
        {
            RootFrame.Navigate(pageType, targetPageArguments, navigationTransitionInfo);
        }

        public DriveTabPage DriveTabPage => _driveTabPage;

        /// <summary>
        /// Navigate to DriveTabPage and add a new drive tab.
        /// Called by CloudPage when a drive is double-clicked.
        /// </summary>
        public void NavigateToDriveTab(ViewModels.DriveViewModel drive)
        {
            if (_driveTabPage != null)
            {
                _driveTabPage.AddTab(drive);
                // Make sure DriveTabPage is visible
                if (contentFrame.Content != _driveTabPage)
                {
                    contentFrame.Navigate(typeof(DriveTabPage));
                }
            }
            else
            {
                // First time: navigate to DriveTabPage, OnNavigatedTo will create the drive tab
                contentFrame.Navigate(typeof(DriveTabPage), drive);
            }
        }

        /// <summary>
        /// Navigate to DriveTabPage showing the CloudPage tab.
        /// Called when the "Files" nav item is clicked.
        /// </summary>
        private void NavigateToDriveTabPage()
        {
            if (_driveTabPage != null)
            {
                if (contentFrame.Content != _driveTabPage)
                {
                    contentFrame.Navigate(typeof(DriveTabPage));
                }
                // Select the CloudPage tab
                _driveTabPage.SelectCloudTab();
            }
            else
            {
                // First time: just navigate, the constructor creates the CloudPage tab
                contentFrame.Navigate(typeof(DriveTabPage));
            }
        }

        private void ContentFrame_Navigated(object sender, Microsoft.UI.Xaml.Navigation.NavigationEventArgs e)
        {
            if (e.Content is DriveTabPage tabPage)
            {
                _driveTabPage = tabPage;
            }
        }

        public Frame RootFrame => contentFrame;
    }
}
