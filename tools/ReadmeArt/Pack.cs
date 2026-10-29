using System.Globalization;
using System.IO;
using System.Windows.Media;
using System.Windows.Media.Imaging;

namespace ReadmeArt;

/// <summary>
/// Scatters skin shots over a desktop the way people piled them up for screenshots: each one, in order, goes to the
/// highest spot where its outline (not its bounding box) clears the ones already placed. Writes a compose spec.
/// </summary>
static class Pack
{
    /// <param name="gap">Space between neighbours; negative lets them overlap that much, as windows on a busy desktop.</param>
    public static void Run(int width, int height, int gap, int seed, string icon, string specPath, string[] images)
    {
        int margin = Math.Max(8, gap), spread = Math.Max(0, gap);
        var sky = new int[width];
        Array.Fill(sky, margin);
        var lines = new List<string> { $"size {width} {height}", $"backdrop {seed}" };
        int floor = height - Scenery.TaskbarHeight;
        foreach (var path in images)
        {
            var (top, bottom, w) = Outline(Render.Load(path));
            int bestX = -1, bestY = int.MaxValue;
            for (int x = margin; x + w <= width - margin; x += 2)
            {
                int y = 0;
                for (int c = 0; c < w; c++)
                    if (top[c] >= 0) y = Math.Max(y, sky[x + c] - top[c]);
                if (y < bestY) (bestX, bestY) = (x, y);
            }
            for (int c = 0; c < w; c++)
            {
                if (top[c] < 0) continue;
                for (int dx = -spread; dx <= spread; dx++)
                {
                    int i = bestX + c + dx;
                    if (i >= 0 && i < width) sky[i] = Math.Max(sky[i], bestY + bottom[c] + gap);
                }
            }
            int lowest = bestY + bottom.Max();
            Console.WriteLine($"{Path.GetFileNameWithoutExtension(path),-20} {bestX,5},{bestY,5}  bottom {lowest}{(lowest > floor ? "  (below the taskbar)" : "")}");
            lines.Add(string.Create(CultureInfo.InvariantCulture, $"image \"{path}\" {bestX} {bestY}"));
        }
        lines.Add($"taskbar \"{icon}\"");
        File.WriteAllLines(specPath, lines);
    }

    /// <summary>For each column: the first and last visible row (-1 where the column is empty).</summary>
    static (int[] Top, int[] Bottom, int Width) Outline(BitmapSource image)
    {
        var src = new FormatConvertedBitmap(image, PixelFormats.Bgra32, null, 0);
        var px = Render.Pixels(src);
        int w = src.PixelWidth, h = src.PixelHeight;
        var top = new int[w];
        var bottom = new int[w];
        for (int x = 0; x < w; x++)
        {
            top[x] = bottom[x] = -1;
            for (int y = 0; y < h; y++)
                if ((px[y * w + x] >>> 24) > 16)
                {
                    if (top[x] < 0) top[x] = y;
                    bottom[x] = y;
                }
        }
        return (top, bottom, w);
    }
}
