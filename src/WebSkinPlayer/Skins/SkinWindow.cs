using System.Runtime.InteropServices;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Interop;
using System.Windows.Media;

namespace WebSkinPlayer.Skins;

/// <summary>Draws a view's element tree and forwards input to it.</summary>
public sealed class SkinSurface : FrameworkElement
{
    readonly SkinView _view;

    public SkinSurface(SkinView view)
    {
        _view = view;
        Focusable = true;
        SnapsToDevicePixels = true;
        HorizontalAlignment = HorizontalAlignment.Left;
        VerticalAlignment = VerticalAlignment.Top;
        TextOptions.SetTextFormattingMode(this, TextFormattingMode.Display);
        TextOptions.SetTextRenderingMode(this, TextRenderingMode.Grayscale);
        // Skins are pixel art made of abutting tiles; anti-aliased edges leave seams at 125%/150%.
        RenderOptions.SetEdgeMode(this, EdgeMode.Aliased);
        Loaded += (_, _) => UpdateScaling();
    }

    void UpdateScaling()
    {
        double s = VisualTreeHelper.GetDpi(this).DpiScaleX;
        bool integral = Math.Abs(s - Math.Round(s)) < 0.01;
        RenderOptions.SetBitmapScalingMode(this, integral ? BitmapScalingMode.NearestNeighbor : BitmapScalingMode.Linear);
    }

    protected override void OnDpiChanged(DpiScale oldDpi, DpiScale newDpi)
    {
        base.OnDpiChanged(oldDpi, newDpi);
        UpdateScaling();
        InvalidateVisual();
    }

    protected override void OnRender(DrawingContext dc)
    {
        // A fully transparent fill keeps the whole rectangle hit-testable inside WPF; the OS still
        // lets clicks through where the layered window's pixels are fully transparent.
        dc.DrawRectangle(Brushes.Transparent, null, new Rect(0, 0, Math.Max(1, _view.Width), Math.Max(1, _view.Height)));
        try { _view.RenderTree(dc); }
        catch (Exception ex) { Log.Write("Skin render error: " + ex); }
    }

    public Point PointToScreenDip(Point p)
    {
        if (PresentationSource.FromVisual(this) is not { } src) return p;
        var screen = PointToScreen(p);
        return src.CompositionTarget!.TransformFromDevice.Transform(screen);
    }

    protected override void OnMouseMove(MouseEventArgs e) { _view.HandleMouseMove(e.GetPosition(this)); }
    protected override void OnMouseLeave(MouseEventArgs e) { _view.HandleMouseLeave(); }

    protected override void OnMouseDown(MouseButtonEventArgs e)
    {
        Focus();
        var p = e.GetPosition(this);
        if (SkinView.DebugInput) Log.Write($"[input] surface down {e.ChangedButton} {e.ButtonState} at {p}");
        bool taken = _view.HandleMouseDown(p, e.ChangedButton, e.ClickCount);
        if (taken)
        {
            CaptureMouse();
            e.Handled = true;
            return;
        }
        if (e.ChangedButton == MouseButton.Left && e.ClickCount == 1)
        {
            e.Handled = true;
            _view.Window?.BeginMove();
        }
        else if (e.ChangedButton == MouseButton.Right)
        {
            e.Handled = true;
            _view.Theme.ShowAppMenu(_view, "context", p);
        }
    }

    protected override void OnMouseUp(MouseButtonEventArgs e)
    {
        if (IsMouseCaptured) ReleaseMouseCapture();
        _view.HandleMouseUp(e.GetPosition(this), e.ChangedButton);
    }

    protected override void OnLostMouseCapture(MouseEventArgs e)
    {
        base.OnLostMouseCapture(e);
    }

    protected override void OnMouseWheel(MouseWheelEventArgs e)
    {
        _view.HandleMouseWheel(e.GetPosition(this), e.Delta);
        e.Handled = true;
    }

    protected override Size MeasureOverride(Size availableSize) => new(Math.Max(1, _view.Width), Math.Max(1, _view.Height));
    protected override Size ArrangeOverride(Size finalSize) => new(Math.Max(1, _view.Width), Math.Max(1, _view.Height));
}

/// <summary>The top-level window for one open view.</summary>
public sealed class SkinWindow : Window
{
    readonly SkinView _view;
    readonly Grid _root = new();
    bool _syncing, _closingFromView;
    string PlacementKey => $"{System.IO.Path.GetFileName(_view.Theme.Path)}|{_view.Id ?? "main"}".ToLowerInvariant();

