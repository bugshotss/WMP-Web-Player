using System.IO;
using System.Text;
using System.Windows.Media.Imaging;
using WebSkinPlayer.Skins;

namespace WebSkinPlayer;

/// <summary>
/// Headless compatibility pass: "WebSkinPlayer.exe --test-skins &lt;folder&gt; &lt;outFolder&gt;" loads every
/// skin offscreen, runs its scripts, saves a PNG of the main view and writes report.txt.
/// </summary>
public static class SkinTestRunner
{
    public static void Run(string source, string outDir)
    {
        Directory.CreateDirectory(outDir);
        var files = Directory.Exists(source)
            ? Directory.EnumerateFiles(source, "*.wmz").Concat(Directory.EnumerateFiles(source, "*.wms", SearchOption.AllDirectories)).OrderBy(f => f).ToList()
            : new List<string> { source };
        var report = new StringBuilder();
        int ok = 0, withErrors = 0, failed = 0;
        foreach (var file in files)
        {
            var errors = new List<string>();
            string name = Path.GetFileNameWithoutExtension(file);
            try
            {
                // Script errors as when applied (player present); picture as the Skin Chooser shows it.
                var tree = new StringBuilder();
                SkinTheme.RenderPreview(file, errors, tree, chooserMode: false);
                File.WriteAllText(Path.Combine(outDir, name + ".tree.txt"), tree.ToString());
                var bmp = SkinTheme.RenderPreview(file, null);
                if (bmp != null)
                {
                    var enc = new PngBitmapEncoder();
                    enc.Frames.Add(BitmapFrame.Create(bmp));
                    using var fs = File.Create(Path.Combine(outDir, name + ".png"));
                    enc.Save(fs);
                }
                if (errors.Count == 0) { ok++; report.AppendLine($"OK      {name}  ({bmp?.PixelWidth}x{bmp?.PixelHeight})"); }
                else
                {
                    withErrors++;
                    report.AppendLine($"ERRORS  {name}  ({errors.Count})");
                    foreach (var e in errors.Distinct().Take(8)) report.AppendLine("        " + e);
                }
            }
            catch (Exception ex)
            {
                failed++;
                report.AppendLine($"FAILED  {name}: {ex.GetType().Name}: {ex.Message}");
                Log.Write($"Test {name} failed: {ex}");
            }
        }
        report.Insert(0, $"{files.Count} skins: {ok} clean, {withErrors} with script errors, {failed} failed to load{Environment.NewLine}{Environment.NewLine}");
        File.WriteAllText(Path.Combine(outDir, "report.txt"), report.ToString());
    }
}
