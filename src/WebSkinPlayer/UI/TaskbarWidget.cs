using System.Runtime.InteropServices;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Interop;
using System.Windows.Media;
using System.Windows.Threading;

namespace WebSkinPlayer.UI;

/// <summary>
/// Docks <see cref="WidgetContent"/> inside the Windows taskbar, the way WMP's taskbar mini
/// player (and apps like FluentFlyout) did: a child window of Shell_TrayWnd so it moves and
/// auto-hides with the taskbar. Falls back to a topmost overlay if the taskbar refuses a child.
/// </summary>
public sealed class TaskbarWidget : IDisposable
{
    const double WidthDip = 300;
    const int WS_CHILD = 0x40000000, WS_VISIBLE = 0x10000000, WS_CLIPSIBLINGS = 0x04000000;

    readonly WidgetContent _content = new();
    readonly DispatcherTimer _timer = new(DispatcherPriority.Background) { Interval = TimeSpan.FromSeconds(1) };
    Window? _overlay;
    IntPtr _taskbar;
    bool _dragging;
    Point _dragStart;
    double _dragOrigin;

    public bool IsShown { get; private set; }
    public event Action? OpenRequested;

    static Services S => App.Services;

    public TaskbarWidget()
    {
        _content.OpenRequested += () => OpenRequested?.Invoke();
        _content.MouseRightButtonUp += (_, e) => { ShowMenu(); e.Handled = true; };
        _content.PreviewMouseLeftButtonDown += OnDragStart;
        _content.PreviewMouseMove += OnDragMove;
        _content.PreviewMouseLeftButtonUp += OnDragEnd;
        _timer.Tick += (_, _) => Maintain();
        Microsoft.Win32.SystemEvents.UserPreferenceChanged += OnThemeChanged;
    }

    void OnThemeChanged(object? s, Microsoft.Win32.UserPreferenceChangedEventArgs e) =>
        _content.Dispatcher.InvokeAsync(() => _content.ApplyTheme(TaskbarTheme.IsLight));

    public void Show()
    {
        if (IsShown) return;
        IsShown = true;
        Attach();
        _timer.Start();
    }

    public void Hide()
    {
        if (!IsShown) return;
        IsShown = false;
        _timer.Stop();
        Detach();
    }

    /// <summary>
    /// A transparent (layered) WPF window, created top-level and then re-parented into the
    /// taskbar as a WS_CHILD. If the taskbar can't be found or refuses the child, the same
    /// window simply stays a topmost overlay positioned over the taskbar.
    /// </summary>
    void Attach()
    {
        _taskbar = FindWindow("Shell_TrayWnd", null);
        var host = new Window
        {
            Title = "WebSkin Player taskbar widget",
            WindowStyle = WindowStyle.None, AllowsTransparency = true, Background = Brushes.Transparent,
            ShowInTaskbar = false, Topmost = true, ResizeMode = ResizeMode.NoResize, ShowActivated = false,
            Left = -32000, Top = -32000, Width = WidthDip, Height = 48,
            Content = Frame(),
        };
        host.SourceInitialized += (_, _) =>
        {
            var hwnd = new WindowInteropHelper(host).Handle;
            SetWindowLong(hwnd, -20, GetWindowLong(hwnd, -20) | 0x80 /* WS_EX_TOOLWINDOW */ | 0x08000000 /* WS_EX_NOACTIVATE */);
            if (_taskbar == IntPtr.Zero) return;
            int style = GetWindowLong(hwnd, -16);
            SetWindowLong(hwnd, -16, (style & ~unchecked((int)0x80000000) /* WS_POPUP */) | WS_CHILD | WS_CLIPSIBLINGS);
            _docked = SetParent(hwnd, _taskbar) != IntPtr.Zero;
            Log.Write(_docked ? "Taskbar widget docked into the taskbar" : "Taskbar widget couldn't dock; using an overlay");
        };
        _overlay = host;
        host.Show();
        Position();
    }

    bool _docked;

    FrameworkElement Frame()
    {
        if (_content.Parent is Decorator old) old.Child = null;
        // Alpha 1/255: invisible, but keeps the whole pill clickable in a layered window.
        return new Border { Child = _content, Background = new SolidColorBrush(Color.FromArgb(1, 0, 0, 0)) };
    }

