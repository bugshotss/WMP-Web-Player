using System.Globalization;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Media;
using System.Windows.Media.Effects;
using System.Windows.Media.Imaging;
using System.Windows.Shapes;

namespace ReadmeArt;

/// <summary>The era's desktop, drawn from scratch: green hills under a blue sky with fair-weather clouds, and a
/// blue taskbar.</summary>
static class Scenery
{
    public static BitmapSource Backdrop(int w, int h, int seed)
    {
        var rng = new Random(seed);
        var root = new Canvas { Width = w, Height = h, ClipToBounds = true };
        root.Children.Add(new Rectangle { Width = w, Height = h, Fill = Render.Vertical((0, "#1846B0"), (0.28, "#2C69D3"), (0.52, "#68A4EA"), (0.70, "#B6D6F6")) });

        var clouds = new Canvas { Width = w, Height = h, Effect = new BlurEffect { Radius = Math.Clamp(h * 0.016, 3, 16), RenderingBias = RenderingBias.Quality } };
        int count = Math.Max(4, w * h / 70000);
        for (int c = 0; c < count; c++) Cloud(clouds, rng, rng.NextDouble() * w, h * (0.05 + rng.NextDouble() * 0.48), h);
        for (int c = 0; c < Math.Max(2, w / 300); c++)
        {
            double ww = w * (0.15 + rng.NextDouble() * 0.25);
            var wisp = new Ellipse { Width = ww, Height = h * (0.008 + rng.NextDouble() * 0.012), Fill = Brushes.White, Opacity = 0.25 + rng.NextDouble() * 0.2,
                                     RenderTransform = new RotateTransform(rng.NextDouble() * 6 - 3) };
            Canvas.SetLeft(wisp, rng.NextDouble() * w - ww / 2);
            Canvas.SetTop(wisp, h * (0.04 + rng.NextDouble() * 0.25));
            clouds.Children.Add(wisp);
        }
        root.Children.Add(clouds);

        root.Children.Add(Hill(F($"M 0,{0.60 * h} C {0.16 * w},{0.53 * h} {0.34 * w},{0.55 * h} {0.52 * w},{0.63 * h} L {0.52 * w},{h} L 0,{h} Z"),
            Render.Vertical((0, "#6FAE57"), (1, "#3E7C2C"))));
        var main = F($"M 0,{0.71 * h} C {0.20 * w},{0.58 * h} {0.42 * w},{0.52 * h} {0.62 * w},{0.58 * h} C {0.78 * w},{0.63 * h} {0.90 * w},{0.67 * h} {w},{0.64 * h} L {w},{h} L 0,{h} Z");
        root.Children.Add(Hill(main, Render.Vertical((0, "#8AD14E"), (0.22, "#62AE31"), (0.65, "#3D881C"), (1, "#2A6911"))));
        var shine = new RadialGradientBrush(Color.FromArgb(0x90, 0xC9, 0xF0, 0x8C), Color.FromArgb(0, 0xC9, 0xF0, 0x8C))
        { Center = new Point(0.42, 0.12), GradientOrigin = new Point(0.42, 0.12), RadiusX = 0.45, RadiusY = 0.55 };
        root.Children.Add(Hill(main, shine));
        root.Children.Add(Hill(F($"M {0.38 * w},{h} C {0.55 * w},{0.81 * h} {0.78 * w},{0.75 * h} {w},{0.77 * h} L {w},{h} Z"),
            Render.Vertical((0, "#5BA62D"), (0.35, "#3F8A1D"), (1, "#22600D"))));
        return Render.Element(root, w, h);
    }

