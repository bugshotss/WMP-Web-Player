using WebSkinPlayer.Audio;

namespace WebSkinPlayer.Visualization;

/// <summary>Recreation of WMP's "Bars and Waves" visualization and its four presets.</summary>
public sealed unsafe class BarsAndWaves : IVisualization
{
    public const string ClassId = "{48501FF0-F6A9-11D2-9435-00A0C92A2F2D}";
    public string Id => "Bars";
    public string Title => "Bars and Waves";
    public IReadOnlyList<string> Presets { get; } = new[] { "Bars", "Ocean Mist", "Fire Storm", "Scope" };
    public int CurrentPreset { get; set; }

    float[] _bars = Array.Empty<float>(), _peaks = Array.Empty<float>(), _peakHold = Array.Empty<float>();
    int[] _scratch = Array.Empty<int>();
    double _hue;

    public void Render(VisFrame f, PixelBuffer p, VisContext ctx)
    {
        switch (CurrentPreset)
        {
            case 1: OceanMist(f, p); break;
            case 2: FireStorm(f, p); break;
            case 3: Scope(f, p); break;
            default: Bars(f, p); break;
        }
    }

    /// <summary>Log-spaced band value (0..1) between two fractions of the usable spectrum.</summary>
    static float Band(VisFrame f, int band, int bands)
    {
        double lo = 40, hi = 16000;
        double binHz = f.SampleRate / 2048.0;
        double f0 = lo * Math.Pow(hi / lo, band / (double)bands);
        double f1 = lo * Math.Pow(hi / lo, (band + 1) / (double)bands);
        double c0 = f0 / binHz, c1 = f1 / binHz;
        if (c1 - c0 < 1.5)
        {
            // Narrower than an FFT bin (the bass end): interpolate at the band centre.
            double c = Math.Clamp((c0 + c1) / 2, 1, VisFrame.Bins - 2);
            int i0 = (int)c;
            float t = (float)(c - i0);
            return f.Spectrum[i0] * (1 - t) + f.Spectrum[i0 + 1] * t;
        }
        int b0 = Math.Clamp((int)c0, 1, VisFrame.Bins - 1);
        int b1 = Math.Clamp((int)Math.Ceiling(c1), b0 + 1, VisFrame.Bins);
        float v = 0;
        for (int i = b0; i < b1; i++) v = MathF.Max(v, f.Spectrum[i]);
        return v;
    }

    void Bars(VisFrame f, PixelBuffer p)
    {
        int w = p.Width, h = p.Height;
        int n = Math.Clamp(w / 7, 12, 80);
        if (_bars.Length != n) { _bars = new float[n]; _peaks = new float[n]; _peakHold = new float[n]; }
        p.Clear(0);
        _hue += 0.0015;
        float barW = w / (float)n;
        int gap = barW >= 5 ? 1 : 0;
        for (int i = 0; i < n; i++)
        {
            float v = Band(f, i, n);
            v = MathF.Pow(v, 1.4f);
            _bars[i] = v > _bars[i] ? v : MathF.Max(v, _bars[i] - 0.035f);
            if (_bars[i] >= _peaks[i]) { _peaks[i] = _bars[i]; _peakHold[i] = 18; }
            else if (_peakHold[i] > 0) _peakHold[i]--;
            else _peaks[i] = MathF.Max(0, _peaks[i] - 0.012f);

            int x0 = (int)(i * barW), x1 = (int)((i + 1) * barW) - gap;
            int top = h - (int)(_bars[i] * (h - 3));
            for (int y = top; y < h; y++)
            {
                float t = 1f - (y / (float)h);
                int c = Draw.Hsv(_hue + 0.55 + t * 0.18, 0.85 - t * 0.5, 0.45 + t * 0.55);
                for (int x = x0; x < x1; x++) Draw.Set(p, x, y, c);
            }
            int py = h - 2 - (int)(_peaks[i] * (h - 3));
            Draw.FillRect(p, x0, py, Math.Max(1, x1 - x0), 2, Draw.Rgb(230, 235, 255));
        }
    }

