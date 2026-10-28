using System.Diagnostics;
using System.Windows;
using WebSkinPlayer.Media;
using WebSkinPlayer.Skins.Scripting;
using WebSkinPlayer.Visualization;

namespace WebSkinPlayer.Skins;

public sealed record TrackInfo(string Title, string Artist, string Album, double Duration);

/// <summary>
/// WMP's scripting object model (player, mediacenter, theme, event) for one skin theme,
/// backed by the shared <see cref="PlayerModel"/>.
/// </summary>
public sealed class WmpObjects
{
    readonly SkinTheme _theme;
    readonly PlayerModel _pm;

    public HostObject Player { get; } = new("player");
    public HostObject Controls { get; } = new("controls");
    public HostObject Settings { get; } = new("settings");
    public HostObject CurrentMedia { get; } = new("currentMedia");
    public HostObject Network { get; } = new("network");
    public HostObject CurrentPlaylist { get; } = new("currentPlaylist");
    public HostObject Dvd { get; } = new("dvd");
    public HostObject Error { get; } = new("error");
    public HostObject Cdroms { get; } = new("cdromCollection");
    public HostObject MediaCollection { get; } = new("mediaCollection");
    public HostObject PlaylistCollection { get; } = new("playlistCollection");
    public MediaCenterObject MediaCenter { get; }
    public HostObject Theme { get; } = new("theme");
    public EventObject Event { get; } = new();
    public List<TrackInfo> History { get; } = new();

    /// <summary>Raised for player events (name, args) so views can run PLAYER handlers.</summary>
    public event Action<string, object?[]>? PlayerEvent;

    public static readonly Dictionary<string, string[]> EventParameters = new(StringComparer.OrdinalIgnoreCase)
    {
        ["OpenStateChange"] = new[] { "NewState" },
        ["PlayStateChange"] = new[] { "NewState" },
        ["StatusChange"] = Array.Empty<string>(),
        ["MediaChange"] = new[] { "Item" },
        ["CurrentItemChange"] = new[] { "pdispMedia" },
        ["CurrentMediaItemAvailable"] = new[] { "bstrItemName" },
        ["CurrentPlaylistChange"] = new[] { "change" },
        ["CurrentPlaylistItemAvailable"] = new[] { "bstrItemName" },
        ["PlaylistChange"] = new[] { "Playlist", "change" },
        ["ModeChange"] = new[] { "ModeName", "NewValue" },
        ["PositionChange"] = new[] { "oldPosition", "newPosition" },
        ["Disconnect"] = new[] { "Result" },
        ["Buffering"] = new[] { "Start" },
        ["Error"] = Array.Empty<string>(),
        ["MediaError"] = new[] { "pMediaObject" },
        ["Warning"] = new[] { "WarningType", "Param", "Description" },
        ["EndOfStream"] = new[] { "Result" },
        ["MarkerHit"] = new[] { "MarkerNum" },
        ["ScriptCommand"] = new[] { "scType", "Param" },
        ["NewStream"] = Array.Empty<string>(),
        ["DurationUnitChange"] = new[] { "NewDurationUnit" },
        ["MediaCollectionChange"] = Array.Empty<string>(),
        ["PlayerReconnect"] = Array.Empty<string>(),
        ["DomainChange"] = new[] { "strDomain" },
        ["OpenPlaylistSwitch"] = new[] { "pItem" },
    };

    public WmpObjects(SkinTheme theme, PlayerModel pm)
    {
        _theme = theme;
        _pm = pm;
        MediaCenter = new MediaCenterObject();
        BuildPlayer();
        BuildControls();
        BuildSettings();
        BuildMedia();
        BuildPlaylist();
        BuildMisc();
        BuildTheme();
        RecordHistory();
    }

    public void Attach()
    {
        _pm.PropertyChanged += OnModelProperty;
        _pm.PlayerEvent += OnModelEvent;
        App.Services.Visualizations.EffectChanged += MediaCenter.OnEffectChanged;
    }

