using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Media.Effects;
using WebSkinPlayer.Audio;
using WebSkinPlayer.Media;

namespace WebSkinPlayer.UI;

/// <summary>
/// What the taskbar widget shows: artwork, title/artist, a tiny live spectrum (WMP's taskbar
/// mini player had a little visualization too) and previous / play-pause / next.
/// </summary>
public sealed class WidgetContent : Border
{
    readonly Border _art = new() { Width = 30, Height = 30, CornerRadius = new CornerRadius(4), Margin = new Thickness(6, 0, 8, 0), ClipToBounds = true };
    readonly TextBlock _title = new() { FontSize = 12, FontWeight = FontWeights.SemiBold, TextTrimming = TextTrimming.CharacterEllipsis };
    readonly TextBlock _artist = new() { FontSize = 11, TextTrimming = TextTrimming.CharacterEllipsis };
    readonly MiniSpectrum _spectrum = new() { Width = 26, Height = 20, Margin = new Thickness(6, 0, 2, 0), VerticalAlignment = VerticalAlignment.Center };
    readonly Button _prev, _play, _next;
    bool _light;

    public event Action? OpenRequested;

    static Services S => App.Services;

    public WidgetContent()
    {
        CornerRadius = new CornerRadius(6);
        Background = Brushes.Transparent;
        Padding = new Thickness(2, 0, 4, 0);
        SnapsToDevicePixels = true;
        UseLayoutRounding = true;
        TextOptions.SetTextFormattingMode(this, TextFormattingMode.Display);

        var text = new StackPanel { VerticalAlignment = VerticalAlignment.Center, Margin = new Thickness(0, 0, 4, 0) };
        text.Children.Add(_title);
        text.Children.Add(_artist);

        _prev = IconButton("", "Previous", () => S.Player.Previous());
        _play = IconButton("", "Play", () => S.Player.TogglePlayPause(), big: true);
        _next = IconButton("", "Next", () => S.Player.Next());

        var grid = new Grid();
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        var buttons = new StackPanel { Orientation = Orientation.Horizontal, VerticalAlignment = VerticalAlignment.Center };
        buttons.Children.Add(_prev);
        buttons.Children.Add(_play);
        buttons.Children.Add(_next);
        Grid.SetColumn(_art, 0);
        Grid.SetColumn(text, 1);
        Grid.SetColumn(_spectrum, 2);
        Grid.SetColumn(buttons, 3);
        grid.Children.Add(_art);
        grid.Children.Add(text);
        grid.Children.Add(_spectrum);
        grid.Children.Add(buttons);
        Child = grid;

        MouseEnter += (_, _) => Background = new SolidColorBrush(_light ? Color.FromArgb(0x14, 0, 0, 0) : Color.FromArgb(0x14, 255, 255, 255));
        MouseLeave += (_, _) => Background = Brushes.Transparent;
        MouseWheel += (_, e) => { S.Player.Volume += e.Delta > 0 ? 5 : -5; ToolTip = $"Volume {S.Player.Volume}%"; e.Handled = true; };
        Loaded += (_, _) => { S.Media.Changed += Refresh; S.Visualizations.Subscribe(_spectrum.OnFrame); Refresh(); };
        Unloaded += (_, _) => { S.Media.Changed -= Refresh; S.Visualizations.Unsubscribe(_spectrum.OnFrame); };
        ApplyTheme(TaskbarTheme.IsLight);
    }

    Button IconButton(string glyph, string tip, Action click, bool big = false)
    {
        var b = new Button
        {
            Content = glyph,
            ToolTip = tip,
            Width = big ? 34 : 30,
            Height = 32,
            FontFamily = new FontFamily("Segoe Fluent Icons, Segoe MDL2 Assets"),
            FontSize = big ? 16 : 13,
            Focusable = false,
            Cursor = Cursors.Hand,
        };
        b.Template = ButtonTemplate();
        b.Click += (_, e) => { click(); e.Handled = true; };
        return b;
    }

    static ControlTemplate ButtonTemplate()
    {
        var t = new ControlTemplate(typeof(Button));
        var border = new FrameworkElementFactory(typeof(Border), "b");
        border.SetValue(Border.CornerRadiusProperty, new CornerRadius(4));
        border.SetValue(Border.BackgroundProperty, Brushes.Transparent);
        var cp = new FrameworkElementFactory(typeof(ContentPresenter));
        cp.SetValue(HorizontalAlignmentProperty, HorizontalAlignment.Center);
        cp.SetValue(VerticalAlignmentProperty, VerticalAlignment.Center);
        border.AppendChild(cp);
        t.VisualTree = border;
        var hover = new Trigger { Property = UIElement.IsMouseOverProperty, Value = true };
        hover.Setters.Add(new Setter(Border.BackgroundProperty, new SolidColorBrush(Color.FromArgb(0x22, 128, 128, 128)), "b"));
        var pressed = new Trigger { Property = System.Windows.Controls.Primitives.ButtonBase.IsPressedProperty, Value = true };
        pressed.Setters.Add(new Setter(Border.BackgroundProperty, new SolidColorBrush(Color.FromArgb(0x38, 128, 128, 128)), "b"));
        var disabled = new Trigger { Property = UIElement.IsEnabledProperty, Value = false };
        disabled.Setters.Add(new Setter(UIElement.OpacityProperty, 0.35));
        t.Triggers.Add(hover);
        t.Triggers.Add(pressed);
        t.Triggers.Add(disabled);
        return t;
    }

