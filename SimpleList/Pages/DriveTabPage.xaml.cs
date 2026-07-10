using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Navigation;
using SimpleList.Helpers;
using SimpleList.ViewModels;
using System;
using System.Collections.Generic;

namespace SimpleList.Pages
{
    public sealed partial class DriveTabPage : Page
    {
        /// <summary>
        /// Each tab's state: its DriveViewModel (null for the CloudPage tab), its own Frame, and the TabViewItem.
        /// </summary>
        private class TabState
        {
            public DriveViewModel Drive { get; set; }
            public Frame Frame { get; set; }
            public TabViewItem TabItem { get; set; }
            public bool IsFixed { get; set; }
        }

        private readonly Dictionary<TabViewItem, TabState> _tabStates = new();
        private TabState _activeTab;
        private TabViewItem _cloudTabItem;

        public string BackTooltip { get; } = ResourceHelper.GetLocalized("DriveTabPage_Back");
        public string ForwardTooltip { get; } = ResourceHelper.GetLocalized("DriveTabPage_Forward");
        public string RefreshTooltip { get; } = ResourceHelper.GetLocalized("DriveTabPage_Refresh");

        public DriveTabPage()
        {
            InitializeComponent();
            CreateCloudTab();
        }

        /// <summary>
        /// Create the fixed CloudPage tab as the first tab.
        /// </summary>
        private void CreateCloudTab()
        {
            var frame = new Frame();
            frame.Navigated += TabFrame_Navigated;

            var tabItem = new TabViewItem
            {
                Header = ResourceHelper.GetLocalized("DriveTabPage_FilesTab"),
                IconSource = new SymbolIconSource { Symbol = Symbol.Document },
                IsClosable = false
            };

            var state = new TabState
            {
                Drive = null,
                Frame = frame,
                TabItem = tabItem,
                IsFixed = true
            };
            _tabStates[tabItem] = state;
            _cloudTabItem = tabItem;

            frame.Visibility = Visibility.Collapsed;
            FrameContainer.Children.Add(frame);

            DriveTabView.TabItems.Add(tabItem);
            DriveTabView.SelectedItem = tabItem;

            // Navigate to CloudPage
            frame.Navigate(typeof(CloudPage));
        }

        protected override void OnNavigatedTo(NavigationEventArgs e)
        {
            base.OnNavigatedTo(e);
            if (e.Parameter is DriveViewModel drive)
            {
                AddTab(drive);
            }
        }

        /// <summary>
        /// Add a new drive tab and select it.
        /// </summary>
        public void AddTab(DriveViewModel drive)
        {
            var frame = new Frame();
            frame.Navigated += TabFrame_Navigated;

            var tabItem = new TabViewItem
            {
                Header = drive.DisplayName,
                IconSource = new SymbolIconSource { Symbol = Symbol.Globe }
            };

            var state = new TabState
            {
                Drive = drive,
                Frame = frame,
                TabItem = tabItem,
                IsFixed = false
            };
            _tabStates[tabItem] = state;

            frame.Visibility = Visibility.Collapsed;
            FrameContainer.Children.Add(frame);

            DriveTabView.TabItems.Add(tabItem);
            DriveTabView.SelectedItem = tabItem;

            frame.Navigate(typeof(DriveHubPage), drive);
        }

        /// <summary>
        /// Add a new preview tab for a file and select it.
        /// </summary>
        public void AddPreviewTab(FileViewModel file)
        {
            var frame = new Frame();
            frame.Navigated += TabFrame_Navigated;

            var tabItem = new TabViewItem
            {
                Header = file.Name,
                IconSource = new SymbolIconSource { Symbol = Symbol.Preview }
            };

            var state = new TabState
            {
                Drive = file.Drive,
                Frame = frame,
                TabItem = tabItem,
                IsFixed = false
            };
            _tabStates[tabItem] = state;

            frame.Visibility = Visibility.Collapsed;
            FrameContainer.Children.Add(frame);

            DriveTabView.TabItems.Add(tabItem);
            DriveTabView.SelectedItem = tabItem;

            frame.Navigate(typeof(PreviewPage), file);
        }

