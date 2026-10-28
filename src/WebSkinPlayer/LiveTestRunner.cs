using System.IO;
using System.Text;
using System.Windows;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using WebSkinPlayer.Skins;
using WebSkinPlayer.Skins.Elements;

namespace WebSkinPlayer;

/// <summary>
/// "--test-live &lt;folder|file&gt; &lt;outDir&gt; [seconds]": opens each skin for real but hidden and
/// sandboxed (scripts, timers, GIF animations, player events all run; nothing reaches the real
/// player), waits, then measures how much of every visualizer area is actually visible.
/// </summary>
public static class LiveTestRunner
{
    public static async Task RunAsync(string source, string outDir, double seconds)
    {
        Sandbox.Enabled = true;
        Directory.CreateDirectory(outDir);
        var files = Directory.Exists(source)
            ? Directory.EnumerateFiles(source, "*.wmz").OrderBy(f => f).ToList()
            : new List<string> { source };
        var report = new StringBuilder();
        var counts = new Dictionary<string, int>();
        foreach (var file in files)
        {
            string name = Path.GetFileNameWithoutExtension(file);
            SkinTheme? theme = null;
            var lines = new List<string>();
            string status = "NO-VIS";
            try
            {
                theme = SkinTheme.Load(file);
                theme.OpenMainView();
                // Hidden windows never paint, so draw periodically like the real window would
                // (this is what starts and advances GIF animations).
                for (double t = 0; t < seconds; t += 0.5)
                {
                    foreach (var v in theme.OpenViews.ToList()) Render(v);
                    await Task.Delay(500);
                }
                foreach (var view in theme.OpenViews.ToList())
                {
                    var result = Analyze(view, lines);
                    if (Rank(result) > Rank(status)) status = result;
                }
                // The largest open view is the player; the active one can be a 0x0 helper view.
                var main = theme.OpenViews.OrderByDescending(v => (long)v.Width * v.Height).FirstOrDefault();
                if (main != null) Save(Render(main), Path.Combine(outDir, name + ".png"));
            }
            catch (Exception ex)
            {
                status = "FAILED";
                lines.Add($"    {ex.GetType().Name}: {ex.Message}");
            }
            finally
            {
                try { theme?.Close(); } catch { }
            }
            counts[status] = counts.GetValueOrDefault(status) + 1;
            report.AppendLine($"{status,-8} {name}");
            foreach (var l in lines) report.AppendLine(l);
            await Task.Delay(50);
        }
        report.Insert(0, $"{files.Count} skins after {seconds}s live: " + string.Join(", ", counts.OrderBy(k => k.Key).Select(k => $"{k.Value} {k.Key}")) +
            Environment.NewLine + "OK = visualizer visible; PARTLY/COVERED = drawn under other elements; HIDDEN/FADED = hidden or faded out by the skin until a button or mode shows it" +
            Environment.NewLine + Environment.NewLine);
        File.WriteAllText(Path.Combine(outDir, "live-report.txt"), report.ToString());
    }

    static int Rank(string s) => s switch { "OK" => 5, "PARTLY" => 4, "COVERED" => 3, "HIDDEN" or "FADED" => 2, "FAILED" => 1, _ => 0 };