    public SkinWindow(SkinView view)
    {
        _view = view;
        Title = view.GetString("title");
        bool titleBar = view.GetBool("titlebar");
        if (titleBar)
        {
            WindowStyle = WindowStyle.SingleBorderWindow;
            ResizeMode = view.GetBool("resizable") ? ResizeMode.CanResize : ResizeMode.CanMinimize;
            Background = Brushes.Black;
        }
        else
        {
            WindowStyle = WindowStyle.None;
            AllowsTransparency = true;
            Background = Brushes.Transparent;
            ResizeMode = ResizeMode.CanMinimize;
        }
        ShowInTaskbar = true;
        SizeToContent = SizeToContent.WidthAndHeight;
        UseLayoutRounding = true;
        _root.Children.Add(view.Surface);
        Content = _root;
        _root.SizeChanged += OnRootSizeChanged;
        KeyDown += (_, e) => { if (_view.HandleKeyDown(e.Key == Key.System ? e.SystemKey : e.Key)) e.Handled = true; };
        KeyUp += (_, e) => _view.HandleKeyUp(e.Key);
        Activated += (_, _) => _view.Theme.ActiveView = _view;
        Closing += OnClosing;
        LocationChanged += (_, _) => SavePlacement();
        Topmost = App.Services.Settings.AlwaysOnTop;
        try { Icon = System.Windows.Media.Imaging.BitmapFrame.Create(new Uri("pack://application:,,,/app.ico")); } catch { }
        SourceInitialized += (_, _) =>
        {
            int on = 1;
            DwmSetWindowAttribute(new WindowInteropHelper(this).Handle, 20, ref on, 4);
        };
    }

    public void Place(double? x, double? y)
    {
        WindowStartupLocation = WindowStartupLocation.Manual;
        var area = SystemParameters.WorkArea;
        if (x.HasValue && y.HasValue && _view.Theme.ActiveView?.Window is { } opener && opener != this)
        {
            Left = opener.Left + x.Value;
            Top = opener.Top + y.Value;
            return;
        }
        if (App.Services.Settings.SkinWindows.TryGetValue(PlacementKey, out var p) && !double.IsNaN(p.Left))
        {
            Left = Math.Clamp(p.Left, area.Left - _view.Width + 40, area.Right - 40);
            Top = Math.Clamp(p.Top, area.Top, area.Bottom - 40);
            return;
        }
        Left = area.Left + Math.Max(0, (area.Width - _view.Width) / 2);
        Top = area.Top + Math.Max(0, (area.Height - _view.Height) / 2);
    }

    void SavePlacement()
    {
        if (!IsLoaded || WindowState != WindowState.Normal) return;
        App.Services.Settings.SkinWindows[PlacementKey] = new WindowPlacement { Left = Left, Top = Top, Width = _view.Width, Height = _view.Height };
    }

    /// <summary>The view's width/height changed (script or alignment): resize the window.</summary>
    public void SyncSizeFromView()
    {
        _syncing = true;
        _view.Surface.InvalidateMeasure();
        SizeToContent = SizeToContent.WidthAndHeight;
        UpdateLayout();
        _syncing = false;
    }

    void OnRootSizeChanged(object sender, SizeChangedEventArgs e)
    {
        if (_syncing || !IsLoaded || SizeToContent == SizeToContent.WidthAndHeight) return;
        // The user (or a view.size() drag) resized the window.
        int w = (int)Math.Round(_root.ActualWidth), h = (int)Math.Round(_root.ActualHeight);
        int minW = Math.Max(1, _view.GetInt("minwidth")), minH = Math.Max(1, _view.GetInt("minheight"));
        int maxW = _view.GetInt("maxwidth"), maxH = _view.GetInt("maxheight");
        w = Math.Max(minW, maxW > 0 ? Math.Min(maxW, w) : w);
        h = Math.Max(minH, maxH > 0 ? Math.Min(maxH, h) : h);
        _syncing = true;
        _view.SetValue("width", w);
        _view.SetValue("height", h);
        _view.Surface.InvalidateMeasure();
        _syncing = false;
    }

    public void BeginMove()
    {
        try { if (Mouse.LeftButton == MouseButtonState.Pressed) DragMove(); } catch (InvalidOperationException) { }
        SavePlacement();
    }

    /// <summary>view.size("bottomright") etc.: start a system resize loop from that edge.</summary>
    public void BeginResize(string direction)
    {
        int code = direction.ToLowerInvariant() switch
        {
            "left" => 1, "right" => 2, "top" => 3, "topleft" => 4, "topright" => 5,
            "bottom" => 6, "bottomleft" => 7, "bottomright" => 8, _ => 8,
        };
        var hwnd = new WindowInteropHelper(this).Handle;
        SizeToContent = SizeToContent.Manual;
        _view.Surface.ReleaseMouseCapture();
        ReleaseCapture();
        SendMessage(hwnd, 0x0112 /* WM_SYSCOMMAND */, (IntPtr)(0xF000 /* SC_SIZE */ + code), IntPtr.Zero);
        SizeToContent = SizeToContent.WidthAndHeight;
        SavePlacement();
    }

    void OnClosing(object? sender, System.ComponentModel.CancelEventArgs e)
    {
        SavePlacement();
        if (_closingFromView) return;
        // Closed by the user (Alt+F4 / title bar): treat like view.close().
        e.Cancel = true;
        Dispatcher.InvokeAsync(() => _view.Theme.CloseView(_view));
    }

    public void CloseFromView()
    {
        _closingFromView = true;
        Close();
    }

    [DllImport("user32")] static extern IntPtr SendMessage(IntPtr hwnd, int msg, IntPtr w, IntPtr l);
    [DllImport("user32")] static extern bool ReleaseCapture();
    [DllImport("dwmapi")] static extern int DwmSetWindowAttribute(IntPtr hwnd, int attr, ref int value, int size);
}
