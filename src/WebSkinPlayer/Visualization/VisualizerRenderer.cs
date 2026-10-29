using System.Windows;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using WebSkinPlayer.Audio;

namespace WebSkinPlayer.Visualization;

/// <summary>
/// Owns one visualization instance plus the bitmap it renders into. Used both by skin
/// &lt;EFFECTS&gt; elements and by the full-mode Now Playing view.
/// </summary>
public sealed unsafe class VisualizerRenderer : IDisposable
{
    IVisualization? _vis;
    PixelBuffer? _buffer;
    public WriteableBitmap? Bitmap { get; private set; }
    public VisDescriptor? Descriptor { get; private set; }
    public event Action? BitmapChanged;

    /// <summary>Upper bound on internal resolution; larger surfaces are scaled up when drawn.</summary>
    public int MaxPixels { get; set; } = 960 * 600;

    public IVisualization? Visualization => _vis;
    public string EffectId => Descriptor?.Id ?? "";
    public int Preset => _vis?.CurrentPreset ?? 0;
    public IReadOnlyList<string> Presets => _vis?.Presets ?? Array.Empty<string>();

    public void SetEffect(VisDescriptor descriptor, int preset)
    {
        if (Descriptor?.Id != descriptor.Id || _vis == null)
        {
            _vis?.Dispose();
            _vis = descriptor.Create() ?? VisualizationCatalog.Default.Create();
            Descriptor = descriptor;
            _buffer?.Clear(0);
        }
        if (_vis != null) _vis.CurrentPreset = Math.Clamp(preset, 0, Math.Max(0, _vis.Presets.Count - 1));
    }

    /// <summary>Ensures the bitmap matches the requested pixel size (after the MaxPixels cap).</summary>
    public void Resize(int width, int height)
    {
        width = Math.Max(1, width); height = Math.Max(1, height);
        int cap = MaxPixels;
        int hint = _vis?.MaxPixelsHint ?? 0;
        if (hint > 0) cap = Math.Min(cap, hint);
        double scale = Math.Sqrt(Math.Min(1.0, cap / (double)(width * height)));
        int w = Math.Max(1, (int)(width * scale)), h = Math.Max(1, (int)(height * scale));
        if (_buffer != null && _buffer.Width == w && _buffer.Height == h) return;
        _buffer?.Dispose();
        _buffer = new PixelBuffer(w, h);
        Bitmap = new WriteableBitmap(w, h, 96, 96, PixelFormats.Bgr32, null);
        BitmapChanged?.Invoke();
    }

    public void Render(VisFrame frame, VisContext context)
    {
        if (_vis == null || _buffer == null || Bitmap == null) return;
        try { _vis.Render(frame, _buffer, context); }
        catch (Exception ex) { Log.Write($"Visualization {EffectId} error: {ex.Message}"); }
        var all = new Int32Rect(0, 0, _buffer.Width, _buffer.Height);
        // Recoloured into a picture of its own: visualizations that build on their last frame (Battery's trails, the
        // plug-ins' pictures between helper frames) must find it as they drew it.
        if (VisualizationColors.MapFor(EffectId) is { } colors)
        {
            if (_colored?.Length != _buffer.Length) _colored = new int[_buffer.Length];
            colors.Apply(_buffer.Pixels, _colored);
            Bitmap.WritePixels(all, _colored, _buffer.Width * 4, 0);
        }
        else Bitmap.WritePixels(all, (IntPtr)_buffer.Pixels, _buffer.Length * 4, _buffer.Width * 4);
    }

    int[]? _colored;

    public void Dispose()
    {
        _vis?.Dispose();
        _vis = null;
        _buffer?.Dispose();
        _buffer = null;
    }
}