    public void ApplyTheme(bool light)
    {
        _light = light;
        var fg = light ? Color.FromRgb(0x1A, 0x1A, 0x1A) : Color.FromRgb(0xF3, 0xF3, 0xF3);
        var dim = light ? Color.FromRgb(0x5C, 0x5C, 0x5C) : Color.FromRgb(0xB8, 0xB8, 0xB8);
        _title.Foreground = new SolidColorBrush(fg);
        _artist.Foreground = new SolidColorBrush(dim);
        foreach (var b in new[] { _prev, _play, _next }) b.Foreground = new SolidColorBrush(fg);
        _spectrum.Color = TaskbarTheme.Accent;
    }

    protected override void OnMouseLeftButtonUp(MouseButtonEventArgs e)
    {
        base.OnMouseLeftButtonUp(e);
        if (!e.Handled) OpenRequested?.Invoke();
    }

    void Refresh()
    {
        var m = S.Media.Current;
        _title.Text = m.HasSession && m.Title.Length > 0 ? m.Title : "WebSkin Player";
        _artist.Text = m.HasSession ? (m.Artist.Length > 0 ? m.Artist : m.SourceDisplayName) : "Nothing playing";
        bool playing = m.Status == MediaStatus.Playing;
        _play.Content = playing ? "" : "";
        _play.ToolTip = playing ? "Pause" : "Play";
        _play.IsEnabled = m.HasSession;
        _prev.IsEnabled = m.CanPrevious;
        _next.IsEnabled = m.CanNext;
        _spectrum.Visibility = playing ? Visibility.Visible : Visibility.Collapsed;
        ToolTip = m.HasSession ? $"{m.Title}{(m.Artist.Length > 0 ? " — " + m.Artist : "")}\n{m.SourceDisplayName} · scroll for volume" : "Open SoundCloud or Spotify in your browser";

        if (m.Artwork != null)
            _art.Background = new ImageBrush(m.Artwork) { Stretch = Stretch.UniformToFill };
        else
            _art.Background = new LinearGradientBrush(Color.FromRgb(0x2B, 0x5D, 0x9E), Color.FromRgb(0x0E, 0x22, 0x40), 45);
        _art.Child = m.Artwork == null
            ? new TextBlock { Text = "", FontFamily = new FontFamily("Segoe Fluent Icons, Segoe MDL2 Assets"), FontSize = 14, Foreground = Brushes.White, HorizontalAlignment = HorizontalAlignment.Center, VerticalAlignment = VerticalAlignment.Center }
            : null;
        _art.Effect = new DropShadowEffect { BlurRadius = 4, ShadowDepth = 1, Opacity = 0.35 };
    }
}

/// <summary>A few-bar spectrum drawn straight from the loopback analysis, ~30 fps.</summary>
public sealed class MiniSpectrum : FrameworkElement
{
    const int Bars = 7;
    readonly float[] _levels = new float[Bars];
    long _lastTick;
    public Color Color { get; set; } = Color.FromRgb(0x3F, 0xA9, 0xF5);

    public void OnFrame(VisFrame f)
    {
        long now = Environment.TickCount64;
        if (now - _lastTick < 33 || Visibility != Visibility.Visible) return;
        _lastTick = now;
        double binHz = f.SampleRate / 2048.0;
        for (int b = 0; b < Bars; b++)
        {
            double f0 = 60 * Math.Pow(12000 / 60.0, b / (double)Bars), f1 = 60 * Math.Pow(12000 / 60.0, (b + 1) / (double)Bars);
            int i0 = Math.Clamp((int)(f0 / binHz), 1, VisFrame.Bins - 1), i1 = Math.Clamp((int)(f1 / binHz) + 1, i0 + 1, VisFrame.Bins);
            float v = 0;
            for (int i = i0; i < i1; i++) v = MathF.Max(v, f.Spectrum[i]);
            v = MathF.Pow(v, 1.6f);
            _levels[b] = v > _levels[b] ? v : _levels[b] * 0.8f + v * 0.2f;
        }
        InvalidateVisual();
    }

    protected override void OnRender(DrawingContext dc)
    {
        double w = ActualWidth, h = ActualHeight;
        double bw = w / Bars;
        var brush = new SolidColorBrush(Color);
        for (int b = 0; b < Bars; b++)
        {
            double bh = Math.Max(2, _levels[b] * h);
            dc.DrawRoundedRectangle(brush, null, new Rect(b * bw + 0.5, h - bh, Math.Max(1, bw - 1.5), bh), 1, 1);
        }
    }
}

/// <summary>Taskbar light/dark mode and the Windows accent colour.</summary>
public static class TaskbarTheme
{
    public static bool IsLight
    {
        get
        {
            try
            {
                using var k = Microsoft.Win32.Registry.CurrentUser.OpenSubKey(@"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
                return k?.GetValue("SystemUsesLightTheme") is int v && v == 1;
            }
            catch { return false; }
        }
    }

    public static Color Accent
    {
        get
        {
            try
            {
                using var k = Microsoft.Win32.Registry.CurrentUser.OpenSubKey(@"Software\Microsoft\Windows\DWM");
                if (k?.GetValue("AccentColor") is int abgr)
                {
                    var c = Color.FromRgb((byte)abgr, (byte)(abgr >> 8), (byte)(abgr >> 16));
                    // Keep it readable on the dark taskbar.
                    if (!IsLight && c.R + c.G + c.B < 200) c = Color.FromRgb((byte)Math.Min(255, c.R + 90), (byte)Math.Min(255, c.G + 90), (byte)Math.Min(255, c.B + 90));
                    return c;
                }
            }
            catch { }
            return Color.FromRgb(0x3F, 0xA9, 0xF5);
        }
    }
}
