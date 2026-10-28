using System.Runtime.InteropServices;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Interop;
using System.Windows.Media;
using System.Windows.Media.Animation;
using System.Windows.Media.Effects;
using System.Windows.Media.Imaging;

namespace WebSkinPlayer.UI;

/// <summary>One monitor of the visualizer screen saver: the shared visualizer picture, stretched to fill the
/// screen, with the track and visualization names fading in now and then.</summary>
public sealed class ScreenSaverWindow : Window
{
    readonly Image _image = new() { Stretch = Stretch.Fill };
    readonly Border _fade = new() { Background = Brushes.Black, Opacity = 0, IsHitTestVisible = false };
    readonly StackPanel _info = new() { HorizontalAlignment = HorizontalAlignment.Left, VerticalAlignment = VerticalAlignment.Bottom, Margin = new Thickness(56, 0, 56, 48), Opacity = 0 };
    readonly TextBlock _title = Line(30, FontWeights.SemiBold, Colors.White);
    readonly TextBlock _artist = Line(20, FontWeights.Normal, Color.FromRgb(0xB9, 0xC4, 0xD2));
    readonly TextBlock _visName = Line(13, FontWeights.Normal, Color.FromRgb(0x8B, 0x95, 0xA3));
    readonly Int32Rect _monitor;

    public ScreenSaverWindow(Int32Rect monitor)
    {
        _monitor = monitor;
        Title = "WebSkin Player Screen Saver";
        WindowStyle = WindowStyle.None;
        ResizeMode = ResizeMode.NoResize;
        ShowInTaskbar = false;
        Topmost = true;
        Background = Brushes.Black;
        Cursor = Cursors.None;
        RenderOptions.SetBitmapScalingMode(_image, BitmapScalingMode.Linear);
        _visName.Margin = new Thickness(0, 10, 0, 0);
        _info.Children.Add(_title);
        _info.Children.Add(_artist);
        _info.Children.Add(_visName);
        var root = new Grid();
        root.Children.Add(_image);
        root.Children.Add(_info);
        root.Children.Add(_fade);
        Content = root;
        // Move onto its monitor in physical pixels, then maximize there: a borderless maximized window
        // covers the whole monitor, taskbar included, at whatever DPI that monitor uses.
        SourceInitialized += (_, _) =>
        {
            SetWindowPos(new WindowInteropHelper(this).Handle, IntPtr.Zero, _monitor.X + 8, _monitor.Y + 8, 320, 240, 0x0014 /* NOZORDER|NOACTIVATE */);
            WindowState = WindowState.Maximized;
        };
    }

    static TextBlock Line(double size, FontWeight weight, Color color) => new()
    {
        FontFamily = new FontFamily("Segoe UI"), FontSize = size, FontWeight = weight, Foreground = new SolidColorBrush(color),
        TextTrimming = TextTrimming.CharacterEllipsis, MaxWidth = 1400,
        Effect = new DropShadowEffect { BlurRadius = 8, ShadowDepth = 0, Opacity = 0.95 },
    };

    public void SetBitmap(BitmapSource? bitmap) => _image.Source = bitmap;

    public void ShowTrack(string title, string artist)
    {
        _title.Text = title;
        _artist.Text = artist;
        _title.Visibility = title.Length > 0 ? Visibility.Visible : Visibility.Collapsed;
        _artist.Visibility = artist.Length > 0 ? Visibility.Visible : Visibility.Collapsed;
        ShowInfo(10);
    }

    public void SetVisualizationName(string name)
    {
        _visName.Text = name;
        ShowInfo(6);
    }

    /// <summary>Fades the names in, holds them for <paramref name="seconds"/>, and fades them out.</summary>
    void ShowInfo(double seconds)
    {
        var a = new DoubleAnimationUsingKeyFrames();
        a.KeyFrames.Add(new LinearDoubleKeyFrame(1, KeyTime.FromTimeSpan(TimeSpan.FromSeconds(0.6))));
        a.KeyFrames.Add(new LinearDoubleKeyFrame(1, KeyTime.FromTimeSpan(TimeSpan.FromSeconds(seconds))));
        a.KeyFrames.Add(new LinearDoubleKeyFrame(0, KeyTime.FromTimeSpan(TimeSpan.FromSeconds(seconds + 1.2))));
        _info.BeginAnimation(OpacityProperty, a);
    }

    /// <summary>Fades this and the other screens to black, runs <paramref name="swap"/>, and fades back.</summary>
    public void FadeThrough(Action swap, IEnumerable<ScreenSaverWindow> others)
    {
        var all = others.Prepend(this).ToList();
        foreach (var w in all)
        {
            var toBlack = new DoubleAnimation(1, TimeSpan.FromMilliseconds(400));
            if (w == this)
                toBlack.Completed += (_, _) =>
                {
                    swap();
                    foreach (var x in all) x._fade.BeginAnimation(OpacityProperty, new DoubleAnimation(0, TimeSpan.FromMilliseconds(500)));
                };
            w._fade.BeginAnimation(OpacityProperty, toBlack);
        }
    }

    [DllImport("user32")] static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int w, int hh, uint flags);
}
