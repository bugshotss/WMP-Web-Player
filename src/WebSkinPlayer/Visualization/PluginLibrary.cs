using System.IO;
using System.Diagnostics;
using System.IO.Compression;
using System.Text.Json;
using System.Text.RegularExpressions;

namespace WebSkinPlayer.Visualization;

/// <summary>
/// <see cref="Output"/> says where the plug-in's picture shows up: "dc" (the DC handed to Render), "window" (DirectX
/// plug-ins draw onto the window WMP gave them) or "windowed" (IWMPEffects2::RenderWindowed). Found when it's added.
/// </summary>
public sealed record PluginEntry(string Id, string Title, string Clsid, string DllPath, IReadOnlyList<string> Presets, string Source, string Output);

/// <summary>
/// WMP visualization plug-ins added to the player (%LOCALAPPDATA%\WebSkinPlayer\Visualizations). They're
/// 32-bit DLLs from 2000-2005, so they run in WebSkinPlayer.VisHost32; nothing is registered with Windows.
/// </summary>
public static class PluginLibrary
{
    public static string Folder => Path.Combine(Log.Folder, "Visualizations");
    static string ListPath => Path.Combine(Folder, "plugins.json");

    /// <summary>The 32-bit helper, shipped in its own folder next to the player.</summary>
    public static string HostPath =>
        Environment.GetEnvironmentVariable("WEBSKINPLAYER_VISHOST") is { Length: > 0 } dev ? dev
        : Path.Combine(AppContext.BaseDirectory, "VisHost32", "WebSkinPlayer.VisHost32.exe");

    public static bool HostAvailable => File.Exists(HostPath);

    sealed class Stored
    {
        public string Id { get; set; } = "";
        public string Title { get; set; } = "";
        public string Clsid { get; set; } = "";
        public string Dll { get; set; } = "";
        public List<string> Presets { get; set; } = new();
        public string Source { get; set; } = "";
        public string Output { get; set; } = "dc";
    }

    static readonly JsonSerializerOptions Json = new() { WriteIndented = true };

    public static List<PluginEntry> Load()
    {
        try
        {
            // The skin tests stay deterministic: no helper processes behind every preview.
            if (Sandbox.Enabled || !File.Exists(ListPath) || !HostAvailable) return new();
            var stored = JsonSerializer.Deserialize<List<Stored>>(File.ReadAllText(ListPath), Json) ?? new();
            return stored.Select(s => new PluginEntry(s.Id, s.Title, s.Clsid, Path.Combine(Folder, s.Dll), s.Presets, s.Source, s.Output))
                         .Where(e => File.Exists(e.DllPath)).ToList();
        }
        catch (Exception ex) { Log.Write("Visualization list: " + ex.Message); return new(); }
    }

