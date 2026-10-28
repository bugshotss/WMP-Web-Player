using System.Runtime.InteropServices;

namespace WebSkinPlayer.Visualization;

/// <summary>
/// A 32bpp top-down GDI DIB section with its own memory DC. Built-in visualizations write the
/// pixels directly; hosted WMP plug-ins draw into <see cref="Hdc"/>.
/// </summary>
public sealed unsafe class PixelBuffer : IDisposable
{
    public int Width { get; }
    public int Height { get; }
    public int* Pixels { get; }
    public IntPtr Hdc { get; }
    public int Length => Width * Height;
    readonly IntPtr _dib, _old;

    public PixelBuffer(int width, int height)
    {
        Width = Math.Max(1, width);
        Height = Math.Max(1, height);
        var bmi = new BITMAPINFO
        {
            biSize = 40, biWidth = Width, biHeight = -Height, biPlanes = 1, biBitCount = 32, biCompression = 0,
        };
        Hdc = CreateCompatibleDC(IntPtr.Zero);
        _dib = CreateDIBSection(Hdc, ref bmi, 0, out IntPtr bits, IntPtr.Zero, 0);
        if (_dib == IntPtr.Zero) throw new InvalidOperationException("CreateDIBSection failed");
        _old = SelectObject(Hdc, _dib);
        Pixels = (int*)bits;
        Clear(0);
    }

    public Span<int> Span => new(Pixels, Length);

    public void Clear(int color) => Span.Fill(color);

    /// <summary>Makes GDI flush pending drawing into the DIB before we read the pixels.</summary>
    public static void Flush() => GdiFlush();

    public void Dispose()
    {
        SelectObject(Hdc, _old);
        DeleteObject(_dib);
        DeleteDC(Hdc);
    }

    [StructLayout(LayoutKind.Sequential)]
    struct BITMAPINFO
    {
        public int biSize, biWidth, biHeight;
        public short biPlanes, biBitCount;
        public int biCompression, biSizeImage, biXPelsPerMeter, biYPelsPerMeter, biClrUsed, biClrImportant;
        public int bmiColors;
    }

    [DllImport("gdi32")] static extern IntPtr CreateCompatibleDC(IntPtr hdc);
    [DllImport("gdi32")] static extern IntPtr CreateDIBSection(IntPtr hdc, ref BITMAPINFO bmi, uint usage, out IntPtr bits, IntPtr section, uint offset);
    [DllImport("gdi32")] static extern IntPtr SelectObject(IntPtr hdc, IntPtr obj);
    [DllImport("gdi32")] static extern bool DeleteObject(IntPtr obj);
    [DllImport("gdi32")] static extern bool DeleteDC(IntPtr hdc);
    [DllImport("gdi32")] static extern bool GdiFlush();
}

/// <summary>Software drawing primitives for the built-in visualizations (BGRA ints).</summary>
public static unsafe class Draw
{
    public static int Rgb(int r, int g, int b) => (Math.Clamp(r, 0, 255) << 16) | (Math.Clamp(g, 0, 255) << 8) | Math.Clamp(b, 0, 255);

    public static int Hsv(double h, double s, double v)
    {
        h = ((h % 1) + 1) % 1 * 6;
        int i = (int)h;
        double f = h - i, p = v * (1 - s), q = v * (1 - s * f), t = v * (1 - s * (1 - f));
        (double r, double g, double b) = i switch
        {
            0 => (v, t, p), 1 => (q, v, p), 2 => (p, v, t), 3 => (p, q, v), 4 => (t, p, v), _ => (v, p, q),
        };
        return Rgb((int)(r * 255), (int)(g * 255), (int)(b * 255));
    }

    public static int Scale(int c, float k)
    {
        int r = (int)(((c >> 16) & 255) * k), g = (int)(((c >> 8) & 255) * k), b = (int)((c & 255) * k);
        return Rgb(r, g, b);
    }

    public static int Lerp(int a, int b, float t)
    {
        int ar = (a >> 16) & 255, ag = (a >> 8) & 255, ab = a & 255;
        int br = (b >> 16) & 255, bg = (b >> 8) & 255, bb = b & 255;
        return Rgb((int)(ar + (br - ar) * t), (int)(ag + (bg - ag) * t), (int)(ab + (bb - ab) * t));
    }