    void OceanMist(VisFrame f, PixelBuffer p)
    {
        int w = p.Width, h = p.Height;
        DriftAndBlur(p, -1, 0.93f);
        _hue += 0.002;
        int n = Math.Max(16, w / 3);
        const int bands = 48;
        Span<float> bv = stackalloc float[bands + 1];
        for (int b = 0; b < bands; b++) bv[b] = Band(f, b, bands);
        bv[bands] = bv[bands - 1];
        float mid = h * 0.5f;
        float px = 0, pyTop = mid, pyBot = mid;
        for (int i = 0; i <= n; i++)
        {
            // Smoothstep between neighbouring bands so the mist is a curve, not steps.
            float pos = i / (float)n * (bands - 1);
            int b0 = (int)pos;
            float t = pos - b0;
            t = t * t * (3 - 2 * t);
            float v = bv[b0] * (1 - t) + bv[b0 + 1] * t;
            float x = i * w / (float)n;
            float amp = v * h * 0.42f;
            float yTop = mid - amp, yBot = mid + amp;
            int c = Draw.Hsv(_hue + 0.5 + v * 0.12, 0.55, 0.35 + v * 0.65);
            if (i > 0)
            {
                Draw.Line(p, px, pyTop, x, yTop, c, 1.5f);
                Draw.Line(p, px, pyBot, x, yBot, Draw.Scale(c, 0.7f), 1.5f);
            }
            px = x; pyTop = yTop; pyBot = yBot;
        }
    }

    void FireStorm(VisFrame f, PixelBuffer p)
    {
        int w = p.Width, h = p.Height;
        DriftAndBlur(p, -2, 0.90f);
        var rng = Random.Shared;
        int n = Math.Max(16, w / 2);
        for (int i = 0; i < n; i++)
        {
            float v = Band(f, (int)(i / (float)n * 40), 40);
            float x = i * w / (float)n;
            float flame = v * h * 0.55f;
            for (int k = 0; k < 2; k++)
            {
                float y = h - 1 - (float)rng.NextDouble() * flame;
                int c = Draw.Rgb(255, (int)(120 + 135 * v), (int)(60 * v));
                Draw.Dot(p, x + (float)rng.NextDouble() * 2, y, 1.5f + v * 2.5f, c);
            }
        }
    }

    void Scope(VisFrame f, PixelBuffer p)
    {
        int w = p.Width, h = p.Height;
        Draw.Fade(p, 0.55f, 4);
        _hue += 0.004;
        int c = Draw.Hsv(_hue, 0.7, 1.0);
        float mid = h * 0.5f, amp = h * 0.45f;
        float lx = 0, ly = mid;
        for (int x = 0; x < w; x++)
        {
            int i = (int)(x / (float)w * (VisFrame.Bins - 1));
            float y = mid - (f.WaveL[i] + f.WaveR[i]) * 0.5f * amp * 1.6f;
            if (x > 0) Draw.Line(p, lx, ly, x, y, c, 1.4f);
            lx = x; ly = y;
        }
    }

    /// <summary>Scrolls the image vertically by <paramref name="dy"/> with a light blur and fade.</summary>
    void DriftAndBlur(PixelBuffer p, int dy, float decay)
    {
        int w = p.Width, h = p.Height;
        if (_scratch.Length != w * h) _scratch = new int[w * h];
        var src = p.Pixels;
        int k = (int)(decay * 256);
        fixed (int* dst = _scratch)
        {
            for (int y = 0; y < h; y++)
            {
                int sy = y - dy;
                for (int x = 0; x < w; x++)
                {
                    if ((uint)sy >= (uint)h) { dst[y * w + x] = 0; continue; }
                    int xl = x > 0 ? x - 1 : x, xr = x < w - 1 ? x + 1 : x;
                    int a = src[sy * w + xl], b = src[sy * w + x], c = src[sy * w + xr];
                    int r = ((((a >> 16) & 255) + 2 * ((b >> 16) & 255) + ((c >> 16) & 255)) * k) >> 10;
                    int g = ((((a >> 8) & 255) + 2 * ((b >> 8) & 255) + ((c >> 8) & 255)) * k) >> 10;
                    int bl = (((a & 255) + 2 * (b & 255) + (c & 255)) * k) >> 10;
                    dst[y * w + x] = (Math.Max(0, r - 1) << 16) | (Math.Max(0, g - 1) << 8) | Math.Max(0, bl - 1);
                }
            }
            Buffer.MemoryCopy(dst, src, (long)w * h * 4, (long)w * h * 4);
        }
    }

    public void Dispose() { }
}
