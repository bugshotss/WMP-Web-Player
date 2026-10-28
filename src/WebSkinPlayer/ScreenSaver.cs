using System.Runtime.InteropServices;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Threading;
using WebSkinPlayer.Audio;
using WebSkinPlayer.UI;
using WebSkinPlayer.Visualization;

namespace WebSkinPlayer;

/// <summary>
/// The visualizer screen saver: once the PC has been idle for a while, every monitor shows a random
/// visualization (changing every so often) with the current track, until the mouse or keyboard is used.
/// It can lock Windows when it closes. Windows itself can't draw on the real lock screen, so this
/// takes the place of the classic WMP-era screen saver instead.
/// </summary>
public sealed class ScreenSaver
{
    static Services S => App.Services;

    readonly DispatcherTimer _idleTimer = new() { Interval = TimeSpan.FromSeconds(2) };
    readonly DispatcherTimer _inputTimer = new() { Interval = TimeSpan.FromMilliseconds(100) };
    readonly DispatcherTimer _rotateTimer = new();
    readonly Random _random = new();
    readonly List<ScreenSaverWindow> _windows = new();
    readonly VisualizerRenderer _renderer = new() { MaxPixels = 1280 * 720 };
    (string Id, int Preset) _current = ("", -1);
    (int Width, int Height) _renderSize = (1280, 720);
    string _currentName = "";
    uint _inputBaseline;
    POINT _cursorStart;
    DateTime _startedAt;
    bool _sessionLocked;

    public bool IsRunning => _windows.Count > 0;
    /// <summary>The visualization and preset on screen, e.g. "Battery: cottonstar".</summary>
    public string ShowingName => _currentName;
    /// <summary>What decides an automatic start, for the test hooks.</summary>
    public string Diagnostics => $"enabled={Enabled} idle={IdleMilliseconds() / 1000}s of {IdleMinutes * 60}s screenNeeded={SomethingNeedsTheScreen()} locked={_sessionLocked}";
    /// <summary>Shows one particular visualization (for the test hooks).</summary>
    public void Show(string id, int preset)
    {
        if (!IsRunning || VisualizationCatalog.Find(id) is not { } effect) return;
        _current = (effect.Id, preset);
        _renderer.SetEffect(effect, preset);
        _currentName = effect.Title;
        foreach (var w in _windows) w.SetVisualizationName(_currentName);
    }

    /// <summary>Switches to another random visualization right away.</summary>
    public void Next() { if (IsRunning) { _rotateTimer.Stop(); Rotate(); _rotateTimer.Start(); } }

    public ScreenSaver()
    {
        _idleTimer.Tick += (_, _) => CheckIdle();
        _inputTimer.Tick += (_, _) => CheckInput();
        _rotateTimer.Tick += (_, _) => Rotate();
        _renderer.BitmapChanged += () => { foreach (var w in _windows) w.SetBitmap(_renderer.Bitmap); };
        Microsoft.Win32.SystemEvents.SessionSwitch += OnSessionSwitch;
        _idleTimer.Start();
    }

    // ---- settings ------------------------------------------------------------------------

    public static readonly int[] IdleChoices = { 1, 2, 3, 5, 10, 15, 20, 30 };
    public static readonly int[] ChangeChoices = { 15, 30, 60, 120, 300 };

    public bool Enabled
    {
        get => S.Settings.ScreenSaverEnabled;
        set { S.Settings.ScreenSaverEnabled = value; S.Settings.Save(); }
    }

    public int IdleMinutes
    {
        get => Math.Clamp(S.Settings.ScreenSaverIdleMinutes, 1, 120);
        set { S.Settings.ScreenSaverIdleMinutes = value; S.Settings.Save(); }
    }

    public int ChangeSeconds
    {
        get => Math.Clamp(S.Settings.ScreenSaverChangeSeconds, 5, 3600);
        set { S.Settings.ScreenSaverChangeSeconds = value; S.Settings.Save(); if (IsRunning) _rotateTimer.Interval = TimeSpan.FromSeconds(ChangeSeconds); }
    }

    public bool LocksWhenClosed
    {
        get => S.Settings.ScreenSaverLocks;
        set { S.Settings.ScreenSaverLocks = value; S.Settings.Save(); }
    }

