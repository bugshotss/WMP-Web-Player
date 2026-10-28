using WebSkinPlayer.Audio;

namespace WebSkinPlayer.Visualization;

/// <summary>
/// Battery-style visualization: every frame warps the previous frame (zoom / rotate / swirl /
/// drift / ripple), fades it, and draws fresh audio-driven shapes on top.
/// </summary>
public sealed unsafe class Battery : IVisualization
{
    public const string ClassId = "{8D2A317B-D98B-4efa-89D6-E49F0ACF98A1}";
    public string Id => "Battery";
    public string Title => "Battery";
    public IReadOnlyList<string> Presets { get; } = new[]
    {
        "Whirlpool", "Event Horizon", "Spider Silk", "Smoke and Mirrors",
        "Kaleidoscope", "Pixel-ate", "Gravity", "Starburst",
    };
    public int CurrentPreset { get; set; }
    // Whole-frame warping is per-pixel work; WMP's own Battery also ran at a reduced resolution.
    public int MaxPixelsHint => 560 * 350;

    int[] _back = Array.Empty<int>();
    float[] _rowRipple = Array.Empty<float>(), _colRipple = Array.Empty<float>();
    double _hue, _spin;
    readonly Random _rng = new(7);
    readonly List<(float x, float y, float vx, float vy, int c, float life)> _sparks = new();

    struct Warp
    {
        public float Zoom, Rotation, Swirl, Dy, Ripple, RippleFreq, Decay, Cx, Cy;
    }

    public void Render(VisFrame f, PixelBuffer p, VisContext ctx)
    {
        int w = p.Width, h = p.Height;
        if (_back.Length != w * h) _back = new int[w * h];
        _hue += 0.0012 + f.Level * 0.004;
        _spin += 0.01 + f.Bass * 0.03;

        var wp = new Warp { Zoom = 1, Decay = 0.95f, Cx = 0.5f, Cy = 0.5f };
        switch (CurrentPreset)
        {
            case 0: wp.Zoom = 0.975f; wp.Rotation = 0.015f + f.Bass * 0.04f; wp.Swirl = 0.07f; wp.Decay = 0.955f; break;
            case 1: wp.Zoom = 0.94f - f.Bass * 0.03f; wp.Rotation = 0.004f; wp.Decay = 0.94f; break;
            case 2: wp.Zoom = 0.995f; wp.Rotation = 0.012f; wp.Ripple = 0.6f; wp.RippleFreq = 0.05f; wp.Decay = 0.965f; break;
            case 3: wp.Dy = -1.6f - f.Mid * 2f; wp.Ripple = 1.6f; wp.RippleFreq = 0.035f; wp.Decay = 0.95f; wp.Zoom = 0.998f; break;
            case 4: wp.Zoom = 0.985f; wp.Rotation = 0.03f * MathF.Sin((float)_spin * 0.3f); wp.Decay = 0.94f; break;
            case 5: wp.Zoom = 1.035f; wp.Decay = 0.90f; break;
            case 6: wp.Zoom = 1.025f + f.Bass * 0.02f; wp.Rotation = -0.012f; wp.Swirl = -0.05f; wp.Decay = 0.955f; break;
            case 7: wp.Zoom = 0.955f; wp.Rotation = 0.006f; wp.Decay = 0.93f; break;
        }

        WarpInto(p, wp, f.Time);
        DrawShapes(f, p);

        if (CurrentPreset == 3) MirrorX(p);
        if (CurrentPreset == 4) { MirrorX(p); MirrorY(p); }
        if (CurrentPreset == 5) Pixelate(p, Math.Max(4, Math.Min(w, h) / 40));
    }

    void WarpInto(PixelBuffer p, Warp wp, double time)
    {
        int w = p.Width, h = p.Height;
        fixed (int* back = _back) Buffer.MemoryCopy(p.Pixels, back, (long)w * h * 4, (long)w * h * 4);

        if (_rowRipple.Length != h) _rowRipple = new float[h];
        if (_colRipple.Length != w) _colRipple = new float[w];
        float t = (float)time * 2.2f;
        for (int y = 0; y < h; y++) _rowRipple[y] = wp.Ripple * MathF.Sin(y * wp.RippleFreq + t);
        for (int x = 0; x < w; x++) _colRipple[x] = wp.Ripple * MathF.Cos(x * wp.RippleFreq * 1.3f + t * 0.8f);

        float cx = wp.Cx * w, cy = wp.Cy * h;
        float rmax = MathF.Sqrt(cx * cx + cy * cy);
        float cosR = MathF.Cos(wp.Rotation), sinR = MathF.Sin(wp.Rotation);
        int decay = (int)(wp.Decay * 256);
        IntPtr dstPtr = (IntPtr)p.Pixels;
        var rows = _rowRipple; var cols = _colRipple;

        // _back is a managed array; pin it for the parallel loop.
        var handle = System.Runtime.InteropServices.GCHandle.Alloc(_back, System.Runtime.InteropServices.GCHandleType.Pinned);
        try
        {
            IntPtr srcPtr = handle.AddrOfPinnedObject();
            void Row(int y)
            {
                int* src = (int*)srcPtr, dst = (int*)dstPtr;
                float dy0 = y - cy;
                for (int x = 0; x < w; x++)
                {
                    float dx0 = x - cx;
                    float c = cosR, s = sinR;
                    if (wp.Swirl != 0)
                    {
                        float r = MathF.Sqrt(dx0 * dx0 + dy0 * dy0) / rmax;
                        float a = wp.Rotation + wp.Swirl * (1 - r) * (1 - r);
                        c = MathF.Cos(a); s = MathF.Sin(a);
                    }
                    float u = cx + (dx0 * c - dy0 * s) * wp.Zoom + rows[y];
                    float v = cy + (dx0 * s + dy0 * c) * wp.Zoom - wp.Dy + cols[x];
                    dst[y * w + x] = Sample(src, w, h, u, v, decay);
                }
            }
            if (w * h > 60_000) Parallel.For(0, h, Row);
            else for (int y = 0; y < h; y++) Row(y);
        }
        finally { handle.Free(); }
    }

