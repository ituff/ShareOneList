using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Data;
using System;

namespace SimpleList.Converters;

/// <summary>
/// Converts a boolean (IsSelectionMode) to ListViewSelectionMode.
/// true → Multiple (shows checkboxes), false → Extended (default multi-select with Ctrl/Shift).
/// </summary>
public class BoolToSelectionModeConverter : IValueConverter
{
    public object Convert(object value, Type targetType, object parameter, string language)
    {
        return value is true ? ListViewSelectionMode.Multiple : ListViewSelectionMode.Extended;
    }

    public object ConvertBack(object value, Type targetType, object parameter, string language)
    {
        return value is ListViewSelectionMode.Multiple;
    }
}
