using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using SimpleList.Models;
using SimpleList.Services;
using System.Threading.Tasks;

namespace SimpleList.ViewModels
{
    public partial class CreateDriveViewModel : ObservableObject
    {
        public CreateDriveViewModel(CloudViewModel cloud)
        {
            _cloud = cloud;
        }

        [RelayCommand]
        public async Task CreateDrive()
        {
            OneDrive drive = new(SelectedCloudType);
            await drive.Login();
            if (drive.IsAuthenticated)
            {
                DriveViewModel driveViewModel = new(drive, DisplayName);
                _cloud.AddDrive(driveViewModel);
            }
        }

        private readonly CloudViewModel _cloud;
        [ObservableProperty] private string _displayName;
        [ObservableProperty] private CloudType _selectedCloudType = CloudType.China;

        public bool IsChina
        {
            get => SelectedCloudType == CloudType.China;
            set { if (value) SelectedCloudType = CloudType.China; }
        }

        public bool IsGlobal
        {
            get => SelectedCloudType == CloudType.Global;
            set { if (value) SelectedCloudType = CloudType.Global; }
        }
    }
}
