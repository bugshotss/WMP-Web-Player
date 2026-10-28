using System.Runtime.InteropServices;
using WebSkinPlayer.Audio;

namespace WebSkinPlayer.Visualization;

/// <summary>WMP 11-style "Album Art" view: the track's artwork with title and artist.</summary>
public sealed unsafe class AlbumArtVisualization : IVisualization
{
    public string Id => "AlbumArt";
    public string Title => "Album Art";
    public IReadOnlyList<string> Presets { get; } = new[] { "Album Art and Info", "Album Art" };
    public int CurrentPreset { get; set; }

    int _cachedVersion = -1, _cachedW, _cachedH;

    public void Render(VisFrame frame, PixelBuffer p, VisContext ctx)
    {
        // Static content: only redraw when the track or size changes.
        if (_cachedVersion == ctx.ArtworkVersion && _cachedW == p.Width && _cachedH == p.Height) return;
        _cachedVersion = ctx.ArtworkVersion; _cachedW = p.Width; _cachedH = p.Height;

        int w = p.Width, h = p.Height;
        for (int y = 0; y < h; y++)
        {
            int c = Draw.Lerp(Draw.Rgb(10, 18, 40), Draw.Rgb(0, 0, 0), y / (float)h);
            new Span<int>(p.Pixels + y * w, w).Fill(c);
        }

        bool info = CurrentPreset == 0 && h > 60;
        int textSpace = info ? Math.Max(28, h / 6) : 0;
        int size = Math.Max(8, Math.Min(w - 8, h - textSpace - 8));
        int ox = (w - size) / 2, oy = (h - textSpace - size) / 2;

        if (ctx.Artwork != null && ctx.ArtworkWidth > 0)
        {
            // Box-filtered downscale (or nearest upscale) into the square.
            for (int y = 0; y < size; y++)
            {
                int sy0 = y * ctx.ArtworkHeight / size, sy1 = Math.Max(sy0 + 1, (y + 1) * ctx.ArtworkHeight / size);
                for (int x = 0; x < size; x++)
                {
                    int sx0 = x * ctx.ArtworkWidth / size, sx1 = Math.Max(sx0 + 1, (x + 1) * ctx.ArtworkWidth / size);
                    int r = 0, g = 0, b = 0, n = 0;
                    for (int sy = sy0; sy < sy1; sy++)
                        for (int sx = sx0; sx < sx1; sx++)
                        {
                            int c = ctx.Artwork[sy * ctx.ArtworkWidth + sx];
                            r += (c >> 16) & 255; g += (c >> 8) & 255; b += c & 255; n++;
                        }
                    Draw.Set(p, ox + x, oy + y, Draw.Rgb(r / n, g / n, b / n));
                }
            }
        }
        else
        {
            // Placeholder disc, like WMP's default album art.
            float cx = ox + size / 2f, cy = oy + size / 2f, rad = size * 0.42f;
            for (int y = oy; y < oy + size; y++)
                for (int x = ox; x < ox + size; x++)
                {
                    float d = MathF.Sqrt((x - cx) * (x - cx) + (y - cy) * (y - cy)) / rad;
                    if (d > 1) continue;
                    float k = d < 0.12f ? 0.1f : 0.35f + 0.5f * (1 - d) + 0.15f * MathF.Sin(d * 60);
                    Draw.Set(p, x, y, Draw.Hsv(0.6, 0.35, Math.Clamp(k, 0, 1)));
                }
        }

        if (info)
        {
            string line = ctx.Artist.Length > 0 ? $"{ctx.Title} — {ctx.Artist}" : ctx.Title;
            DrawText(p, line, 0, h - textSpace, w, textSpace, Math.Clamp(textSpace / 2, 11, 22));
        }
    }

    static void DrawText(PixelBuffer p, string text, int x, int y, int w, int h, int px)
    {
        if (string.IsNullOrEmpty(text)) return;
        IntPtr font = CreateFont(-px, 0, 0, 0, 400, 0, 0, 0, 1, 0, 0, 5, 0, "Segoe UI");
        IntPtr old = SelectObject(p.Hdc, font);
        SetBkMode(p.Hdc, 1);
        SetTextColor(p.Hdc, 0x00F0F0F0);
        var rc = new RECT { left = x + 4, top = y, right = x + w - 4, bottom = y + h };
        DrawTextW(p.Hdc, text, -1, ref rc, 0x1 | 0x4 | 0x20 | 0x8000); // center | vcenter | singleline | end ellipsis
        SelectObject(p.Hdc, old);
        DeleteObject(font);
        PixelBuffer.Flush();
    }

    public void Dispose() { }

    [StructLayout(LayoutKind.Sequential)] struct RECT { public int left, top, right, bottom; }
    [DllImport("gdi32", CharSet = CharSet.Unicode)]
    static extern IntPtr CreateFont(int h, int w, int esc, int orient, int weight, uint italic, uint underline, uint strike, uint charset, uint outPrec, uint clipPrec, uint quality, uint pitch, string face);
    [DllImport("gdi32")] static extern IntPtr SelectObject(IntPtr hdc, IntPtr obj);
    [DllImport("gdi32")] static extern bool DeleteObject(IntPtr obj);
    [DllImport("gdi32")] static extern int SetBkMode(IntPtr hdc, int mode);
    [DllImport("gdi32")] static extern uint SetTextColor(IntPtr hdc, uint color);
    [DllImport("user32", CharSet = CharSet.Unicode)] static extern int DrawTextW(IntPtr hdc, string text, int len, ref RECT rc, uint format);
}