    public void Detach()
    {
        _pm.PropertyChanged -= OnModelProperty;
        _pm.PlayerEvent -= OnModelEvent;
        App.Services.Visualizations.EffectChanged -= MediaCenter.OnEffectChanged;
    }

    void OnModelProperty(string obj, string prop)
    {
        switch (obj)
        {
            case "player": Player.Notify(prop); break;
            case "controls": Controls.Notify(prop); break;
            case "settings": Settings.Notify(prop); break;
            case "currentMedia": CurrentMedia.Notify(prop); break;
        }
    }

    void OnModelEvent(string name, object?[] args)
    {
        if (name == "MediaChange")
        {
            RecordHistory();
            _theme.InvalidateAll(); // WMPImage_AlbumArt* follows the track
        }
        if (name is "MediaChange" or "CurrentItemChange") args = new object?[] { CurrentMedia };
        PlayerEvent?.Invoke(name, args);
    }

    void RecordHistory()
    {
        var m = _pm.Media;
        if (m.Title.Length == 0) return;
        var last = History.LastOrDefault();
        if (last != null && last.Title == m.Title && last.Artist == m.Artist) { History[^1] = last with { Duration = _pm.Duration }; return; }
        History.Add(new TrackInfo(m.Title, m.Artist, m.Album, _pm.Duration));
        if (History.Count > 200) History.RemoveAt(0);
        CurrentPlaylist.Notify("count");
        PlayerEvent?.Invoke("CurrentPlaylistChange", new object?[] { 6 });
    }

    static object? A(object?[] a, int i) => i < a.Length ? a[i] : null;

    public static void OpenInBrowser(string url)
    {
        if (Sandbox.Enabled) return;
        try { Process.Start(new ProcessStartInfo(url) { UseShellExecute = true }); }
        catch (Exception ex) { Log.Write("Open URL failed: " + ex.Message); }
    }

    void BuildPlayer()
    {
        var p = Player;
        p.AddProperty("URL", () => _pm.SourceUrl, v =>
        {
            string url = ScriptObject.ToStr(v);
            if (url.StartsWith("http", StringComparison.OrdinalIgnoreCase)) OpenInBrowser(url);
        });
        p.AddProperty("playState", () => (double)_pm.PlayState);
        p.AddProperty("openState", () => (double)_pm.OpenState);
        p.AddProperty("status", () => _pm.Status);
        p.AddProperty("currentMedia", () => CurrentMedia);
        p.AddProperty("currentPlaylist", () => CurrentPlaylist);
        p.AddProperty("controls", () => Controls);
        p.AddProperty("settings", () => Settings);
        p.AddProperty("network", () => Network);
        p.AddProperty("dvd", () => Dvd);
        p.AddProperty("error", () => Error);
        p.AddProperty("cdromCollection", () => Cdroms);
        p.AddProperty("mediaCollection", () => MediaCollection);
        p.AddProperty("playlistCollection", () => PlaylistCollection);
        p.AddProperty("versionInfo", () => "12.0.26100.1");
        p.AddProperty("isOnline", () => true);
        p.AddProperty("isRemote", () => false);
        p.AddProperty("enabled", () => true);
        p.AddProperty("fullScreen", () => false, v => { if (ScriptObject.ToBool(v)) App.Modes.ShowFullMode(); });
        p.AddProperty("enableContextMenu", () => true);
        p.AddProperty("uiMode", () => "full");
        p.AddProperty("stretchToFit", () => true);
        p.AddProperty("windowlessVideo", () => false);
        p.AddProperty("closedCaption", () => null);
        p.AddProperty("playerApplication", () => null);
        p.AddMethod("launchURL", a => { OpenInBrowser(ScriptObject.ToStr(A(a, 0))); return null; });
        p.AddMethod("close", _ => { _theme.Close(); return null; });
        p.AddMethod("newMedia", _ => CurrentMedia);
        p.AddMethod("newPlaylist", _ => CurrentPlaylist);
        p.AddMethod("openPlayer", _ => { App.Modes.ShowFullMode(); return null; });
    }