    void Detach()
    {
        if (_overlay != null)
        {
            if (_overlay.Content is Border b) b.Child = null;
            _overlay.Close();
            _overlay = null;
        }
        _docked = false;
    }

    double Scale
    {
        get
        {
            uint dpi = _taskbar != IntPtr.Zero ? GetDpiForWindow(_taskbar) : 96;
            return (dpi == 0 ? 96 : dpi) / 96.0;
        }
    }

    /// <summary>Widget rect in taskbar-client pixels (child) or screen pixels (overlay).</summary>
    (int x, int y, int w, int h) Layout()
    {
        GetWindowRect(_taskbar, out var tb);
        double s = Scale;
        int tbW = tb.Right - tb.Left, tbH = tb.Bottom - tb.Top;
        bool vertical = tbH > tbW;
        int h = vertical ? (int)(48 * s) : tbH;
        int w = (int)(WidthDip * s);
        int x;
        double saved = S.Settings.WidgetOffset;
        if (!double.IsNaN(saved)) x = (int)(saved * s);
        // With left-aligned taskbar icons the left edge is taken, so default to the tray side.
        else if (S.Settings.WidgetSide == "right" || !TaskbarIconsCentered) x = DefaultRightX(tb, w, s);
        else x = (int)(12 * s + (WidgetsButtonShown ? 190 * s : 0));
        x = Math.Clamp(x, 0, Math.Max(0, tbW - w));
        if (!_docked) return (tb.Left + x, tb.Top, w, h);
        return (x, 0, w, h);
    }

    int DefaultRightX(RECT tb, int w, double s)
    {
        // Just left of the notification area when we can find it.
        IntPtr tray = FindWindowEx(_taskbar, IntPtr.Zero, "TrayNotifyWnd", null);
        if (tray != IntPtr.Zero && GetWindowRect(tray, out var tr) && tr.Right - tr.Left > 20)
            return tr.Left - tb.Left - w - (int)(8 * s);
        return tb.Right - tb.Left - w - (int)(260 * s);
    }

    static bool TaskbarIconsCentered => RegInt(@"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced", "TaskbarAl", 1) != 0;
    static bool WidgetsButtonShown => RegInt(@"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced", "TaskbarDa", 1) != 0;

    static int RegInt(string key, string name, int fallback)
    {
        try { using var k = Microsoft.Win32.Registry.CurrentUser.OpenSubKey(key); return k?.GetValue(name) is int v ? v : fallback; }
        catch { return fallback; }
    }

    void Position()
    {
        var (x, y, w, h) = Layout();
        double s = Scale;
        _content.Width = w / s;
        _content.Height = h / s;
        if (_overlay == null) return;
        var hwnd = new WindowInteropHelper(_overlay).Handle;
        if (Math.Abs(_overlay.Width - w / s) > 0.5) _overlay.Width = w / s;
        if (Math.Abs(_overlay.Height - h / s) > 0.5) _overlay.Height = h / s;
        // Docked: client coordinates of the taskbar, top of its child z-order. Overlay: topmost.
        SetWindowPos(hwnd, _docked ? IntPtr.Zero : new IntPtr(-1), x, y, w, h, 0x0010 /* NOACTIVATE */ | 0x0040 /* SHOWWINDOW */);
    }

    /// <summary>Once a second: re-dock after an Explorer restart and keep above the taskbar's own content.</summary>
    void Maintain()
    {
        if (!IsShown || _dragging) return;
        IntPtr tb = FindWindow("Shell_TrayWnd", null);
        IntPtr hwnd = _overlay != null ? new WindowInteropHelper(_overlay).Handle : IntPtr.Zero;
        bool lost = _docked && (tb != _taskbar || !IsWindow(_taskbar) || GetParent(hwnd) != _taskbar);
        if (lost || _overlay == null || (!_docked && tb != IntPtr.Zero && _taskbar == IntPtr.Zero))
        {
            Detach();
            if (tb != IntPtr.Zero) Attach();
            return;
        }
        Position();
    }

    // ---- dragging along the taskbar ------------------------------------------------------

    void OnDragStart(object sender, MouseButtonEventArgs e)
    {
        if (e.OriginalSource is DependencyObject d && FindButton(d) != null) return;
        _dragStart = PointToScreen(e);
        GetWindowRect(_taskbar, out var tb);
        var (x, _, _, _) = Layout();
        _dragOrigin = _docked ? x : x - tb.Left;
        _dragging = false;
        _content.CaptureMouse();
    }

