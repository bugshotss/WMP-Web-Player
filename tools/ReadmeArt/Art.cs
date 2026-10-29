using System.IO;
using System.Windows;
using System.Windows.Media;
using System.Windows.Media.Imaging;

namespace ReadmeArt;

/// <summary>The README's own artwork, in the style of the early-2000s web: a glossy banner, 88x31 buttons and a
/// big green download button.</summary>
static class Art
{
    public static void All(string outDir, string icoPath)
    {
        var icon = Render.Icon(icoPath);
        Render.Png(Banner(icon), Path.Combine(outDir, "banner.png"));
        Render.Png(Download(), Path.Combine(outDir, "download.png"));
        foreach (var (name, image) in Buttons())
            Render.Png(image, Path.Combine(outDir, $"button-{name}.png"));
    }

    // ---- banner ---------------------------------------------------------------------------

    static BitmapSource Banner(BitmapSource icon)
    {
        const int W = 880, H = 230;
        var sky = Scenery.Backdrop(W, H, 11);
        return Render.Drawing(W, H, dc =>
        {
            dc.DrawImage(sky, new Rect(0, 0, W, H));
            var band = new Rect(22, 30, W - 44, 170);
            dc.DrawRoundedRectangle(Render.Vertical((0, "#B8163A80"), (1, "#D0081A44")), new Pen(new SolidColorBrush(Color.FromArgb(0xD0, 0x8C, 0xBE, 0xFF)), 1.5), band, 16, 16);
            dc.PushClip(new RectangleGeometry(new Rect(band.X, band.Y, band.Width, band.Height * 0.46), 16, 16));
            dc.DrawRoundedRectangle(Render.Vertical((0, "#55FFFFFF"), (1, "#08FFFFFF")), null, band, 16, 16);
            dc.Pop();

            dc.DrawImage(icon, new Rect(48, 51, 128, 128));

            var title = Render.Text("WebSkin Player", "Trebuchet MS", 64, Brushes.White, bold: true, italic: true).BuildGeometry(new Point(196, 44));
            dc.PushTransform(new TranslateTransform(3, 4));
            dc.DrawGeometry(new SolidColorBrush(Color.FromArgb(0x90, 0, 0, 0)), null, title);
            dc.Pop();
            dc.DrawGeometry(Render.Vertical((0, "#FFFFFF"), (0.55, "#E4F0FF"), (1, "#9CC4FF")), new Pen(new SolidColorBrush(Render.Rgb("#0A2D6E")), 2), title);

            var tagline = new SolidColorBrush(Render.Rgb("#D6E6FF"));
            dc.DrawText(Render.Text("Classic Windows Media Player skins & visualizations", "Tahoma", 19, Brushes.White, bold: true), new Point(200, 128));
            dc.DrawText(Render.Text("for the music playing in your browser", "Tahoma", 17, tagline), new Point(200, 154));
        });
    }

    // ---- download button ------------------------------------------------------------------

    static BitmapSource Download()
    {
        const int W = 300, H = 76;
        return Render.Drawing(W, H, dc =>
        {
            var body = new Rect(2, 2, W - 4, H - 6);
            dc.DrawRoundedRectangle(new SolidColorBrush(Color.FromArgb(0x50, 0, 0, 0)), null, new Rect(body.X + 1, body.Y + 3, body.Width, body.Height), 14, 14);
            dc.DrawRoundedRectangle(Render.Vertical((0, "#7BD65A"), (0.48, "#43AE2C"), (0.52, "#36A021"), (1, "#2C8A19")), new Pen(new SolidColorBrush(Render.Rgb("#1D6510")), 1.5), body, 14, 14);
            dc.PushClip(new RectangleGeometry(new Rect(body.X, body.Y, body.Width, body.Height * 0.48), 14, 14));
            dc.DrawRoundedRectangle(Render.Vertical((0, "#90FFFFFF"), (1, "#20FFFFFF")), null, new Rect(body.X + 3, body.Y + 2, body.Width - 6, body.Height), 12, 12);
            dc.Pop();

            var disc = new Point(40, body.Y + body.Height / 2);
            dc.DrawEllipse(Render.Vertical((0, "#FFFFFF"), (1, "#D7EED0")), new Pen(new SolidColorBrush(Render.Rgb("#2A7F1A")), 1.5), disc, 24, 24);
            dc.DrawGeometry(Render.Vertical((0, "#4CB834"), (1, "#23780F")), null,
                Geometry.Parse($"M {disc.X - 6},{disc.Y - 14} h 12 v 12 h 8 l -14 15 l -14 -15 h 8 z"));

            var label = Render.Text("Download", "Trebuchet MS", 30, Brushes.White, bold: true);
            dc.DrawText(Render.Text("Download", "Trebuchet MS", 30, new SolidColorBrush(Render.Rgb("#1A5A0E")), bold: true), new Point(78, 9));
            dc.DrawText(label, new Point(77, 8));
            dc.DrawText(Render.Text("Free for Windows 10 & 11", "Tahoma", 13, Brushes.White, bold: true), new Point(80, 46));
        });
    }

