using Microsoft.UI.Xaml.Controls;
using SimpleList.ViewModels;
using System.Linq;

namespace SimpleList.Views.Layout
{
    public sealed partial class ColumnCloudView : UserControl
    {
        public ColumnCloudView()
        {
            InitializeComponent();
        }

        private void ChangeSelectedFiles(object sender, SelectionChangedEventArgs e)
        {
            var drive = DataContext as DriveViewModel;
            if (drive?.SelectedItems == null) return;
            drive.SelectedItems.Clear();
            foreach (FileViewModel item in (sender as ListView).SelectedItems.Cast<FileViewModel>())
            {
                drive.SelectedItems.Add(item);
            }
            drive.UpdateSelectionState();
        }
    }
}