    /// <summary>The "Screen Saver" submenu shared by the visualizer, skin and taskbar widget menus.</summary>
    public MenuItem BuildMenu()
    {
        var root = new MenuItem { Header = "Screen Saver" };
        MenuItem Item(string header, Action a, bool? check = null, bool enabled = true)
        {
            var mi = new MenuItem { Header = header, IsEnabled = enabled };
            if (check.HasValue) { mi.IsCheckable = true; mi.IsChecked = check.Value; }
            mi.Click += (_, _) => a();
            return mi;
        }
        root.Items.Add(Item("Use the Visualizer Screen Saver", () => Enabled = !Enabled, Enabled));
        var after = new MenuItem { Header = "Start After" };
        foreach (int m in IdleChoices) after.Items.Add(Item(m == 1 ? "1 minute" : $"{m} minutes", () => IdleMinutes = m, IdleMinutes == m));
        root.Items.Add(after);
        var change = new MenuItem { Header = "Change Visualization Every" };
        foreach (int sec in ChangeChoices) change.Items.Add(Item(sec < 60 ? $"{sec} seconds" : sec == 60 ? "1 minute" : $"{sec / 60} minutes", () => ChangeSeconds = sec, ChangeSeconds == sec));
        root.Items.Add(change);
        root.Items.Add(Item("Lock the PC When It Closes", () => LocksWhenClosed = !LocksWhenClosed, LocksWhenClosed));
        root.Items.Add(new Separator());
        root.Items.Add(Item("Start Now", () => Start(manual: true)));
        return root;
    }

    // ---- when to start ------------------------------------------------------------------

    void CheckIdle()
    {
        if (!Enabled || IsRunning || _sessionLocked || Sandbox.Enabled) return;
        if (IdleMilliseconds() < IdleMinutes * 60_000L) return;
        if (SomethingNeedsTheScreen()) return;
        Start(manual: false);
    }

    /// <summary>
    /// Like Windows' own screen saver, stay away while a full-screen game, presentation or video is up
    /// (video players ask Windows to keep the display on; music in a browser tab doesn't).
    /// </summary>
    static bool SomethingNeedsTheScreen()
    {
        if (SHQueryUserNotificationState(out int state) == 0 && state is 2 or 3 or 4) return true;
        uint exec = 0;
        if (CallNtPowerInformation(16 /* SystemExecutionState */, IntPtr.Zero, 0, ref exec, 4) == 0 && (exec & 0x2 /* ES_DISPLAY_REQUIRED */) != 0) return true;
        return false;
    }

    void OnSessionSwitch(object? sender, Microsoft.Win32.SessionSwitchEventArgs e)
    {
        if (e.Reason is Microsoft.Win32.SessionSwitchReason.SessionLock or Microsoft.Win32.SessionSwitchReason.ConsoleDisconnect or Microsoft.Win32.SessionSwitchReason.RemoteDisconnect)
        {
            _sessionLocked = true;
            Application.Current?.Dispatcher.BeginInvoke(() => Stop(lockAfter: false));
        }
        else if (e.Reason is Microsoft.Win32.SessionSwitchReason.SessionUnlock or Microsoft.Win32.SessionSwitchReason.ConsoleConnect or Microsoft.Win32.SessionSwitchReason.RemoteConnect)
            _sessionLocked = false;
    }

    // ---- running ------------------------------------------------------------------------

    public void Start(bool manual)
    {
        if (IsRunning) return;
        Log.Write($"Screen saver starting ({(manual ? "manually" : $"idle {IdleMinutes} min")})");
        var monitors = MonitorBounds();
        // Rendered once at the main screen's shape (capped by the renderer) and stretched onto every screen.
        _renderSize = monitors.Count > 0 ? (Math.Max(1, monitors[0].Width), Math.Max(1, monitors[0].Height)) : (1280, 720);
        Rotate(fade: false);
        foreach (var bounds in monitors)
        {
            var w = new ScreenSaverWindow(bounds);
            w.SetBitmap(_renderer.Bitmap);
            w.SetVisualizationName(_currentName);
            w.Show();
            _windows.Add(w);
        }
        _windows.FirstOrDefault()?.Activate();
        UpdateTrack();
        S.Media.TrackChanged += UpdateTrack;
        S.Visualizations.Subscribe(OnFrame);
        _startedAt = DateTime.UtcNow;
        _inputBaseline = LastInputTick();
        GetCursorPos(out _cursorStart);
        _rotateTimer.Interval = TimeSpan.FromSeconds(ChangeSeconds);
        _rotateTimer.Start();
        _inputTimer.Start();
    }

    public void Stop(bool lockAfter)
    {
        if (!IsRunning) return;
        _inputTimer.Stop();
        _rotateTimer.Stop();
        S.Visualizations.Unsubscribe(OnFrame);
        S.Media.TrackChanged -= UpdateTrack;
        foreach (var w in _windows) w.Close();
        _windows.Clear();
        _renderer.Dispose();
        _current = ("", -1);
        Log.Write("Screen saver closed" + (lockAfter ? ", locking the PC" : ""));
        if (lockAfter) LockWorkStation();
    }

