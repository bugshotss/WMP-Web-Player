using System.Globalization;
using System.IO;
using System.Windows;
using System.Windows.Media;
using System.Windows.Media.Imaging;

namespace ReadmeArt;

static class Program
{
    const string Usage = """
        ReadmeArt backdrop <width> <height> <seed> <out.png>   rolling hills under a cloudy sky
        ReadmeArt cover <out.png> <title> <subtitle> <hue>      album art for the demo playlist
        ReadmeArt art <outDir> <app.ico>                        banner, 88x31 buttons, download button
        ReadmeArt compose <spec.txt> <out.png|out.gif>          a scene; with "frames" lines, an animated GIF
        ReadmeArt sheet <out.png> <columns> <image>...          a labelled contact sheet, for picking shots
        ReadmeArt trim <in.png> <out.png>                       crops a picture to its visible (non-transparent) part
        ReadmeArt pack <w> <h> <gap> <seed> <app.ico> <spec.txt> <image>...
                                                                piles pictures onto a desktop by their outlines
        """;

    [STAThread]
    static int Main(string[] args)
    {
        try
        {
            switch (args.FirstOrDefault())
            {
                case "backdrop": Render.Png(Scenery.Backdrop(Int(args[1]), Int(args[2]), Int(args[3])), args[4]); break;
                case "cover": Render.Png(Art.Cover(args[2], args[3], double.Parse(args[4], CultureInfo.InvariantCulture)), args[1]); break;
                case "art": Art.All(args[1], args[2]); break;
                case "compose": Compose.Run(args[1], args[2]); break;
                case "sheet": Render.Png(Sheet(Int(args[2]), args[3..]), args[1]); break;
                case "trim": Render.Png(Trim(Render.Load(args[1])), args[2]); break;
                case "pack": Pack.Run(Int(args[1]), Int(args[2]), Int(args[3]), Int(args[4]), args[5], args[6], args[7..]); break;
                default: Console.WriteLine(Usage); return 1;
            }
            return 0;
        }
        catch (Exception ex)
        {
            Console.Error.WriteLine(ex);
            return 2;
        }
    }

    static int Int(string s) => int.Parse(s, CultureInfo.InvariantCulture);

    static BitmapSource Trim(BitmapSource image)
    {
        var src = new FormatConvertedBitmap(image, PixelFormats.Bgra32, null, 0);
        var px = Render.Pixels(src);
        int w = src.PixelWidth, h = src.PixelHeight, x0 = w, y0 = h, x1 = -1, y1 = -1;
        for (int y = 0; y < h; y++)
            for (int x = 0; x < w; x++)
                if ((px[y * w + x] >>> 24) > 8) { x0 = Math.Min(x0, x); x1 = Math.Max(x1, x); y0 = Math.Min(y0, y); y1 = Math.Max(y1, y); }
        return x1 < 0 ? image : new CroppedBitmap(src, new Int32Rect(x0, y0, x1 - x0 + 1, y1 - y0 + 1));
    }

    static BitmapSource Sheet(int columns, string[] files)
    {
        const int cell = 260, label = 18;
        int rows = (files.Length + columns - 1) / columns;
        return Render.Drawing(columns * cell, rows * (cell + label), dc =>
        {
            dc.DrawRectangle(new SolidColorBrush(Color.FromRgb(0x30, 0x30, 0x38)), null, new Rect(0, 0, columns * cell, rows * (cell + label)));
            for (int i = 0; i < files.Length; i++)
            {
                var img = Render.Load(files[i]);
                double s = Math.Min(1, (cell - 8.0) / Math.Max(img.PixelWidth, img.PixelHeight));
                double w = img.PixelWidth * s, h = img.PixelHeight * s;
                double x = i % columns * cell, y = i / columns * (cell + label);
                dc.DrawImage(img, new Rect(x + (cell - w) / 2, y + (cell - h) / 2, w, h));
                dc.DrawText(Render.Text(Path.GetFileNameWithoutExtension(files[i]) + $"  {img.PixelWidth}x{img.PixelHeight}", "Tahoma", 11, Brushes.White), new Point(x + 4, y + cell));
            }
        });
    }
}

static class Render
{
    public static BitmapSource Element(FrameworkElement e, int width, int height)
    {
        e.Measure(new Size(width, height));
        e.Arrange(new Rect(0, 0, width, height));
        e.UpdateLayout();
        var rtb = new RenderTargetBitmap(width, height, 96, 96, PixelFormats.Pbgra32);
        rtb.Render(e);
        rtb.Freeze();
        return rtb;
    }

    /// <param name="aliased">Hard-edged text, as small pixel fonts were.</param>
    public static BitmapSource Drawing(int width, int height, Action<DrawingContext> draw, bool aliased = false)
    {
        var dv = new DrawingVisual();
        TextOptions.SetTextRenderingMode(dv, aliased ? TextRenderingMode.Aliased : TextRenderingMode.Grayscale);
        using (var dc = dv.RenderOpen()) draw(dc);
        var rtb = new RenderTargetBitmap(width, height, 96, 96, PixelFormats.Pbgra32);
        rtb.Render(dv);
        rtb.Freeze();
        return rtb;
    }

    public static void Png(BitmapSource image, string path)
    {
        Directory.CreateDirectory(Path.GetDirectoryName(Path.GetFullPath(path))!);
        var enc = new PngBitmapEncoder();
        enc.Frames.Add(BitmapFrame.Create(image));
        using var fs = File.Create(path);
        enc.Save(fs);
    }

    public static BitmapSource Load(string path)
    {
        var img = new BitmapImage();
        img.BeginInit();
        img.CacheOption = BitmapCacheOption.OnLoad;
        img.UriSource = new Uri(Path.GetFullPath(path));
        img.EndInit();
        img.Freeze();
        return img;
    }

    /// <summary>The largest picture in an .ico file.</summary>
    public static BitmapSource Icon(string path)
    {
        using var fs = File.OpenRead(path);
        var dec = BitmapDecoder.Create(fs, BitmapCreateOptions.PreservePixelFormat, BitmapCacheOption.OnLoad);
        var best = dec.Frames.OrderByDescending(f => f.PixelWidth).First();
        best.Freeze();
        return best;
    }

    public static int[] Pixels(BitmapSource image)
    {
        var px = new int[image.PixelWidth * image.PixelHeight];
        image.CopyPixels(px, image.PixelWidth * 4, 0);
        return px;
    }

    /// <summary>Pixel size of a picture drawn one image pixel per screen pixel, whatever DPI it was saved at.</summary>
    public static Rect At(BitmapSource image, double x, double y) => new(x, y, image.PixelWidth, image.PixelHeight);

    public static FormattedText Text(string text, string font, double size, Brush brush, bool bold = false, bool italic = false) =>
        new(text, CultureInfo.InvariantCulture, FlowDirection.LeftToRight,
            new Typeface(new FontFamily(font), italic ? FontStyles.Italic : FontStyles.Normal, bold ? FontWeights.Bold : FontWeights.Normal, FontStretches.Normal),
            size, brush, null, TextFormattingMode.Display, 1.0);

    public static Color Rgb(string hex) => (Color)ColorConverter.ConvertFromString(hex);

    public static LinearGradientBrush Vertical(params (double Offset, string Color)[] stops)
    {
        var b = new LinearGradientBrush { StartPoint = new Point(0, 0), EndPoint = new Point(0, 1) };
        foreach (var (o, c) in stops) b.GradientStops.Add(new GradientStop(Rgb(c), o));
        b.Freeze();
        return b;
    }
}
