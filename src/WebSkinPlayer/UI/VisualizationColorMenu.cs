using System.Runtime.InteropServices;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Interop;
using System.Windows.Media;
using WebSkinPlayer.Visualization;

namespace WebSkinPlayer.UI;

/// <summary>The colour choices for the visualization showing (<see cref="VisualizationColors"/>), for the skins' and
/// full mode's menus.</summary>
public static class VisualizationColorMenu
{
    /// <summary>A "Colors for ..." submenu.</summary>
    public static MenuItem Build(Window? owner)
    {
        var root = new MenuItem { Header = $"Colors for {App.Services.Visualizations.Current.Title}" };
        foreach (var item in Items(owner)) root.Items.Add(item);
        return root;
    }

    /// <summary>The choices themselves, for a menu of their own.</summary>
    public static List<Control> Items(Window? owner)
    {
        var vis = App.Services.Visualizations.Current;
        string current = VisualizationColors.Get(vis.Id);
        MenuItem Item(string header, string spec)
        {
            var mi = new MenuItem { Header = header, IsChecked = current.Equals(spec, StringComparison.OrdinalIgnoreCase) };
            mi.Click += (_, _) => VisualizationColors.Set(vis.Id, spec);
            return mi;
        }

        var items = new List<Control> { Item("Original Colors", ""), new Separator() };
        foreach (var (name, _) in VisualizationColors.Palettes) items.Add(Item(name, "palette:" + name));
        items.Add(new Separator());
        var shift = new MenuItem { Header = "Shift Colors", IsChecked = current.StartsWith("hue:", StringComparison.OrdinalIgnoreCase) };
        foreach (int degrees in VisualizationColors.HueShifts) shift.Items.Add(Item($"By {degrees}°", $"hue:{degrees}"));
        items.Add(shift);
        string? custom = current.StartsWith("custom:", StringComparison.OrdinalIgnoreCase) ? current[7..] : null;
        var pick = new MenuItem { Header = custom != null ? $"Custom Color ({custom})…" : "Custom Color…", IsChecked = custom != null };
        pick.Click += (_, _) =>
        {
            if (PickColor(owner, custom) is { } hex) VisualizationColors.Set(vis.Id, "custom:" + hex);
        };
        items.Add(pick);
        return items;
    }

    // ---- Windows' colour dialog --------------------------------------------------------------

    /// <summary>The dialog's 16 custom colour slots, kept for the session as Windows' own apps do.</summary>
    static readonly int[] CustomColors = Enumerable.Repeat(0xFFFFFF, 16).ToArray();

    /// <returns>"#RRGGBB", or null if cancelled.</returns>
    static string? PickColor(Window? owner, string? initial)
    {
        var slots = GCHandle.Alloc(CustomColors, GCHandleType.Pinned);
        try
        {
            var cc = new CHOOSECOLOR
            {
                lStructSize = Marshal.SizeOf<CHOOSECOLOR>(),
                hwndOwner = owner != null ? new WindowInteropHelper(owner).Handle : IntPtr.Zero,
                lpCustColors = slots.AddrOfPinnedObject(),
                Flags = 0x1 /* CC_RGBINIT */ | 0x2 /* CC_FULLOPEN */,
            };
            if (initial != null && ColorConverter.ConvertFromString(initial) is Color c) cc.rgbResult = c.R | c.G << 8 | c.B << 16;
            if (!ChooseColorW(ref cc)) return null;
            return $"#{cc.rgbResult & 255:X2}{cc.rgbResult >> 8 & 255:X2}{cc.rgbResult >> 16 & 255:X2}";
        }
        finally { slots.Free(); }
    }

    [StructLayout(LayoutKind.Sequential)]
    struct CHOOSECOLOR
    {
        public int lStructSize;
        public IntPtr hwndOwner, hInstance;
        public int rgbResult;
        public IntPtr lpCustColors;
        public int Flags;
        public IntPtr lCustData, lpfnHook, lpTemplateName;
    }

    [DllImport("comdlg32", CharSet = CharSet.Unicode)] static extern bool ChooseColorW(ref CHOOSECOLOR cc);
}