        /// <summary>
        /// Select the CloudPage tab.
        /// </summary>
        public void SelectCloudTab()
        {
            if (_cloudTabItem != null)
            {
                DriveTabView.SelectedItem = _cloudTabItem;
            }
        }

        private void DriveTabView_SelectionChanged(object sender, SelectionChangedEventArgs e)
        {
            // Hide all frames
            foreach (var state in _tabStates.Values)
            {
                state.Frame.Visibility = Visibility.Collapsed;
            }

            // Show the selected tab's frame
            if (DriveTabView.SelectedItem is TabViewItem selectedTab && _tabStates.TryGetValue(selectedTab, out var activeState))
            {
                _activeTab = activeState;
                activeState.Frame.Visibility = Visibility.Visible;
                UpdateNavigationButtons();
                UpdateNavBarVisibility();
            }
            else
            {
                _activeTab = null;
                UpdateNavigationButtons();
                UpdateNavBarVisibility();
            }
        }

        private void DriveTabView_TabCloseRequested(TabView sender, TabViewTabCloseRequestedEventArgs args)
        {
            if (args.Tab is TabViewItem tabItem && _tabStates.TryGetValue(tabItem, out var state))
            {
                // Don't close the fixed CloudPage tab
                if (state.IsFixed) return;

                state.Frame.Navigated -= TabFrame_Navigated;
                FrameContainer.Children.Remove(state.Frame);
                _tabStates.Remove(tabItem);
                DriveTabView.TabItems.Remove(tabItem);
            }
        }

        /// <summary>
        /// Navigate the active tab's frame to a page.
        /// Called by DriveHubPage/DrivePage.
        /// </summary>
        public void NavigateInner(Type pageType, object parameter = null)
        {
            _activeTab?.Frame.Navigate(pageType, parameter);
        }

        public Frame GetActiveFrame() => _activeTab?.Frame;

        private void BackButton_Click(object sender, RoutedEventArgs e)
        {
            if (_activeTab?.Frame.CanGoBack == true)
            {
                _activeTab.Frame.GoBack();
            }
        }

        private void ForwardButton_Click(object sender, RoutedEventArgs e)
        {
            if (_activeTab?.Frame.CanGoForward == true)
            {
                _activeTab.Frame.GoForward();
            }
        }

        private void RefreshButton_Click(object sender, RoutedEventArgs e)
        {
            if (_activeTab == null) return;

            if (_activeTab.Frame.Content is DrivePage drivePage)
            {
                var vm = drivePage.DataContext as DriveViewModel;
                vm?.RefreshCommand.Execute(null);
            }
            else if (_activeTab.Frame.Content is DriveHubPage)
            {
                _activeTab.Frame.Navigate(typeof(DriveHubPage), _activeTab.Drive);
            }
        }

        private void TabFrame_Navigated(object sender, NavigationEventArgs e)
        {
            if (_activeTab != null && sender == _activeTab.Frame)
            {
                UpdateNavigationButtons();
            }
        }

        private void UpdateNavigationButtons()
        {
            if (_activeTab != null && !_activeTab.IsFixed)
            {
                BackButton.IsEnabled = _activeTab.Frame.CanGoBack;
                ForwardButton.IsEnabled = _activeTab.Frame.CanGoForward;
            }
            else
            {
                BackButton.IsEnabled = false;
                ForwardButton.IsEnabled = false;
            }
        }

        /// <summary>
        /// Hide the navigation bar when the CloudPage (fixed) tab is active.
        /// </summary>
        private void UpdateNavBarVisibility()
        {
            NavBar.Visibility = (_activeTab != null && !_activeTab.IsFixed)
                ? Visibility.Visible
                : Visibility.Collapsed;
        }
    }
}
