using System.Windows;
using WebSkinPlayer.Skins;
using WebSkinPlayer.UI;

namespace WebSkinPlayer;

/// <summary>
/// Switches between full mode (library window), skin mode and the taskbar widget, like WMP's
/// Ctrl+1 / Ctrl+2 (and its taskbar mini player).
/// </summary>
public sealed class ModeController
{
    FullModeWindow? _full;
    SkinTheme? _theme;
    TaskbarWidget? _widget;
    bool _collapsed;
    bool _lastWasSkin;

    public SkinTheme? CurrentTheme => _theme;
    public FullModeWindow? FullWindow => _full;
    public bool InSkinMode => _theme != null;
    public bool InWidgetMode => _collapsed;
    public bool WidgetShown => _widget?.IsShown ?? false;

    public void ShowFullMode()
    {
        _collapsed = false;
        _full ??= CreateFullMode();
        _full.Show();
        if (_full.WindowState == WindowState.Minimized) _full.WindowState = WindowState.Normal;
        _full.Activate();
        CloseTheme();
        _lastWasSkin = false;
        App.Services.Settings.StartInSkinMode = false;
        App.Services.Settings.StartInWidgetMode = false;
        SyncWidget();
    }

    FullModeWindow CreateFullMode()
    {
        var w = new FullModeWindow();
        w.Closed += (_, _) =>
        {
            _full = null;
            if (_theme == null && !_collapsed) Exit();
        };
        return w;
    }

    /// <summary>Loads a skin and switches to it. Returns false (and stays put) if it can't load.</summary>
    public bool ShowSkin(string path)
    {
        SkinTheme theme;
        try
        {
            theme = SkinTheme.Load(path);
        }
        catch (Exception ex)
        {
            Log.Write($"Skin load failed ({path}): {ex}");
            MessageBox.Show($"This skin couldn't be loaded:\n{System.IO.Path.GetFileName(path)}\n\n{ex.Message}", "WebSkin Player", MessageBoxButton.OK, MessageBoxImage.Warning);
            if (_full == null && _theme == null) ShowFullMode();
            return false;
        }

        var old = _theme;
        _theme = theme;
        theme.Closed += () =>
        {
            if (_theme != theme) return;
            _theme = null;
            if (_full == null && !_collapsed) Exit();
        };
        try
        {
            theme.OpenMainView();
        }
        catch (Exception ex)
        {
            Log.Write($"Skin open failed ({path}): {ex}");
            _theme = old;
            theme.Close();
            MessageBox.Show($"This skin couldn't be opened:\n{ex.Message}", "WebSkin Player", MessageBoxButton.OK, MessageBoxImage.Warning);
            if (_full == null && _theme == null) ShowFullMode();
            return false;
        }

        old?.Close();
        _collapsed = false;
        _lastWasSkin = true;
        App.Services.Settings.CurrentSkin = path;
        App.Services.Settings.StartInSkinMode = true;
        App.Services.Settings.StartInWidgetMode = false;
        App.Services.Settings.Save();
        _full?.Hide();
        SyncWidget();
        return true;
    }

    void CloseTheme()
    {
        var t = _theme;
        _theme = null;
        t?.Close();
    }

    // ---- taskbar widget --------------------------------------------------------------------

    TaskbarWidget Widget
    {
        get
        {
            if (_widget == null)
            {
                _widget = new TaskbarWidget();
                _widget.OpenRequested += () => RestoreFromWidget(null);
            }
            return _widget;
        }
    }

    /// <summary>Show the widget when collapsed, or always if the user pinned it.</summary>
    void SyncWidget()
    {
        bool want = _collapsed || App.Services.Settings.WidgetAlwaysOn;
        if (want) Widget.Show();
        else _widget?.Hide();
    }

    /// <summary>Ctrl+3: tuck the player into the taskbar widget, hiding its windows.</summary>
    public void CollapseToWidget()
    {
        _lastWasSkin = _theme != null;
        _collapsed = true;
        _theme?.SetWindowsVisible(false);
        _full?.Hide();
        App.Services.Settings.StartInWidgetMode = true;
        App.Services.Settings.Save();
        Widget.Show();
    }

    /// <summary>Bring the player back from the widget (skin: null = whatever was showing before).</summary>
    public void RestoreFromWidget(bool? skin)
    {
        bool toSkin = skin ?? _lastWasSkin;
        _collapsed = false;
        App.Services.Settings.StartInWidgetMode = false;
        if (toSkin)
        {
            if (_theme != null)
            {
                _theme.SetWindowsVisible(true);
                _full?.Hide();
                _theme.ActiveView?.Window?.Activate();
            }
            else
            {
                string? path = App.Services.Settings.CurrentSkin;
                if (path != null && System.IO.File.Exists(path)) ShowSkin(path);
                else ShowFullMode();
            }
        }
        else ShowFullMode();
        SyncWidget();
    }

    public void HideWidget()
    {
        App.Services.Settings.WidgetAlwaysOn = false;
        App.Services.Settings.Save();
        if (_collapsed) RestoreFromWidget(null);
        else _widget?.Hide();
    }

    /// <summary>Raised when Always on Top is switched, so the toggles can show the new state.</summary>
    public event Action? AlwaysOnTopChanged;

    public bool AlwaysOnTop => App.Services.Settings.AlwaysOnTop;

    /// <summary>Keeps every player window (full mode and all skin windows) above other windows, or lets them go behind.</summary>
    public void SetAlwaysOnTop(bool on)
    {
        App.Services.Settings.AlwaysOnTop = on;
        App.Services.Settings.Save();
        if (_full != null) _full.Topmost = on;
        if (_theme != null)
            foreach (var v in _theme.OpenViews)
                if (v.Window != null) v.Window.Topmost = on;
        AlwaysOnTopChanged?.Invoke();
    }

    public void ToggleAlwaysOnTop() => SetAlwaysOnTop(!AlwaysOnTop);

    public void ToggleWidgetAlwaysOn()
    {
        App.Services.Settings.WidgetAlwaysOn = !App.Services.Settings.WidgetAlwaysOn;
        App.Services.Settings.Save();
        SyncWidget();
    }

    public void Exit()
    {
        _widget?.Dispose();
        App.Services.Settings.Save();
        Application.Current.Shutdown();
    }
}