    void BuildControls()
    {
        var c = Controls;
        c.AddMethod("play", _ => { _pm.Play(); return null; });
        c.AddMethod("pause", _ => { _pm.Pause(); return null; });
        c.AddMethod("stop", _ => { _pm.Stop(); return null; });
        c.AddMethod("next", _ => { _pm.Next(); return null; });
        c.AddMethod("previous", _ => { _pm.Previous(); return null; });
        c.AddMethod("fastForward", _ => { _pm.FastForward(); return null; });
        c.AddMethod("fastReverse", _ => { _pm.FastReverse(); return null; });
        c.AddMethod("step", _ => null);
        c.AddMethod("playItem", _ => { _pm.Play(); return null; });
        c.AddMethod("isAvailable", a => _pm.IsAvailable(ScriptObject.ToStr(A(a, 0))));
        c.AddProperty("currentPosition", () => _pm.Position, v => _pm.SetPosition(ScriptObject.ToNumber(v)));
        c.AddProperty("currentPositionString", () => _pm.Duration > 0 || _pm.Position > 0 ? PlayerModel.FormatTime(_pm.Position) : "");
        c.AddProperty("currentPositionTimecode", () => $"[00000]{PlayerModel.FormatTime(_pm.Position)}.00");
        c.AddProperty("currentItem", () => CurrentMedia, _ => { });
        c.AddProperty("currentMarker", () => 0.0, _ => { });
        c.AddProperty("audioLanguageCount", () => 0.0);
        c.AddProperty("currentAudioLanguage", () => 0.0, _ => { });
        c.AddProperty("currentAudioLanguageIndex", () => 0.0, _ => { });
    }

    void BuildSettings()
    {
        var s = Settings;
        s.AddProperty("volume", () => (double)_pm.Volume, v => _pm.Volume = (int)Math.Round(ScriptObject.ToNumber(v)));
        s.AddProperty("mute", () => _pm.Mute, v => _pm.Mute = ScriptObject.ToBool(v));
        s.AddProperty("balance", () => (double)_pm.Balance, v => _pm.Balance = (int)ScriptObject.ToNumber(v));
        s.AddProperty("shuffle", () => _pm.Shuffle, v => _pm.Shuffle = ScriptObject.ToBool(v));
        s.AddProperty("loop", () => _pm.Loop, v => _pm.Loop = ScriptObject.ToBool(v));
        s.AddProperty("rate", () => 1.0, _ => { });
        s.AddProperty("autoStart", () => true, _ => { });
        s.AddProperty("playCount", () => 1.0, _ => { });
        s.AddProperty("baseURL", () => "", _ => { });
        s.AddProperty("defaultFrame", () => "", _ => { });
        s.AddProperty("enableErrorDialogs", () => false, _ => { });
        s.AddProperty("invokeURLs", () => true, _ => { });
        s.AddProperty("mediaAccessRights", () => "full");
        s.AddMethod("getMode", a => ScriptObject.ToStr(A(a, 0)).ToLowerInvariant() switch
        {
            "shuffle" => _pm.Shuffle,
            "loop" => _pm.Loop,
            "autorewind" => true,
            "showframe" => false,
            _ => false,
        });
        s.AddMethod("setMode", a =>
        {
            string mode = ScriptObject.ToStr(A(a, 0)).ToLowerInvariant();
            bool on = ScriptObject.ToBool(A(a, 1));
            if (mode == "shuffle") _pm.Shuffle = on;
            else if (mode == "loop") _pm.Loop = on;
            return null;
        });
        s.AddMethod("isAvailable", _ => true);
        s.AddMethod("requestMediaAccessRights", _ => true);
    }