    static int Sample(int* src, int w, int h, float u, float v, int decay)
    {
        if (u < 0 || v < 0 || u >= w - 1 || v >= h - 1) return 0;
        int x0 = (int)u, y0 = (int)v;
        int fx = (int)((u - x0) * 256), fy = (int)((v - y0) * 256);
        int* row0 = src + y0 * w + x0;
        int* row1 = row0 + w;
        int c00 = row0[0], c10 = row0[1], c01 = row1[0], c11 = row1[1];
        int Ch(int shift)
        {
            int a = (c00 >> shift) & 255, b = (c10 >> shift) & 255, c = (c01 >> shift) & 255, d = (c11 >> shift) & 255;
            int top = a * (256 - fx) + b * fx, bot = c * (256 - fx) + d * fx;
            int val = ((top * (256 - fy) + bot * fy) >> 16) * decay >> 8;
            return val > 1 ? val - 1 : 0;
        }
        return (Ch(16) << 16) | (Ch(8) << 8) | Ch(0);
    }

    void DrawShapes(VisFrame f, PixelBuffer p)
    {
        int w = p.Width, h = p.Height;
        float cx = w / 2f, cy = h / 2f, m = Math.Min(w, h);
        switch (CurrentPreset)
        {
            case 0: // Whirlpool: spectrum ring
            {
                int n = 64;
                float r0 = m * 0.18f;
                for (int i = 0; i < n; i++)
                {
                    float v = f.Spectrum[2 + i * 6];
                    double a = i / (double)n * Math.PI * 2 + _spin;
                    float r = r0 + v * m * 0.22f;
                    int c = Draw.Hsv(_hue + i / (double)n * 0.3, 0.9, 0.6 + v * 0.4);
                    Draw.Dot(p, cx + (float)Math.Cos(a) * r, cy + (float)Math.Sin(a) * r, 1.2f + v * 2.5f, c);
                }
                break;
            }
            case 1: // Event Horizon: waveform circle
            {
                int n = 180;
                float r0 = m * 0.22f;
                float px = 0, py = 0;
                int c = Draw.Hsv(_hue + 0.6, 0.35, 1.0);
                for (int i = 0; i <= n; i++)
                {
                    double a = i / (double)n * Math.PI * 2;
                    float wv = f.WaveL[(i * 5) % VisFrame.Bins];
                    float r = r0 * (1 + wv * 0.8f);
                    float x = cx + (float)Math.Cos(a) * r, y = cy + (float)Math.Sin(a) * r;
                    if (i > 0) Draw.Line(p, px, py, x, y, c, 1.3f);
                    px = x; py = y;
                }
                if (f.Beat) Draw.Dot(p, cx, cy, m * 0.12f, Draw.Hsv(_hue, 0.6, 0.7));
                break;
            }
            case 2: // Spider Silk: radial threads
            {
                int n = 24;
                for (int i = 0; i < n; i++)
                {
                    float v = MathF.Max(f.Spectrum[3 + i * 8], f.Spectrum[4 + i * 8]);
                    double a = i / (double)n * Math.PI * 2 + _spin * 0.2;
                    float r = m * (0.08f + v * 0.45f);
                    int c = Draw.Hsv(_hue + 0.75, 0.25, 0.5 + v * 0.5);
                    Draw.Line(p, cx, cy, cx + (float)Math.Cos(a) * r, cy + (float)Math.Sin(a) * r, c, 1f);
                }
                break;
            }
            case 3: // Smoke and Mirrors: waveform near the bottom (mirrored later)
            {
                float y0 = h * 0.78f;
                float px = 0, py = y0;
                int c = Draw.Hsv(_hue + 0.08, 0.8, 0.9);
                for (int x = 0; x <= w / 2; x += 2)
                {
                    float y = y0 + f.WaveL[(x * 3) % VisFrame.Bins] * h * 0.25f;
                    if (x > 0) Draw.Line(p, px, py, x, y, c, 2f);
                    px = x; py = y;
                }
                break;
            }
            case 4: // Kaleidoscope: spectrum confetti in one quadrant (mirrored later)
            {
                for (int i = 0; i < 24; i++)
                {
                    int bin = 2 + _rng.Next(200);
                    float v = f.Spectrum[bin];
                    if (v < 0.25f) continue;
                    float x = (float)_rng.NextDouble() * cx, y = (float)_rng.NextDouble() * cy;
                    Draw.Dot(p, x, y, 1 + v * m * 0.025f, Draw.Hsv(_hue + bin / 300.0, 0.85, v));
                }
                break;
            }
            case 5: // Pixel-ate: blocks driven by bands
            {
                int bands = 16;
                for (int i = 0; i < bands; i++)
                {
                    float v = f.Spectrum[2 + i * 12];
                    if (v < 0.3f) continue;
                    float x = (float)_rng.NextDouble() * w, y = (float)_rng.NextDouble() * h;
                    int s = (int)(m * 0.03f + v * m * 0.06f);
                    Draw.FillRect(p, (int)x, (int)y, s, s, Draw.Hsv(_hue + i / (double)bands, 0.9, v));
                }
                break;
            }
            case 6: // Gravity: sparks thrown from the rim, pulled inward by the warp
            {
                if (f.Beat || _rng.NextDouble() < f.Level * 0.5)
                    for (int i = 0; i < 6; i++)
                    {
                        double a = _rng.NextDouble() * Math.PI * 2;
                        float r = m * 0.45f;
                        Draw.Dot(p, cx + (float)Math.Cos(a) * r, cy + (float)Math.Sin(a) * r, 2 + f.Bass * 5, Draw.Hsv(_hue + _rng.NextDouble() * 0.2, 0.8, 1));
                    }
                float px = 0, py = 0;
                for (int i = 0; i <= 120; i++)
                {
                    double a = i / 120.0 * Math.PI * 2;
                    float r = m * 0.4f * (1 + f.WaveR[(i * 8) % VisFrame.Bins] * 0.3f);
                    float x = cx + (float)Math.Cos(a) * r, y = cy + (float)Math.Sin(a) * r;
                    if (i > 0) Draw.Line(p, px, py, x, y, Draw.Hsv(_hue + 0.5, 0.6, 0.6), 1f);
                    px = x; py = y;
                }
                break;
            }
            case 7: // Starburst: sparks flying out from the centre
            {
                int spawn = (int)(2 + f.Level * 10 + (f.Beat ? 30 : 0));
                for (int i = 0; i < spawn && _sparks.Count < 600; i++)
                {
                    double a = _rng.NextDouble() * Math.PI * 2;
                    float sp = (float)(0.5 + _rng.NextDouble() * 2.5) * (m / 200f) * (1 + f.Bass);
                    _sparks.Add((cx, cy, (float)Math.Cos(a) * sp, (float)Math.Sin(a) * sp, Draw.Hsv(_hue + _rng.NextDouble() * 0.25, 0.7, 1), 1f));
                }
                for (int i = _sparks.Count - 1; i >= 0; i--)
                {
                    var s = _sparks[i];
                    s.x += s.vx; s.y += s.vy; s.vx *= 1.02f; s.vy *= 1.02f; s.life -= 0.012f;
                    if (s.life <= 0 || s.x < 0 || s.y < 0 || s.x >= w || s.y >= h) { _sparks.RemoveAt(i); continue; }
                    Draw.Add(p, (int)s.x, (int)s.y, Draw.Scale(s.c, s.life));
                    _sparks[i] = s;
                }
                break;
            }
        }
    }

    static void MirrorX(PixelBuffer p)
    {
        int w = p.Width, h = p.Height;
        for (int y = 0; y < h; y++)
        {
            int* row = p.Pixels + y * w;
            for (int x = 0; x < w / 2; x++) row[w - 1 - x] = row[x];
        }
    }

    static void MirrorY(PixelBuffer p)
    {
        int w = p.Width, h = p.Height;
        for (int y = 0; y < h / 2; y++)
            Buffer.MemoryCopy(p.Pixels + y * w, p.Pixels + (h - 1 - y) * w, w * 4L, w * 4L);
    }

    static void Pixelate(PixelBuffer p, int block)
    {
        int w = p.Width, h = p.Height;
        for (int by = 0; by < h; by += block)
            for (int bx = 0; bx < w; bx += block)
            {
                int sx = Math.Min(w - 1, bx + block / 2), sy = Math.Min(h - 1, by + block / 2);
                int c = p.Pixels[sy * w + sx];
                for (int y = by; y < Math.Min(h, by + block); y++)
                    new Span<int>(p.Pixels + y * w + bx, Math.Min(block, w - bx)).Fill(c);
            }
    }

    public void Dispose() => _sparks.Clear();
}
