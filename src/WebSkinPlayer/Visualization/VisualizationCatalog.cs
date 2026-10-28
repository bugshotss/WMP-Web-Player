namespace WebSkinPlayer.Visualization;

public sealed record VisDescriptor(string Id, string Title, string? ClassId, IReadOnlyList<string> Presets, Func<IVisualization?> Create);

/// <summary>Every visualization the app can show: hosted WMP plug-ins first, then the built-ins.</summary>
public static class VisualizationCatalog
{
    static List<VisDescriptor>? _all;
    static readonly List<string> _builtInTitles = new();

    public static IReadOnlyList<VisDescriptor> All => _all ??= Load();

    /// <summary>Titles of the visualizations the app has without any added plug-ins.</summary>
    public static IReadOnlyList<string> BuiltInTitles { get { _ = All; return _builtInTitles; } }

    /// <summary>Re-reads the list after plug-ins were added.</summary>
    public static void Reload() => _all = null;

    static List<VisDescriptor> Load()
    {
        var list = new List<VisDescriptor>();
        foreach (var (id, clsid) in WmpEffectAdapter.EnumerateInstalled())
        {
            // Bars and Battery live inside wmp.dll and can't be created externally; we ship our own.
            if (id.Equals("Bars", StringComparison.OrdinalIgnoreCase) || id.Equals("Battery", StringComparison.OrdinalIgnoreCase)) continue;
            using var probe = WmpEffectAdapter.TryCreate(id, clsid);
            if (probe == null) continue;
            var presets = probe.Presets.ToArray();
            string title = probe.Title;
            list.Add(new VisDescriptor(id, title, "{" + clsid.ToString().ToUpperInvariant() + "}", presets, () => WmpEffectAdapter.TryCreate(id, clsid)));
        }
        var bars = new BarsAndWaves();
        list.Add(new VisDescriptor(bars.Id, bars.Title, BarsAndWaves.ClassId, bars.Presets, () => new BarsAndWaves()));
        var battery = new Battery();
        list.Add(new VisDescriptor(battery.Id, battery.Title, Battery.ClassId, battery.Presets, () => new Battery()));
        var art = new AlbumArtVisualization();
        list.Add(new VisDescriptor(art.Id, art.Title, null, art.Presets, () => new AlbumArtVisualization()));
        _builtInTitles.Clear();
        _builtInTitles.AddRange(list.Select(d => d.Title));
        // Added 32-bit plug-ins (WMP 8's originals, the Plus! and Averett packs...), run by the 32-bit helper.
        foreach (var plugin in PluginLibrary.Load())
            if (!list.Any(d => d.Id.Equals(plugin.Id, StringComparison.OrdinalIgnoreCase)))
                list.Add(new VisDescriptor(plugin.Id, plugin.Title, plugin.Clsid, plugin.Presets, () => new Hosted32Visualization(plugin)));
        Log.Write("Visualizations: " + string.Join(", ", list.Select(v => $"{v.Title} ({v.Presets.Count})")));
        return list;
    }

    /// <summary>Resolve by id, title, or class id (with or without braces), as skins may use any.</summary>
    public static VisDescriptor? Find(string? key)
    {
        if (string.IsNullOrWhiteSpace(key)) return null;
        string k = key.Trim();
        string bare = k.Trim('{', '}');
        foreach (var d in All)
        {
            if (d.Id.Equals(k, StringComparison.OrdinalIgnoreCase) || d.Title.Equals(k, StringComparison.OrdinalIgnoreCase)) return d;
            if (d.ClassId != null && d.ClassId.Trim('{', '}').Equals(bare, StringComparison.OrdinalIgnoreCase)) return d;
        }
        // Common aliases used by skins and older WMP versions (WMP 7/XP shipped Spikes, Ambience,
        // Musical Colors, Particle and Plenoptic); map them onto the closest effect we have.
        if (k.Contains("bars", StringComparison.OrdinalIgnoreCase) || k.Contains("wave", StringComparison.OrdinalIgnoreCase) ||
            k.Contains("spike", StringComparison.OrdinalIgnoreCase) || k.Contains("scope", StringComparison.OrdinalIgnoreCase)) return All.FirstOrDefault(d => d.Id == "Bars");
        if (k.Contains("battery", StringComparison.OrdinalIgnoreCase) || k.Contains("ambience", StringComparison.OrdinalIgnoreCase) ||
            k.Contains("musical", StringComparison.OrdinalIgnoreCase) || k.Contains("particle", StringComparison.OrdinalIgnoreCase) ||
            k.Contains("plenoptic", StringComparison.OrdinalIgnoreCase)) return All.FirstOrDefault(d => d.Id == "Battery");
        if (k.Contains("album", StringComparison.OrdinalIgnoreCase)) return All.FirstOrDefault(d => d.Id == "AlbumArt");
        return null;
    }

    public static VisDescriptor Default => All.FirstOrDefault(d => d.Id == "Bars") ?? All[0];
}