    /// <summary>
    /// Adds the plug-ins in <paramref name="source"/> (a .dll, a .zip, an .msi, or a folder searched recursively).
    /// Returns a line per visualization found, saying whether it was added.
    /// </summary>
    public static List<string> Import(string source, string label)
    {
        var report = new List<string>();
        if (!HostAvailable) { report.Add("The 32-bit helper (VisHost32) is missing."); return report; }
        string? temp = null;
        try
        {
            string root = source;
            if (source.EndsWith(".zip", StringComparison.OrdinalIgnoreCase) || source.EndsWith(".msi", StringComparison.OrdinalIgnoreCase))
            {
                temp = Path.Combine(Path.GetTempPath(), "WebSkinPlayer-import-" + Guid.NewGuid().ToString("N"));
                if (source.EndsWith(".zip", StringComparison.OrdinalIgnoreCase)) ZipFile.ExtractToDirectory(source, temp);
                else if (!UnpackMsi(source, temp)) { report.Add($"{Path.GetFileName(source)}: couldn't unpack the installer"); return report; }
                root = temp;
            }
            var dlls = File.Exists(root) ? new List<string> { root }
                : Directory.EnumerateFiles(root, "*.*", SearchOption.AllDirectories).Where(IsPluginDll).ToList();
            var list = LoadStored();
            var packs = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
            foreach (var dll in dlls)
            {
                // One copy per folder: plug-ins shipped together (sharing DirectX helper DLLs) stay together.
                string from = Path.GetDirectoryName(dll)!;
                if (!packs.TryGetValue(from, out var pack))
                {
                    bool shared = dlls.Count(d => Path.GetDirectoryName(d)!.Equals(from, StringComparison.OrdinalIgnoreCase)) > 1;
                    string name = !shared ? Path.GetFileNameWithoutExtension(dll)
                        : Path.GetFileNameWithoutExtension(source) + (from.Equals(root, StringComparison.OrdinalIgnoreCase) ? "" : "_" + Path.GetFileName(from));
                    pack = packs[from] = Path.Combine(Folder, Safe(name));
                    CopyPack(from, pack, File.Exists(root) ? dll : null);
                }
                string copied = Path.Combine(pack, Path.GetFileName(dll));
                var (code, json) = RunHost($"probe \"{copied}\"", 30_000);
                if (json == null) { report.Add($"{Path.GetFileName(dll)}: no visualization found ({code})"); continue; }
                using var doc = JsonDocument.Parse(json);
                foreach (var fx in doc.RootElement.GetProperty("effects").EnumerateArray())
                {
                    string clsid = fx.GetProperty("clsid").GetString()!;
                    string title = fx.GetProperty("title").GetString() is { Length: > 0 } t ? t.Trim() : Path.GetFileNameWithoutExtension(dll);
                    var presets = fx.GetProperty("presets").EnumerateArray().Select(p => p.GetString() ?? "").ToList();
                    if (FindOutput(copied, clsid) is not { } output) { report.Add($"{title}: draws nothing here, not added"); continue; }
                    list.RemoveAll(s => s.Clsid.Equals(clsid, StringComparison.OrdinalIgnoreCase));
                    title = UniqueTitle(title, dll, list);
                    list.Add(new Stored
                    {
                        Id = Regex.Replace(title, "[^A-Za-z0-9]", ""), Title = title, Clsid = clsid,
                        Dll = Path.GetRelativePath(Folder, copied), Presets = presets.Count > 0 ? presets : new() { title }, Source = label,
                        Output = output,
                    });
                    report.Add($"{title}: added ({presets.Count} presets)");
                }
            }
            Directory.CreateDirectory(Folder);
            File.WriteAllText(ListPath, JsonSerializer.Serialize(list, Json));
        }
        finally
        {
            if (temp != null) try { Directory.Delete(temp, true); } catch { }
        }
        return report;
    }

    static List<Stored> LoadStored()
    {
        try { return File.Exists(ListPath) ? JsonSerializer.Deserialize<List<Stored>>(File.ReadAllText(ListPath), Json) ?? new() : new(); }
        catch { return new(); }
    }

    /// <summary>A 32-bit COM DLL (exports DllGetClassObject), not one of the old installers' helper DLLs.</summary>
    static bool IsPluginDll(string path)
    {
        string name = Path.GetFileName(path);
        if (!Regex.IsMatch(name, @"\.(dll|ax)$", RegexOptions.IgnoreCase) || Regex.IsMatch(name, @"^(advpack|w95inf16|w95inf32)\.dll$", RegexOptions.IgnoreCase)) return false;
        try
        {
            var bytes = File.ReadAllBytes(path);
            if (bytes.Length < 512 || bytes[0] != 'M' || bytes[1] != 'Z') return false;
            int pe = BitConverter.ToInt32(bytes, 0x3C);
            return pe > 0 && pe + 6 < bytes.Length && BitConverter.ToUInt16(bytes, pe + 4) == 0x14C
                && System.Text.Encoding.ASCII.GetString(bytes).Contains("DllGetClassObject");
        }
        catch { return false; }
    }

    /// <summary>Copies the plug-in with the files beside it (pictures and data some of them load), minus installer leftovers.</summary>
    static void CopyPack(string from, string to, string? onlyFile)
    {
        Directory.CreateDirectory(to);
        var files = onlyFile != null ? new[] { onlyFile } : Directory.GetFiles(from, "*", SearchOption.AllDirectories);
        foreach (var f in files)
        {
            string name = Path.GetFileName(f);
            if (Regex.IsMatch(name, @"^(advpack|w95inf16|w95inf32)\.dll$|\.inf$", RegexOptions.IgnoreCase)) continue;
            string dest = Path.Combine(to, Path.GetRelativePath(onlyFile != null ? Path.GetDirectoryName(f)! : from, f));
            Directory.CreateDirectory(Path.GetDirectoryName(dest)!);
            File.Copy(f, dest, overwrite: true);
        }
    }

