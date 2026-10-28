using System.IO;
using System.Text.Json;

namespace WebSkinPlayer;

public sealed class WindowPlacement
{
    public double Left { get; set; } = double.NaN;
    public double Top { get; set; } = double.NaN;
    public double Width { get; set; } = double.NaN;
    public double Height { get; set; } = double.NaN;
}

/// <summary>User settings persisted as JSON next to the log.</summary>
public sealed class AppSettings
{
    public string? CurrentSkin { get; set; }
    public bool StartInSkinMode { get; set; }
    public string EffectId { get; set; } = "Bars";
    public int EffectPreset { get; set; }
    public string? PreferredApp { get; set; }
    /// <summary>Player windows (full mode and every skin window) stay above other windows.</summary>
    public bool AlwaysOnTop { get; set; }
    /// <summary>Visualizer screen saver: random full-screen visualizers once the PC has been idle this long.</summary>
    public bool ScreenSaverEnabled { get; set; }
    public int ScreenSaverIdleMinutes { get; set; } = 5;
    public int ScreenSaverChangeSeconds { get; set; } = 30;
    /// <summary>Lock Windows when the screen saver is dismissed.</summary>
    public bool ScreenSaverLocks { get; set; }
    /// <summary>Taskbar widget: shown even when a player window is open.</summary>
    public bool WidgetAlwaysOn { get; set; }
    /// <summary>The app was collapsed into the taskbar widget when it last closed.</summary>
    public bool StartInWidgetMode { get; set; }
    /// <summary>"left" or "right" default placement; WidgetOffset (DIPs from the taskbar's left) once dragged.</summary>
    public string WidgetSide { get; set; } = "left";
    public double WidgetOffset { get; set; } = double.NaN;
    public WindowPlacement FullMode { get; set; } = new();
    /// <summary>Per-skin, per-view window placement, key "skinfile|viewid".</summary>
    public Dictionary<string, WindowPlacement> SkinWindows { get; set; } = new();
    /// <summary>theme.savePreference storage, per skin file.</summary>
    public Dictionary<string, Dictionary<string, string>> SkinPreferences { get; set; } = new();
    /// <summary>Global mediacenter-style flags skins toggle (showTitles, showEffects...).</summary>
    public Dictionary<string, string> MediaCenter { get; set; } = new();

    static string FilePath => Path.Combine(Log.Folder, "settings.json");
    static readonly JsonSerializerOptions Json = new()
    {
        WriteIndented = true,
        NumberHandling = System.Text.Json.Serialization.JsonNumberHandling.AllowNamedFloatingPointLiterals,
    };

    public static AppSettings Load()
    {
        try
        {
            if (File.Exists(FilePath))
            {
                var settings = JsonSerializer.Deserialize<AppSettings>(File.ReadAllText(FilePath), Json) ?? new();
                // Earlier builds kept a skin-only flag among the skins' mediacenter values.
                if (settings.MediaCenter.Remove("__alwaysOnTop", out var old) && old.StartsWith("true")) settings.AlwaysOnTop = true;
                return settings;
            }
        }
        catch (Exception ex) { Log.Write("Settings load failed: " + ex.Message); }
        return new AppSettings();
    }

    public void Save()
    {
        if (Sandbox.Enabled) return;
        try
        {
            Directory.CreateDirectory(Log.Folder);
            File.WriteAllText(FilePath, JsonSerializer.Serialize(this, Json));
        }
        catch (Exception ex) { Log.Write("Settings save failed: " + ex.Message); }
    }
}