    // ---- 88x31 buttons --------------------------------------------------------------------

    static IEnumerable<(string Name, BitmapSource Image)> Buttons()
    {
        yield return ("download", Button88("#5FC24A", "#237A15", DownArrow, "DOWNLOAD", "IT NOW!"));
        yield return ("windows", Button88("#4E8DF0", "#173F9E", Window, "WINDOWS", "10 & 11"));
        yield return ("skins", Button88("#FFB13D", "#B35400", Blob, "336 SKINS", "TESTED"));
        yield return ("plugins", Button88("#A070F0", "#4A1E99", Bars, "WMP 7-10", "PLUG-INS"));
        yield return ("webmusic", Button88("#31C6D8", "#0B6B85", Notes, "WEB MUSIC", "READY"));
        yield return ("noadmin", Button88("#C9CED6", "#6B7380", Padlock, "NO ADMIN", "NEEDED"));
        yield return ("dotnet", Button88("#8A63E8", "#3B1E8C", Label(".NET", "#FFFFFF"), "POWERED", "BY .NET 10"));
        yield return ("jint", Button88("#F2D04B", "#A0780B", Label("JS", "#3A2A00"), "SCRIPTS", "BY JINT"));
        yield return ("naudio", Button88("#5C6E86", "#1E2838", Speaker, "AUDIO BY", "NAUDIO"));
        yield return ("1024", Button88("#2F3B4F", "#05080F", Monitor, "BEST VIEWED", "1024x768"));
    }

    static BitmapSource Button88(string top, string bottom, Action<DrawingContext> glyph, string line1, string line2) =>
        Render.Drawing(88, 31, dc =>
        {
            dc.DrawRectangle(Brushes.Black, null, new Rect(0, 0, 88, 31));
            dc.DrawRectangle(Render.Vertical((0, top), (1, bottom)), null, new Rect(1, 1, 86, 29));
            var light = new SolidColorBrush(Color.FromArgb(0xC0, 255, 255, 255));
            var dark = new SolidColorBrush(Color.FromArgb(0x70, 0, 0, 0));
            dc.DrawRectangle(light, null, new Rect(1, 1, 86, 1));
            dc.DrawRectangle(light, null, new Rect(1, 1, 1, 29));
            dc.DrawRectangle(dark, null, new Rect(1, 29, 86, 1));
            dc.DrawRectangle(dark, null, new Rect(86, 1, 1, 29));
            dc.DrawRectangle(new SolidColorBrush(Color.FromArgb(0x38, 0, 0, 0)), null, new Rect(3, 3, 25, 25));
            dc.DrawRectangle(dark, null, new Rect(29, 3, 1, 25));
            dc.PushTransform(new TranslateTransform(3, 3));
            glyph(dc);
            dc.Pop();
            double size = 10;
            while (size > 7.5 && new[] { line1, line2 }.Any(l => Render.Text(l, "Tahoma", size, Brushes.White, bold: true).WidthIncludingTrailingWhitespace > 54)) size -= 0.5;
            Centered(dc, line1, size, 31, 87, 4);
            Centered(dc, line2, size, 31, 87, 16);
        }, aliased: true);

    /// <summary>White text with a hard shadow, centred between <paramref name="left"/> and <paramref name="right"/>.</summary>
    static void Centered(DrawingContext dc, string text, double size, double left, double right, double y)
    {
        var t = Render.Text(text, "Tahoma", size, Brushes.White, bold: true);
        double x = Math.Round(left + (right - left - t.WidthIncludingTrailingWhitespace) / 2);
        dc.DrawText(Render.Text(text, "Tahoma", size, new SolidColorBrush(Color.FromArgb(0xB0, 0, 0, 0)), bold: true), new Point(x + 1, y + 1));
        dc.DrawText(t, new Point(x, y));
    }

    // Glyphs, drawn in a 25x25 square.
    static readonly Brush Ink = Brushes.White;

    static void DownArrow(DrawingContext dc) => dc.DrawGeometry(Ink, null, Geometry.Parse("M 9,3 h 7 v 9 h 6 l -9.5,10 l -9.5,-10 h 6 z"));

    static void Window(DrawingContext dc)
    {
        dc.DrawRectangle(Ink, null, new Rect(3, 5, 19, 15));
        dc.DrawRectangle(new SolidColorBrush(Render.Rgb("#0A3FB0")), null, new Rect(4, 6, 17, 3));
        dc.DrawRectangle(new SolidColorBrush(Render.Rgb("#9DC3FF")), null, new Rect(4, 10, 17, 9));
    }

    static void Blob(DrawingContext dc)
    {
        dc.DrawGeometry(Ink, null, Geometry.Parse("M 4,12 C 2,4 10,1 14,4 C 19,1 25,6 22,12 C 25,18 19,24 13,21 C 7,24 1,19 4,12 z"));
        dc.DrawEllipse(new SolidColorBrush(Render.Rgb("#2A2A2A")), null, new Point(12.5, 12.5), 5, 4);
        dc.DrawEllipse(new SolidColorBrush(Render.Rgb("#7FD0FF")), null, new Point(12.5, 12.5), 3.5, 2.5);
    }