    void OnFrame(VisFrame frame)
    {
        if (!IsRunning) return;
        _renderer.Resize(_renderSize.Width, _renderSize.Height);
        _renderer.Render(frame, S.Visualizations.Context);
    }

    /// <summary>Any key or click, or moving the mouse more than a few pixels, closes it.</summary>
    void CheckInput()
    {
        uint last = LastInputTick();
        if (last == _inputBaseline) return;
        // A second's grace so the click or key that started it doesn't close it again.
        if (DateTime.UtcNow - _startedAt < TimeSpan.FromSeconds(1.5)) { _inputBaseline = last; GetCursorPos(out _cursorStart); return; }
        GetCursorPos(out var now);
        int moved = Math.Abs(now.X - _cursorStart.X) + Math.Abs(now.Y - _cursorStart.Y);
        if (moved > 0 && moved <= 12) { _inputBaseline = last; return; } // a jittery mouse
        Stop(lockAfter: LocksWhenClosed);
    }

    // ---- what it shows ------------------------------------------------------------------

    /// <summary>Every visualization and preset worth watching full screen (album art isn't one).</summary>
    IEnumerable<(VisDescriptor Effect, int Preset)> Choices() =>
        VisualizationCatalog.All.Where(d => d.Id != "AlbumArt")
            .SelectMany(d => Enumerable.Range(0, Math.Max(1, d.Presets.Count)).Select(p => (d, p)));

    void Rotate() => Rotate(fade: true);

    void Rotate(bool fade)
    {
        var choices = Choices().Where(c => (c.Effect.Id, c.Preset) != _current).ToList();
        if (choices.Count == 0) return;
        var (effect, preset) = choices[_random.Next(choices.Count)];
        void Swap()
        {
            _current = (effect.Id, preset);
            _renderer.SetEffect(effect, preset);
            string presetName = preset < effect.Presets.Count ? effect.Presets[preset] : "";
            _currentName = presetName.Length > 0 && presetName != effect.Title ? $"{effect.Title}: {presetName}" : effect.Title;
            foreach (var w in _windows) w.SetVisualizationName(_currentName);
        }
        if (!fade || _windows.Count == 0) { Swap(); return; }
        _windows[0].FadeThrough(Swap, _windows.Skip(1));
    }

    void UpdateTrack()
    {
        var m = S.Media.Current;
        foreach (var w in _windows) w.ShowTrack(m.HasSession ? m.Title : "", m.HasSession ? m.Artist : "");
    }

    // ---- Win32 ----------------------------------------------------------------------------

    static long IdleMilliseconds() => unchecked((uint)Environment.TickCount - LastInputTick());

    static uint LastInputTick()
    {
        var info = new LASTINPUTINFO { cbSize = (uint)Marshal.SizeOf<LASTINPUTINFO>() };
        return GetLastInputInfo(ref info) ? info.dwTime : (uint)Environment.TickCount;
    }

    /// <summary>Each monitor's rectangle in physical pixels.</summary>
    static List<Int32Rect> MonitorBounds()
    {
        var list = new List<Int32Rect>();
        EnumDisplayMonitors(IntPtr.Zero, IntPtr.Zero, (IntPtr h, IntPtr hdc, ref RECT r, IntPtr data) =>
        {
            list.Add(new Int32Rect(r.Left, r.Top, r.Right - r.Left, r.Bottom - r.Top));
            return true;
        }, IntPtr.Zero);
        // Primary first, so the render size follows the main screen.
        return list.OrderBy(b => b.X == 0 && b.Y == 0 ? 0 : 1).ToList();
    }

    [StructLayout(LayoutKind.Sequential)] struct LASTINPUTINFO { public uint cbSize; public uint dwTime; }
    [StructLayout(LayoutKind.Sequential)] struct POINT { public int X, Y; }
    [StructLayout(LayoutKind.Sequential)] struct RECT { public int Left, Top, Right, Bottom; }
    delegate bool MonitorEnumProc(IntPtr monitor, IntPtr hdc, ref RECT rect, IntPtr data);

    [DllImport("user32")] static extern bool GetLastInputInfo(ref LASTINPUTINFO info);
    [DllImport("user32")] static extern bool GetCursorPos(out POINT p);
    [DllImport("user32")] static extern bool LockWorkStation();
    [DllImport("user32")] static extern bool EnumDisplayMonitors(IntPtr hdc, IntPtr clip, MonitorEnumProc proc, IntPtr data);
    [DllImport("shell32")] static extern int SHQueryUserNotificationState(out int state);
    [DllImport("powrprof")] static extern uint CallNtPowerInformation(int level, IntPtr input, uint inputSize, ref uint output, uint outputSize);
}