    /// <summary>A cumulus: puffs piled into a dome over a flat, slightly shaded base.</summary>
    static void Cloud(Canvas canvas, Random rng, double cx, double baseY, double h)
    {
        double size = h * (0.16 + rng.NextDouble() * 0.22) * (1.25 - baseY / h);
        int puffs = 9 + rng.Next(9);
        for (int i = 0; i < puffs; i++)
        {
            double dx = (rng.NextDouble() - 0.5) * size;
            double edge = Math.Abs(dx) / (size / 2);
            double r = size * (0.10 + rng.NextDouble() * 0.12) * (1 - 0.45 * edge);
            double dome = (1 - edge) * size * 0.16 * rng.NextDouble();
            bool shade = i < puffs / 4;
            var puff = new Ellipse
            {
                Width = 2 * r, Height = 1.6 * r,
                Fill = shade ? new SolidColorBrush(Color.FromRgb(0xD3, 0xE2, 0xF3)) : Brushes.White,
                Opacity = shade ? 0.45 : 0.72 + rng.NextDouble() * 0.25,
            };
            Canvas.SetLeft(puff, cx + dx - r);
            Canvas.SetTop(puff, baseY - (shade ? 0.9 : 1.5) * r - dome);
            canvas.Children.Add(puff);
        }
        var floor = new Ellipse { Width = size * 1.1, Height = size * 0.12, Fill = Brushes.White, Opacity = 0.8 };
        Canvas.SetLeft(floor, cx - size * 0.55);
        Canvas.SetTop(floor, baseY - size * 0.08);
        canvas.Children.Add(floor);
    }

    static Path Hill(Geometry g, Brush fill) => new() { Data = g, Fill = fill };

    static Geometry F(FormattableString path) => Geometry.Parse(path.ToString(CultureInfo.InvariantCulture));

    public const int TaskbarHeight = 30;

    /// <summary>A Luna-blue taskbar along <paramref name="y"/>: a green "play" button, the player's task button and
    /// a tray with a clock.</summary>
    public static void Taskbar(DrawingContext dc, double y, double w, BitmapSource icon)
    {
        dc.DrawRectangle(Render.Vertical((0, "#3A7BE8"), (0.06, "#5B9CF7"), (0.13, "#3F80EE"), (0.5, "#2864DD"), (0.9, "#2257D3"), (1, "#1A47B7")), null,
            new Rect(0, y, w, TaskbarHeight));

        var green = Render.Vertical((0, "#5EB84F"), (0.1, "#8BD47F"), (0.22, "#4EA93F"), (0.8, "#3B9330"), (1, "#2D7A24"));
        dc.PushClip(new RectangleGeometry(new Rect(0, y, w, TaskbarHeight)));
        dc.DrawRoundedRectangle(green, new Pen(new SolidColorBrush(Render.Rgb("#2B6F22")), 1), new Rect(-14, y, 112, TaskbarHeight), 12, 12);
        dc.Pop();
        dc.DrawImage(icon, new Rect(8, y + 5, 20, 20));
        var play = Render.Text("play", "Trebuchet MS", 19, Brushes.White, bold: true, italic: true);
        dc.DrawText(Render.Text("play", "Trebuchet MS", 19, new SolidColorBrush(Render.Rgb("#1C4F14")), bold: true, italic: true), new Point(34, y + 3));
        dc.DrawText(play, new Point(33, y + 2));

        var task = new Rect(110, y + 3, 168, TaskbarHeight - 5);
        dc.DrawRoundedRectangle(Render.Vertical((0, "#5A9CF6"), (0.15, "#3D86F2"), (1, "#1E5CD8")), new Pen(new SolidColorBrush(Render.Rgb("#1A4BB5")), 1), task, 3, 3);
        dc.DrawImage(icon, new Rect(task.X + 6, task.Y + 4, 16, 16));
        dc.DrawText(Render.Text("WebSkin Player", "Tahoma", 11, Brushes.White), new Point(task.X + 27, task.Y + 5));

        var tray = new Rect(w - 104, y, 104, TaskbarHeight);
        dc.DrawRectangle(Render.Vertical((0, "#1AA6F4"), (0.1, "#33B5F9"), (1, "#0F82DB")), null, tray);
        dc.DrawRectangle(new SolidColorBrush(Render.Rgb("#0B5BB8")), null, new Rect(tray.X, y, 1, TaskbarHeight));
        dc.DrawRectangle(new SolidColorBrush(Render.Rgb("#6CCBFB")), null, new Rect(tray.X + 1, y, 1, TaskbarHeight));
        dc.DrawImage(icon, new Rect(tray.X + 10, y + 7, 16, 16));
        dc.DrawText(Render.Text("10:26 PM", "Tahoma", 11, Brushes.White), new Point(tray.X + 38, y + 8));
    }
}