    /// <summary>
    /// Renders a couple of seconds of test music the way WMP 9 does and returns where the picture appeared (see
    /// <see cref="PluginEntry.Output"/>), or null if nothing did. Plug-ins that draw into the DC keep doing that;
    /// WMP 9-style ones that don't are tried with RenderWindowed (WMP's full-mode path, which some draw properly only
    /// through: Ribbons gives a grey relief otherwise) before settling for what they put on the window.
    /// </summary>
    static string? FindOutput(string dll, string clsid)
    {
        if (Snapshot(dll, clsid, "attach") is not { } attach) return null;
        if (attach.Lit > 0.001) return "dc";
        if (attach.Effects2 && Snapshot(dll, clsid, "windowed") is { WindowLit: > 0.001 }) return "windowed";
        return attach.WindowLit > 0.001 ? "window" : null;
    }

    static (double Lit, double WindowLit, bool Effects2)? Snapshot(string dll, string clsid, string mode)
    {
        string bmp = Path.Combine(Path.GetTempPath(), $"wsp-viz-{Guid.NewGuid():N}.bmp");
        try
        {
            var (_, json) = RunHost($"snapshot \"{dll}\" {clsid} \"{bmp}\" 0 {mode}", 30_000);
            if (json == null) return null;
            using var doc = JsonDocument.Parse(json);
            var r = doc.RootElement;
            return (r.GetProperty("lit").GetDouble(), r.GetProperty("windowLit").GetDouble(), r.GetProperty("effects2").GetBoolean());
        }
        catch { return null; }
        finally { try { File.Delete(bmp); } catch { } }
    }

    /// <summary>An administrative install: Windows Installer copies the files out without installing anything.</summary>
    static bool UnpackMsi(string msi, string folder)
    {
        Directory.CreateDirectory(folder);
        var psi = new ProcessStartInfo("msiexec.exe") { UseShellExecute = false, CreateNoWindow = true };
        foreach (var a in new[] { "/a", Path.GetFullPath(msi), "/qn", "TARGETDIR=" + folder }) psi.ArgumentList.Add(a);
        using var p = Process.Start(psi)!;
        return p.WaitForExit(120_000) && p.ExitCode == 0;
    }

    /// <summary>Plug-ins named like a built-in visualization (WMP 8's own Bars and Waves, Battery) get a suffix.</summary>
    static string UniqueTitle(string title, string dll, List<Stored> list)
    {
        bool Taken(string t) => VisualizationCatalog.BuiltInTitles.Contains(t, StringComparer.OrdinalIgnoreCase)
                                || list.Any(s => s.Title.Equals(t, StringComparison.OrdinalIgnoreCase));
        if (!Taken(title)) return title;
        string suffix = Path.GetFileName(dll).Equals("wmpui.dll", StringComparison.OrdinalIgnoreCase) ? "WMP 8" : Path.GetFileNameWithoutExtension(dll);
        string candidate = $"{title} ({suffix})";
        for (int n = 2; Taken(candidate); n++) candidate = $"{title} ({suffix} {n})";
        return candidate;
    }

    static (int Code, string? Json) RunHost(string args, int timeoutMs)
    {
        var psi = new ProcessStartInfo(HostPath, args) { UseShellExecute = false, RedirectStandardOutput = true, RedirectStandardError = true, CreateNoWindow = true };
        using var p = Process.Start(psi)!;
        var output = p.StandardOutput.ReadToEndAsync();
        if (!p.WaitForExit(timeoutMs)) { try { p.Kill(); } catch { } return (-1, null); }
        string text = output.Result.Trim();
        return (p.ExitCode, text.StartsWith('{') ? text : null);
    }

    static string Safe(string name) => Regex.Replace(name, @"[^\w.-]", "_");
}