    void BuildMedia()
    {
        var m = CurrentMedia;
        m.AddProperty("name", () => _pm.Media.Title, _ => { });
        m.AddProperty("duration", () => _pm.Duration);
        m.AddProperty("durationString", () => _pm.Duration > 0 ? PlayerModel.FormatTime(_pm.Duration) : "");
        m.AddProperty("sourceURL", () => _pm.SourceUrl);
        m.AddProperty("imageSourceWidth", () => 0.0);
        m.AddProperty("imageSourceHeight", () => 0.0);
        m.AddProperty("markerCount", () => 0.0);
        m.AddProperty("attributeCount", () => (double)MediaAttributes.Length);
        m.AddMethod("getItemInfo", a => _pm.GetItemInfo(ScriptObject.ToStr(A(a, 0))));
        m.AddMethod("getItemInfoByAtom", a => _pm.GetItemInfo(MediaAttributes.ElementAtOrDefault((int)ScriptObject.ToNumber(A(a, 0))) ?? ""));
        m.AddMethod("getAttributeName", a => MediaAttributes.ElementAtOrDefault((int)ScriptObject.ToNumber(A(a, 0))) ?? "");
        m.AddMethod("getAttributeCountByType", _ => 0.0);
        m.AddMethod("getItemInfoByType", a => _pm.GetItemInfo(ScriptObject.ToStr(A(a, 0))));
        m.AddMethod("setItemInfo", _ => null);
        m.AddMethod("isIdentical", a => ReferenceEquals(A(a, 0), CurrentMedia));
        m.AddMethod("isMemberOf", _ => true);
        m.AddMethod("isReadOnlyItem", _ => true);
        m.AddMethod("getMarkerTime", _ => 0.0);
        m.AddMethod("getMarkerName", _ => "");
    }

    static readonly string[] MediaAttributes = { "Title", "Author", "WM/AlbumTitle", "Duration", "SourceURL", "MediaType" };

    void BuildPlaylist()
    {
        var pl = CurrentPlaylist;
        pl.AddProperty("count", () => (double)Math.Max(1, History.Count));
        pl.AddProperty("name", () => _pm.Media.SourceDisplayName.Length > 0 ? _pm.Media.SourceDisplayName : "Now Playing", _ => { });
        pl.AddProperty("attributeCount", () => 0.0);
        pl.AddMethod("item", a =>
        {
            int i = (int)ScriptObject.ToNumber(A(a, 0));
            if (i == History.Count - 1 || History.Count == 0) return CurrentMedia;
            var t = History.ElementAtOrDefault(i);
            if (t == null) return null;
            var o = new HostObject("media");
            o.AddProperty("name", () => t.Title);
            o.AddProperty("duration", () => t.Duration);
            o.AddProperty("durationString", () => PlayerModel.FormatTime(t.Duration));
            o.AddProperty("sourceURL", () => "");
            o.AddMethod("getItemInfo", b => ScriptObject.ToStr(A(b, 0)).ToLowerInvariant() switch
            {
                "title" or "name" => t.Title,
                "author" or "artist" or "wm/albumartist" => t.Artist,
                "album" or "wm/albumtitle" => t.Album,
                _ => "",
            });
            o.AddMethod("isIdentical", b => false);
            return o;
        });
        pl.AddMethod("getItemInfo", _ => "");
        pl.AddMethod("setItemInfo", _ => null);
        pl.AddMethod("getAttributeName", _ => "");
        pl.AddMethod("isIdentical", a => ReferenceEquals(A(a, 0), CurrentPlaylist));
        foreach (var name in new[] { "appendItem", "insertItem", "removeItem", "clear", "moveItem" }) pl.AddMethod(name, _ => null);
    }

