using System.Globalization;
using System.IO;
using System.Text.RegularExpressions;
using System.Windows;
using System.Windows.Media;
using System.Windows.Media.Imaging;

namespace ReadmeArt;

/// <summary>
/// Lays out a scene from a spec, one instruction per line, drawn in order ('#' starts a comment):
/// <code>
///   size W H                canvas size
///   backdrop SEED           the hills and sky, filling the canvas
///   image PATH X Y          a picture, one image pixel per pixel
///   frames DIR X Y          a recording (DIR\frame0000.png...): makes the output an animated GIF
///   taskbar ICON            a taskbar along the bottom
///   delay CS                hundredths of a second per GIF frame (default 6)
///   dither LEVELS           ordered dithering strength for the GIF palettes (default 0: none)
///   skip N | take N         which recorded frames to use
/// </code>
/// Paths with spaces go in double quotes.
/// </summary>
static partial class Compose
{
    public static void Run(string specPath, string outPath)
    {
        int width = 640, height = 480, delay = 6, skip = 0, take = int.MaxValue;
        double dither = 0;
        var layers = new List<Func<int, (BitmapSource Image, Rect At)?>>();
        var recordings = new List<List<BitmapSource>>();
        Action<DrawingContext>? taskbar = null;
        foreach (var raw in File.ReadAllLines(specPath))
        {
            var line = raw.Trim();
            if (line.Length == 0 || line.StartsWith('#')) continue;
            var t = Token().Matches(line).Select(m => m.Groups[1].Success ? m.Groups[1].Value : m.Value).ToArray();
            double N(int i) => double.Parse(t[i], CultureInfo.InvariantCulture);
            switch (t[0])
            {
                case "size": width = (int)N(1); height = (int)N(2); break;
                case "delay": delay = (int)N(1); break;
                case "dither": dither = N(1); break;
                case "skip": skip = (int)N(1); break;
                case "take": take = (int)N(1); break;
                case "backdrop":
                {
                    var sky = Scenery.Backdrop(width, height, (int)N(1));
                    layers.Add(_ => (sky, new Rect(0, 0, width, height)));
                    break;
                }
                case "image":
                {
                    var img = Render.Load(t[1]);
                    var at = Render.At(img, N(2), N(3));
                    layers.Add(_ => (img, at));
                    break;
                }
                case "frames":
                {
                    var frames = Directory.GetFiles(t[1], "frame*.png").Order(StringComparer.Ordinal).Select(Render.Load).ToList();
                    if (frames.Count == 0) throw new InvalidOperationException("No frames in " + t[1]);
                    recordings.Add(frames);
                    double x = N(2), y = N(3);
                    layers.Add(i => { var f = frames[(i + skip) % frames.Count]; return (f, Render.At(f, x, y)); });
                    break;
                }
                case "taskbar":
                {
                    var icon = Render.Icon(t[1]);
                    taskbar = dc => Scenery.Taskbar(dc, height - Scenery.TaskbarHeight, width, icon);
                    break;
                }
                default: throw new InvalidOperationException("Unknown instruction: " + line);
            }
        }

        BitmapSource Frame(int i) => Render.Drawing(width, height, dc =>
        {
            foreach (var layer in layers)
                if (layer(i) is { } l) dc.DrawImage(l.Image, l.At);
            taskbar?.Invoke(dc);
        });

        if (recordings.Count == 0)
        {
            Render.Png(Frame(0), outPath);
            return;
        }
        int count = Math.Min(take, recordings.Max(r => r.Count) - skip);
        using var gif = new GifWriter(outPath, width, height, dither);
        for (int i = 0; i < count; i++) gif.Add(Render.Pixels(Frame(i)), delay);
        Console.WriteLine($"{outPath}: {count} frames");
    }

    [GeneratedRegex("\"([^\"]*)\"|\\S+")]
    private static partial Regex Token();
}
