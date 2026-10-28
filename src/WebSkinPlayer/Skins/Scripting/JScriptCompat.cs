using System.Text.RegularExpressions;

namespace WebSkinPlayer.Skins.Scripting;

/// <summary>
/// Smooths over the differences between Microsoft's JScript (what WMP ran skins with) and the
/// standards-based engine we use.
/// </summary>
public static partial class JScriptCompat
{
    /// <summary>Removes an optional "jscript:" / "javascript:" prefix from handler code.</summary>
    public static string StripPrefix(string code)
    {
        code = code.Trim();
        if (code.StartsWith("jscript:", StringComparison.OrdinalIgnoreCase)) return code[8..].Trim();
        if (code.StartsWith("javascript:", StringComparison.OrdinalIgnoreCase)) return code[11..].Trim();
        return code;
    }

    /// <summary>
    /// WMP object properties. JScript invoked COM members with DISPATCH_METHOD|DISPATCH_PROPERTYGET,
    /// so skins could write "mediacenter.effectType()" and get the property value. None of these
    /// names is a method anywhere in WMP's object model.
    /// </summary>
    static readonly HashSet<string> PropertyNames = new(StringComparer.OrdinalIgnoreCase)
    {
        "effectType", "effectPreset", "effectTitle", "effectPresetTitle", "showEffects", "showTitles", "videoZoom", "videoStretchToFit",
        "URL", "playState", "openState", "status", "currentMedia", "currentPlaylist", "controls", "settings", "network",
        "versionInfo", "fullScreen", "uiMode", "isOnline", "currentPosition", "currentPositionString", "currentPositionTimecode",
        "currentItem", "volume", "mute", "balance", "rate", "autoStart", "playCount", "baseURL", "duration", "durationString",
        "sourceURL", "imageSourceWidth", "imageSourceHeight", "attributeCount", "markerCount", "bufferingProgress",
        "downloadProgress", "receptionQuality", "bitRate", "bandWidth", "currentViewID", "currentEffectType", "currentPreset",
        "currentEffectTitle", "currentPresetTitle", "currentPresetTitle", "gainLevel1", "gainLevel2", "gainLevel3", "gainLevel4",
        "gainLevel5", "gainLevel6", "gainLevel7", "gainLevel8", "gainLevel9", "gainLevel10", "selectedItem", "itemCount",
        "focusObjectID", "timerInterval",
    };

    public static string Prepare(string code)
    {
        if (code.IndexOf("::", StringComparison.Ordinal) >= 0)
            code = EventFunction().Replace(code, m => $"function {m.Groups[1].Value}__{m.Groups[2].Value}(");
        if (code.IndexOf("()", StringComparison.Ordinal) >= 0)
            code = PropertyCall().Replace(code, m => PropertyNames.Contains(m.Groups[1].Value) ? "." + m.Groups[1].Value : m.Value);
        return code;
    }

    [GeneratedRegex(@"\.([A-Za-z_]\w*)\s*\(\s*\)")]
    private static partial Regex PropertyCall();

    // JScript's "function obj::event(...)" event-sink syntax.
    [GeneratedRegex(@"function\s+([A-Za-z_$][\w$]*)::([A-Za-z_$][\w$]*)\s*\(")]
    private static partial Regex EventFunction();
}
