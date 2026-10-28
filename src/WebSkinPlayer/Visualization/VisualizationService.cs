using System.Windows.Media;
using System.Windows.Media.Imaging;
using WebSkinPlayer.Audio;
using WebSkinPlayer.Media;

namespace WebSkinPlayer.Visualization;

/// <summary>
/// Drives every visible visualizer once per display frame from the shared loopback analysis,
/// and holds the global "current visualization" (WMP's mediacenter.effectType/effectPreset).
/// </summary>
public sealed class VisualizationService : IDisposable
{
    readonly LoopbackAnalyzer _analyzer;
    readonly HashSet<Action<VisFrame>> _subscribers = new();
    readonly VisContext _context = new();
    bool _hooked;

    public string EffectId { get; private set; }
    public int Preset { get; private set; }
    public event Action? EffectChanged;
    public VisContext Context => _context;

    public VisualizationService(LoopbackAnalyzer analyzer, string? effectId, int preset)
    {
        _analyzer = analyzer;
        var d = VisualizationCatalog.Find(effectId) ?? VisualizationCatalog.Default;
        EffectId = d.Id;
        Preset = Math.Clamp(preset, 0, Math.Max(0, d.Presets.Count - 1));
    }

    public VisDescriptor Current => VisualizationCatalog.Find(EffectId) ?? VisualizationCatalog.Default;

    public void SetEffect(string? idOrClsid, int preset = 0)
    {
        var d = VisualizationCatalog.Find(idOrClsid) ?? VisualizationCatalog.Default;
        preset = Math.Clamp(preset, 0, Math.Max(0, d.Presets.Count - 1));
        if (d.Id == EffectId && preset == Preset) return;
        EffectId = d.Id;
        Preset = preset;
        EffectChanged?.Invoke();
    }

    public void SetPreset(int preset) => SetEffect(EffectId, preset);

    public void NextEffect(int dir = 1)
    {
        var all = VisualizationCatalog.All;
        int i = all.ToList().FindIndex(d => d.Id == EffectId);
        var next = all[((i + dir) % all.Count + all.Count) % all.Count];
        SetEffect(next.Id, 0);
    }

    public void NextPreset(int dir = 1)
    {
        int n = Math.Max(1, Current.Presets.Count);
        SetEffect(EffectId, ((Preset + dir) % n + n) % n);
    }

    public void Subscribe(Action<VisFrame> onFrame)
    {
        _subscribers.Add(onFrame);
        if (!_hooked) { CompositionTarget.Rendering += OnRendering; _hooked = true; }
    }

    public void Unsubscribe(Action<VisFrame> onFrame)
    {
        _subscribers.Remove(onFrame);
        if (_subscribers.Count == 0 && _hooked) { CompositionTarget.Rendering -= OnRendering; _hooked = false; }
    }

    readonly System.Diagnostics.Stopwatch _cost = new();
    int _skip;

    void OnRendering(object? sender, EventArgs e)
    {
        // If visualizers can't keep up, drop frames rather than starve the UI thread.
        if (_skip > 0) { _skip--; return; }
        long tick = e is RenderingEventArgs r ? r.RenderingTime.Ticks : Environment.TickCount64;
        _cost.Restart();
        var frame = _analyzer.GetFrame(tick);
        foreach (var s in _subscribers.ToArray()) s(frame);
        double ms = _cost.Elapsed.TotalMilliseconds;
        if (ms > 12) _skip = Math.Min(4, (int)(ms / 12));
    }

    /// <summary>Called when the media session reports a new track.</summary>
    public void UpdateTrack(MediaSnapshot m)
    {
        _context.Title = m.Title;
        _context.Artist = m.Artist;
        _context.Album = m.Album;
        _context.Artwork = null;
        _context.ArtworkWidth = _context.ArtworkHeight = 0;
        if (m.Artwork != null)
        {
            try
            {
                var conv = new FormatConvertedBitmap(m.Artwork, PixelFormats.Bgr32, null, 0);
                int w = conv.PixelWidth, h = conv.PixelHeight;
                var px = new int[w * h];
                conv.CopyPixels(px, w * 4, 0);
                _context.Artwork = px;
                _context.ArtworkWidth = w;
                _context.ArtworkHeight = h;
            }
            catch (Exception ex) { Log.Write("Artwork convert failed: " + ex.Message); }
        }
        _context.ArtworkVersion++;
    }

    public void Dispose()
    {
        if (_hooked) CompositionTarget.Rendering -= OnRendering;
        _subscribers.Clear();
    }
}