    public static int AddSat(int a, int b)
    {
        int r = Math.Min(255, ((a >> 16) & 255) + ((b >> 16) & 255));
        int g = Math.Min(255, ((a >> 8) & 255) + ((b >> 8) & 255));
        int bl = Math.Min(255, (a & 255) + (b & 255));
        return (r << 16) | (g << 8) | bl;
    }

    public static void Add(PixelBuffer p, int x, int y, int color)
    {
        if ((uint)x >= (uint)p.Width || (uint)y >= (uint)p.Height) return;
        int* px = p.Pixels + y * p.Width + x;
        *px = AddSat(*px, color);
    }

    public static void Set(PixelBuffer p, int x, int y, int color)
    {
        if ((uint)x >= (uint)p.Width || (uint)y >= (uint)p.Height) return;
        p.Pixels[y * p.Width + x] = color;
    }

    public static void FillRect(PixelBuffer p, int x, int y, int w, int h, int color)
    {
        int x0 = Math.Max(0, x), y0 = Math.Max(0, y), x1 = Math.Min(p.Width, x + w), y1 = Math.Min(p.Height, y + h);
        for (int yy = y0; yy < y1; yy++)
            new Span<int>(p.Pixels + yy * p.Width + x0, Math.Max(0, x1 - x0)).Fill(color);
    }

    /// <summary>Anti-aliased-ish additive line (Wu-style coverage, configurable thickness).</summary>
    public static void Line(PixelBuffer p, float x0, float y0, float x1, float y1, int color, float thickness = 1f)
    {
        float dx = x1 - x0, dy = y1 - y0;
        float len = MathF.Max(MathF.Abs(dx), MathF.Abs(dy));
        int steps = Math.Max(1, (int)MathF.Ceiling(len));
        float half = MathF.Max(0.5f, thickness / 2f);
        bool steep = MathF.Abs(dy) > MathF.Abs(dx);
        for (int i = 0; i <= steps; i++)
        {
            float t = i / (float)steps;
            float x = x0 + dx * t, y = y0 + dy * t;
            if (steep)
            {
                int xi = (int)MathF.Floor(x - half), xe = (int)MathF.Ceiling(x + half);
                for (int xx = xi; xx <= xe; xx++)
                {
                    float cov = Math.Clamp(half + 0.5f - MathF.Abs(xx - x), 0f, 1f);
                    if (cov > 0) Add(p, xx, (int)y, Scale(color, cov));
                }
            }
            else
            {
                int yi = (int)MathF.Floor(y - half), ye = (int)MathF.Ceiling(y + half);
                for (int yy = yi; yy <= ye; yy++)
                {
                    float cov = Math.Clamp(half + 0.5f - MathF.Abs(yy - y), 0f, 1f);
                    if (cov > 0) Add(p, (int)x, yy, Scale(color, cov));
                }
            }
        }
    }

    /// <summary>Additive soft round dot.</summary>
    public static void Dot(PixelBuffer p, float cx, float cy, float radius, int color)
    {
        int x0 = (int)(cx - radius - 1), x1 = (int)(cx + radius + 1), y0 = (int)(cy - radius - 1), y1 = (int)(cy + radius + 1);
        float r2 = radius * radius;
        for (int y = y0; y <= y1; y++)
            for (int x = x0; x <= x1; x++)
            {
                float d2 = (x - cx) * (x - cx) + (y - cy) * (y - cy);
                if (d2 > r2) continue;
                float k = 1f - d2 / r2;
                Add(p, x, y, Scale(color, k * k));
            }
    }

    /// <summary>Multiply every pixel by <paramref name="k"/> and subtract a floor so trails die out.</summary>
    public static void Fade(PixelBuffer p, float k, int floor = 1)
    {
        int kk = (int)(k * 256);
        int* px = p.Pixels;
        for (int i = 0, n = p.Length; i < n; i++)
        {
            int c = px[i];
            if (c == 0) continue;
            int r = Math.Max(0, ((((c >> 16) & 255) * kk) >> 8) - floor);
            int g = Math.Max(0, ((((c >> 8) & 255) * kk) >> 8) - floor);
            int b = Math.Max(0, (((c & 255) * kk) >> 8) - floor);
            px[i] = (r << 16) | (g << 8) | b;
        }
    }
}