    void BuildMisc()
    {
        var n = Network;
        foreach (var name in new[] { "bufferingProgress", "downloadProgress", "receptionQuality" }) n.AddProperty(name, () => 100.0);
        foreach (var name in new[] { "bandWidth", "bitRate", "maxBitRate", "maxBandwidth", "bufferingCount", "bufferingTime", "encodedFrameRate",
                                     "frameRate", "framesSkipped", "lostPackets", "receivedPackets", "recoveredPackets", "frameRate" })
            n.AddProperty(name, () => 0.0, _ => { });
        n.AddProperty("sourceProtocol", () => "https");
        n.AddMethod("getProxyName", _ => "");
        n.AddMethod("getProxySettings", _ => 0.0);

        Dvd.AddMethod("isAvailable", _ => false);
        foreach (var name in new[] { "topMenu", "titleMenu", "back", "resume" }) Dvd.AddMethod(name, _ => null);
        Dvd.AddProperty("domain", () => "");

        Error.AddProperty("errorCount", () => 0.0);
        Error.AddMethod("item", _ => null);
        Error.AddMethod("clearErrorQueue", _ => null);
        Error.AddMethod("webHelp", _ => null);

        Cdroms.AddProperty("count", () => 0.0);
        Cdroms.AddMethod("item", _ => null);
        Cdroms.AddMethod("getByDriveSpecifier", _ => null);
        foreach (var obj in new[] { MediaCollection, PlaylistCollection })
            foreach (var name in new[] { "add", "getAll", "getByName", "getByGenre", "getByAuthor", "getByAlbum", "getByAttribute", "remove", "newPlaylist", "importPlaylist", "getAttributeStringCollection", "isDeleted", "setDeleted" })
                obj.AddMethod(name, _ => null);
    }

    void BuildTheme()
    {
        var t = Theme;
        t.AddProperty("currentViewID", () => _theme.ActiveView?.Id ?? "", v => _theme.SwitchToView(ScriptObject.ToStr(v)));
        t.AddProperty("title", () => _theme.Title);
        t.AddProperty("author", () => _theme.Author);
        t.AddProperty("copyright", () => _theme.Copyright);
        t.AddProperty("version", () => "12.0");
        t.AddMethod("openView", a => { _theme.OpenView(ScriptObject.ToStr(A(a, 0)), null, null); return null; });
        t.AddMethod("openViewRelative", a => { _theme.OpenView(ScriptObject.ToStr(A(a, 0)), ScriptObject.ToNumber(A(a, 1)), ScriptObject.ToNumber(A(a, 2))); return null; });
        t.AddMethod("closeView", a => { _theme.CloseView(ScriptObject.ToStr(A(a, 0))); return null; });
        t.AddMethod("loadPreference", a => _theme.LoadPreference(ScriptObject.ToStr(A(a, 0))));
        t.AddMethod("savePreference", a => { _theme.SavePreference(ScriptObject.ToStr(A(a, 0)), ScriptObject.ToStr(A(a, 1))); return null; });
        t.AddMethod("loadString", a => _theme.LoadString(ScriptObject.ToStr(A(a, 0))));
        t.AddMethod("openDialog", a => { _theme.OpenDialog(ScriptObject.ToStr(A(a, 0)), ScriptObject.ToStr(A(a, 1))); return null; });
        t.AddMethod("playSound", a => { _theme.PlaySound(ScriptObject.ToStr(A(a, 0))); return null; });
        t.AddMethod("stopSound", _ => null);
        t.AddMethod("showErrorDialog", _ => null);
        t.AddMethod("logString", a => { Log.Write("[skin] " + ScriptObject.ToStr(A(a, 0))); return null; });
        t.AddMethod("messageBox", a => { if (!Sandbox.Enabled) MessageBox.Show(ScriptObject.ToStr(A(a, 0)), _theme.Title); return null; });
    }
}

/// <summary>The "mediacenter" object: global player-UI settings, persisted between sessions.</summary>
public sealed class MediaCenterObject : ScriptObject
{
    public override string ClassName => "mediacenter";