    static void Bars(DrawingContext dc)
    {
        int[] heights = { 8, 15, 11, 19, 6 };
        for (int i = 0; i < heights.Length; i++)
            dc.DrawRectangle(Render.Vertical((0, "#FFF27A"), (1, "#7CFF6B")), null, new Rect(3 + i * 4, 22 - heights[i], 3, heights[i]));
    }

    static void Notes(DrawingContext dc) => dc.DrawText(Render.Text("♫", "Arial", 21, Ink, bold: true), new Point(3, -1));

    static void Padlock(DrawingContext dc)
    {
        dc.DrawGeometry(null, new Pen(new SolidColorBrush(Render.Rgb("#3A3F48")), 2.5), Geometry.Parse("M 8,11 V 7 A 4.5,4.5 0 0 1 17,7"));
        dc.DrawRoundedRectangle(new SolidColorBrush(Render.Rgb("#F2C230")), new Pen(new SolidColorBrush(Render.Rgb("#3A3F48")), 1), new Rect(5, 11, 15, 11), 2, 2);
    }

    static void Speaker(DrawingContext dc)
    {
        dc.DrawGeometry(Ink, null, Geometry.Parse("M 4,9 h 4 l 6,-5 v 17 l -6,-5 h -4 z"));
        var pen = new Pen(Ink, 1.6);
        dc.DrawGeometry(null, pen, Geometry.Parse("M 17,9 A 5,5 0 0 1 17,16"));
        dc.DrawGeometry(null, pen, Geometry.Parse("M 19.5,6 A 9,9 0 0 1 19.5,19"));
    }

    static void Monitor(DrawingContext dc)
    {
        dc.DrawRectangle(new SolidColorBrush(Render.Rgb("#D8D8D0")), null, new Rect(2, 3, 21, 16));
        dc.DrawRectangle(Render.Vertical((0, "#3C7BE0"), (1, "#5DB040")), null, new Rect(4, 5, 17, 12));
        dc.DrawRectangle(new SolidColorBrush(Render.Rgb("#D8D8D0")), null, new Rect(9, 19, 7, 2));
        dc.DrawRectangle(new SolidColorBrush(Render.Rgb("#D8D8D0")), null, new Rect(6, 21, 13, 2));
    }

    static Action<DrawingContext> Label(string text, string color) => dc =>
    {
        var t = Render.Text(text, "Tahoma", text.Length > 2 ? 9 : 13, new SolidColorBrush(Render.Rgb(color)), bold: true);
        dc.DrawText(t, new Point(Math.Round((25 - t.WidthIncludingTrailingWhitespace) / 2), Math.Round((25 - t.Height) / 2)));
    };

    // ---- album art for the demo playlist --------------------------------------------------

    public static BitmapSource Cover(string title, string subtitle, double hue)
    {
        const int S = 300;
        return Render.Drawing(S, S, dc =>
        {
            var bg = new LinearGradientBrush(Hsv(hue, 0.55, 0.92), Hsv(hue + 35, 0.85, 0.30), new Point(0, 0), new Point(1, 1));
            dc.DrawRectangle(bg, null, new Rect(0, 0, S, S));
            for (int i = 1; i <= 9; i++)
                dc.DrawEllipse(null, new Pen(new SolidColorBrush(Color.FromArgb((byte)(70 - i * 6), 255, 255, 255)), 2 + i % 3), new Point(S * 0.78, S * 0.28), i * 26, i * 26);
            dc.DrawEllipse(new RadialGradientBrush(Color.FromArgb(0x90, 255, 255, 255), Color.FromArgb(0, 255, 255, 255)), null, new Point(S * 0.78, S * 0.28), 60, 60);
            dc.DrawRectangle(new SolidColorBrush(Color.FromArgb(0x60, 0, 0, 0)), null, new Rect(0, S - 92, S, 92));
            dc.DrawText(Render.Text(title, "Georgia", 30, Brushes.White, italic: true), new Point(18, S - 84));
            dc.DrawText(Render.Text(subtitle, "Tahoma", 14, new SolidColorBrush(Color.FromArgb(0xD0, 255, 255, 255))), new Point(20, S - 38));
        });
    }

    static Color Hsv(double h, double s, double v)
    {
        h = (h % 360 + 360) % 360 / 60;
        double c = v * s, x = c * (1 - Math.Abs(h % 2 - 1)), m = v - c;
        var (r, g, b) = (int)h switch { 0 => (c, x, 0d), 1 => (x, c, 0d), 2 => (0d, c, x), 3 => (0d, x, c), 4 => (x, 0d, c), _ => (c, 0d, x) };
        return Color.FromRgb((byte)((r + m) * 255), (byte)((g + m) * 255), (byte)((b + m) * 255));
    }
}
