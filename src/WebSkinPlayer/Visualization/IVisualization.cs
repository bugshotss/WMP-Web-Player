using WebSkinPlayer.Audio;

namespace WebSkinPlayer.Visualization;

/// <summary>What a visualization may want to know about the current track.</summary>
public sealed class VisContext
{
    public string Title = "";
    public string Artist = "";
    public string Album = "";
    /// <summary>Album artwork as BGRA pixels, or null.</summary>
    public int[]? Artwork;
    public int ArtworkWidth, ArtworkHeight;
    public int ArtworkVersion;
}

/// <summary>A visualization plug-in: either a built-in recreation or a hosted WMP IWMPEffects.</summary>
public interface IVisualization : IDisposable
{
    /// <summary>Stable id used by skins and settings (e.g. "Bars", "Battery", "Alchemy").</summary>
    string Id { get; }
    string Title { get; }
    IReadOnlyList<string> Presets { get; }
    int CurrentPreset { get; set; }
    /// <summary>Largest internal resolution worth rendering at (0 = no preference); output is scaled up.</summary>
    int MaxPixelsHint => 0;
    void Render(VisFrame frame, PixelBuffer target, VisContext context);
}