    static string Analyze(SkinView view, List<string> lines)
    {
        var fx = view.AllElements.OfType<EffectsElement>().Where(e => e is not VideoElement v || v.ShowsVisualization).ToList();
        if (fx.Count == 0) return "NO-VIS";
        EffectsElement.MarkerMode = true;
        int[] px;
        int w = Math.Max(1, view.Width), h = Math.Max(1, view.Height);
        try { px = Pixels(Render(view), out w, out h); }
        finally { EffectsElement.MarkerMode = false; }
        int marker = (EffectsElement.MarkerColor.R << 16) | (EffectsElement.MarkerColor.G << 8) | EffectsElement.MarkerColor.B;
        string best = "NO-VIS";
        foreach (var e in fx)
        {
            var b = e.AbsoluteBounds;
            b.Intersect(new Rect(0, 0, w, h));
            string state;
            double share = 0;
            if (!e.IsEffectivelyVisible) state = "HIDDEN";
            else if (b.IsEmpty || b.Width < 1 || b.Height < 1) state = "HIDDEN";
            else
            {
                int hit = 0, total = 0;
                for (int y = (int)b.Top; y < (int)b.Bottom; y++)
                    for (int x = (int)b.Left; x < (int)b.Right; x++)
                    {
                        total++;
                        int c = px[y * w + x];
                        if ((c >>> 24) == 255 && (c & 0xFFFFFF) == marker) hit++;
                    }
                share = total > 0 ? hit / (double)total : 0;
                double alpha = 1;
                for (SkinElement? p = e; p != null; p = p.Parent) alpha *= p.Opacity;
                // A visualizer the skin's own script faded out is shown on demand, like a hidden one.
                state = share >= 0.6 ? "OK" : alpha < 0.1 ? "FADED" : share >= 0.1 ? "PARTLY" : "COVERED";
            }
            lines.Add($"    view {view.Id ?? "(main)"}: {e.Tag}{(e.Id != null ? "#" + e.Id : "")} at ({b.X:0},{b.Y:0} {b.Width:0}x{b.Height:0}) {state}{(state is "OK" or "PARTLY" or "COVERED" or "FADED" ? $" {share:P0} visible" : "")}");
            if (state is "PARTLY" or "COVERED" or "FADED") lines.Add("      " + Diagnose(view, e, b));
            if (state == "HIDDEN") lines.Add("      " + WhyHidden(view, e));
            if (Rank(state) > Rank(best)) best = state;
        }
        return best;
    }

    /// <summary>Which element hides the visualizer, and whether the skin's own code ever shows it again.</summary>
    static string WhyHidden(SkinView view, SkinElement fx)
    {
        SkinElement? gate = null;
        for (SkinElement? p = fx; p != null && p is not SkinView; p = p.Parent) if (!p.Visible) gate = p;
        if (gate == null) return "zero size";
        string name = gate.Id ?? "";
        if (name.Length == 0) return $"hidden by an unnamed {gate.Tag} (nothing can show it by name)";
        var pkg = view.Theme.Package;
        var code = string.Join("\n", pkg.Files.Where(f => f.EndsWith(".js", StringComparison.OrdinalIgnoreCase) || f.EndsWith(".wms", StringComparison.OrdinalIgnoreCase))
            .Select(f => { try { return pkg.ReadText(f); } catch { return ""; } }));
        // e.g. visMask.visible = true, visMask.visible=!visMask.visible, visible="wmpprop:...", visMask.show()
        bool shown = System.Text.RegularExpressions.Regex.IsMatch(code,
            $@"\b{System.Text.RegularExpressions.Regex.Escape(name)}\s*\.\s*(visible\s*=(?!=)|show\s*\()", System.Text.RegularExpressions.RegexOptions.IgnoreCase);
        bool bound = (gate.Node.Attr("visible") ?? "").Contains(':');
        return $"hidden by {gate.Tag}#{name}; " + (shown ? "the skin's script shows it on demand" : bound ? "its visibility is bound to player state" : "NOTHING in the skin shows it again");
    }

    /// <summary>Why a visualizer isn't showing: its own fade, and what is drawn over its centre.</summary>
    static string Diagnose(SkinView view, SkinElement fx, Rect b)
    {
        double alpha = 1;
        var faded = new List<string>();
        for (SkinElement? p = fx; p != null; p = p.Parent)
        {
            alpha *= p.Opacity;
            if (p.Opacity < 0.99) faded.Add($"{p.Tag}{(p.Id != null ? "#" + p.Id : "")}={p.Opacity * 255:0}");
        }
        var order = new List<SkinElement>();
        void Walk(SkinElement e) { order.Add(e); foreach (var c in e.ChildrenInDrawOrder) Walk(c); }
        Walk(view);
        int at = order.IndexOf(fx);
        var centre = new Point(b.X + b.Width / 2, b.Y + b.Height / 2);
        var over = order.Skip(at + 1)
            .Where(e => e.IsEffectivelyVisible && e.AbsoluteBounds.Contains(centre) && !IsAncestor(fx, e) && !IsAncestor(e, fx))
            .Reverse().Take(3)
            .Select(e => $"{e.Tag}{(e.Id != null ? "#" + e.Id : "")} z={e.ZIndex} {Describe(e)}");
        return $"opacity {alpha:0.##}{(faded.Count > 0 ? " (" + string.Join(", ", faded) + ")" : "")}, player {App.Services.Player.Status}; drawn over it: " + (string.Join(" | ", over) is { Length: > 0 } s ? s : "nothing");
    }

    static bool IsAncestor(SkinElement a, SkinElement of) { for (var p = of.Parent; p != null; p = p.Parent) if (p == a) return true; return false; }

    static string Describe(SkinElement e)
    {
        var parts = new List<string>();
        foreach (var key in new[] { "backgroundimage", "image", "backgroundcolor", "transparencycolor", "clippingcolor" })
            if (e.GetString(key) is { Length: > 0 } v && v != "none") parts.Add($"{key}={v}");
        return string.Join(" ", parts);
    }

    static BitmapSource Render(SkinView view)
    {
        int w = Math.Clamp(view.Width, 1, 4000), h = Math.Clamp(view.Height, 1, 4000);
        var dv = new DrawingVisual();
        using (var dc = dv.RenderOpen()) view.RenderTree(dc);
        var rtb = new RenderTargetBitmap(w, h, 96, 96, PixelFormats.Pbgra32);
        rtb.Render(dv);
        rtb.Freeze();
        return rtb;
    }

    static int[] Pixels(BitmapSource b, out int w, out int h)
    {
        w = b.PixelWidth; h = b.PixelHeight;
        var px = new int[w * h];
        b.CopyPixels(px, w * 4, 0);
        return px;
    }

    static void Save(BitmapSource b, string path)
    {
        var enc = new PngBitmapEncoder();
        enc.Frames.Add(BitmapFrame.Create(b));
        using var fs = File.Create(path);
        enc.Save(fs);
    }
}