    public MediaCenterObject()
    {
        foreach (var kv in App.Services.Settings.MediaCenter) Values[kv.Key] = Restore(kv.Value);
        Computed("effectType", () => EffectsKey(), v => App.Services.Visualizations.SetEffect(ToStr(v), 0));
        Computed("effectPreset", () => (double)App.Services.Visualizations.Preset, v => App.Services.Visualizations.SetPreset((int)ToNumber(v)));
        Computed("effectPresetTitle", () => App.Services.Visualizations.Current.Presets.ElementAtOrDefault(App.Services.Visualizations.Preset) ?? "");
        Computed("effectTitle", () => App.Services.Visualizations.Current.Title);
        Computed("videoZoom", () => Values.TryGetValue("videoZoom", out var z) ? z : 100.0, v => SetValue("videoZoom", ToNumber(v)));
        Computed("videoStretchToFit", () => !Values.TryGetValue("videoStretchToFit", out var s) || ToBool(s), v => SetValue("videoStretchToFit", ToBool(v)));
        Computed("showEffects", () => !Values.TryGetValue("showEffects", out var s) || ToBool(s), v => SetValue("showEffects", ToBool(v)));
        Computed("showTitles", () => !Values.TryGetValue("showTitles", out var s) || ToBool(s), v => SetValue("showTitles", ToBool(v)));
        Computed("contrastMode", () => false);
    }

    static string EffectsKey() => Elements.EffectsElement.EffectKey(App.Services.Visualizations.Current);

    public void OnEffectChanged()
    {
        Notify("effectType");
        Notify("effectPreset");
        Notify("effectPresetTitle");
        Notify("effectTitle");
    }

    protected override void OnValueChanged(string name)
    {
        App.Services.Settings.MediaCenter[name] = ToStr(Values[name]) + "|" + (Values[name] switch { bool => "b", double => "n", _ => "s" });
        if (name.Equals("videoZoom", StringComparison.OrdinalIgnoreCase) || name.Equals("videoStretchToFit", StringComparison.OrdinalIgnoreCase)) { }
    }

    static object? Restore(string stored)
    {
        int bar = stored.LastIndexOf('|');
        if (bar < 0) return stored;
        string v = stored[..bar];
        return stored[(bar + 1)..] switch { "b" => v == "true", "n" => ToNumber(v), _ => v };
    }
}

/// <summary>The global "event" object, filled in before each mouse/keyboard handler runs.</summary>
public sealed class EventObject : ScriptObject
{
    public override string ClassName => "event";

    public EventObject()
    {
        Computed("screenWidth", () => SystemParameters.WorkArea.Width);
        Computed("screenHeight", () => SystemParameters.WorkArea.Height);
        Computed("screenLeft", () => SystemParameters.WorkArea.Left);
        Computed("screenTop", () => SystemParameters.WorkArea.Top);
        Computed("fullScreenWidth", () => SystemParameters.PrimaryScreenWidth);
        Computed("fullScreenHeight", () => SystemParameters.PrimaryScreenHeight);
        foreach (var n in new[] { "keyCode", "x", "y", "offsetX", "offsetY", "screenX", "screenY", "button", "wheelDelta" }) Values[n] = 0.0;
        foreach (var n in new[] { "shiftKey", "ctrlKey", "altKey", "cancelBubble" }) Values[n] = false;
        Values["returnValue"] = true;
    }

    public void SetMouse(Point local, Point view, Point screen, int button)
    {
        Values["offsetX"] = local.X; Values["offsetY"] = local.Y;
        Values["x"] = view.X; Values["y"] = view.Y;
        Values["screenX"] = screen.X; Values["screenY"] = screen.Y;
        Values["button"] = (double)button;
        SetModifiers();
    }

    public void SetKey(int keyCode)
    {
        Values["keyCode"] = (double)keyCode;
        SetModifiers();
    }

    void SetModifiers()
    {
        var m = System.Windows.Input.Keyboard.Modifiers;
        Values["shiftKey"] = m.HasFlag(System.Windows.Input.ModifierKeys.Shift);
        Values["ctrlKey"] = m.HasFlag(System.Windows.Input.ModifierKeys.Control);
        Values["altKey"] = m.HasFlag(System.Windows.Input.ModifierKeys.Alt);
        Values["returnValue"] = true;
        Values["cancelBubble"] = false;
    }
}