    void OnDragMove(object sender, MouseEventArgs e)
    {
        if (!_content.IsMouseCaptured || e.LeftButton != MouseButtonState.Pressed) return;
        var p = PointToScreen(e);
        double dx = p.X - _dragStart.X;
        if (!_dragging && Math.Abs(dx) < 6) return;
        _dragging = true;
        S.Settings.WidgetOffset = (_dragOrigin + dx) / Scale;
        Position();
    }

    void OnDragEnd(object sender, MouseButtonEventArgs e)
    {
        if (!_content.IsMouseCaptured) return;
        _content.ReleaseMouseCapture();
        if (_dragging)
        {
            _dragging = false;
            e.Handled = true; // don't treat the drag as a click
            S.Settings.Save();
        }
    }

    static Point PointToScreen(MouseEventArgs e)
    {
        GetCursorPos(out var pt);
        return new Point(pt.X, pt.Y);
    }

    static Button? FindButton(DependencyObject d)
    {
        for (var x = d; x != null; x = VisualTreeHelper.GetParent(x) ?? LogicalTreeHelper.GetParent(x))
            if (x is Button b) return b;
        return null;
    }

    // ---- menu ------------------------------------------------------------------------------

    void ShowMenu()
    {
        var menu = new ContextMenu();
        MenuItem Item(string header, Action a, bool? check = null)
        {
            var mi = new MenuItem { Header = header };
            if (check.HasValue) { mi.IsCheckable = true; mi.IsChecked = check.Value; }
            mi.Click += (_, _) => a();
            return mi;
        }
        menu.Items.Add(Item("Open player", () => OpenRequested?.Invoke()));
        menu.Items.Add(Item("Skin mode\tCtrl+2", () => App.Modes.RestoreFromWidget(skin: true)));
        menu.Items.Add(Item("Full mode\tCtrl+1", () => App.Modes.RestoreFromWidget(skin: false)));
        menu.Items.Add(new Separator());
        var pos = new MenuItem { Header = "Position" };
        pos.Items.Add(Item("Left side", () => { S.Settings.WidgetSide = "left"; S.Settings.WidgetOffset = double.NaN; Position(); S.Settings.Save(); }));
        pos.Items.Add(Item("Next to the system tray", () => { S.Settings.WidgetSide = "right"; S.Settings.WidgetOffset = double.NaN; Position(); S.Settings.Save(); }));
        pos.Items.Add(new MenuItem { Header = "Tip: drag the widget to move it", IsEnabled = false });
        menu.Items.Add(pos);
        menu.Items.Add(Item("Keep widget on the taskbar", App.Modes.ToggleWidgetAlwaysOn, S.Settings.WidgetAlwaysOn));
        menu.Items.Add(Item("Hide widget", () => App.Modes.HideWidget()));
        menu.Items.Add(new Separator());
        menu.Items.Add(Item("Exit", App.Modes.Exit));
        menu.Placement = System.Windows.Controls.Primitives.PlacementMode.MousePoint;
        menu.IsOpen = true;
    }

    public void Dispose()
    {
        Microsoft.Win32.SystemEvents.UserPreferenceChanged -= OnThemeChanged;
        Hide();
    }

    [StructLayout(LayoutKind.Sequential)] struct RECT { public int Left, Top, Right, Bottom; }
    [StructLayout(LayoutKind.Sequential)] struct POINT { public int X, Y; }
    [DllImport("user32", CharSet = CharSet.Unicode)] static extern IntPtr FindWindow(string cls, string? name);
    [DllImport("user32", CharSet = CharSet.Unicode)] static extern IntPtr FindWindowEx(IntPtr parent, IntPtr after, string cls, string? name);
    [DllImport("user32")] static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32")] static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int w, int hh, uint flags);
    [DllImport("user32")] static extern bool IsWindow(IntPtr h);
    [DllImport("user32")] static extern IntPtr GetParent(IntPtr h);
    [DllImport("user32")] static extern IntPtr SetParent(IntPtr child, IntPtr parent);
    [DllImport("user32")] static extern uint GetDpiForWindow(IntPtr h);
    [DllImport("user32")] static extern bool GetCursorPos(out POINT p);
    [DllImport("user32")] static extern int GetWindowLong(IntPtr h, int i);
    [DllImport("user32")] static extern int SetWindowLong(IntPtr h, int i, int v);
}
