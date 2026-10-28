using System.IO;
using System.Diagnostics;
using System.IO.Compression;
using System.Text.Json;
using System.Text.RegularExpressions;

namespace WebSkinPlayer.Visualization;

public sealed record PluginEntry(string Id, string Title, string Clsid, string DllPath, IReadOnlyList<string> Presets, string Source);

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
    }

    static readonly JsonSerializerOptions Json = new() { WriteIndented = true };

    public static List<PluginEntry> Load()
    {
        try
        {
            // The skin tests stay deterministic: no helper processes behind every preview.
            if (Sandbox.Enabled || !File.Exists(ListPath) || !HostAvailable) return new();
            var stored = JsonSerializer.Deserialize<List<Stored>>(File.ReadAllText(ListPath), Json) ?? new();
            return stored.Select(s => new PluginEntry(s.Id, s.Title, s.Clsid, Path.Combine(Folder, s.Dll), s.Presets, s.Source))
                         .Where(e => File.Exists(e.DllPath)).ToList();
        }
        catch (Exception ex) { Log.Write("Visualization list: " + ex.Message); return new(); }
    }

    /// <summary>
    /// Adds the plug-ins in <paramref name="source"/> (a .dll, a .zip, or a folder searched recursively).
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
            if (source.EndsWith(".zip", StringComparison.OrdinalIgnoreCase))
            {
                temp = Path.Combine(Path.GetTempPath(), "WebSkinPlayer-import-" + Guid.NewGuid().ToString("N"));
                ZipFile.ExtractToDirectory(source, temp);
                root = temp;
            }
            var dlls = File.Exists(root) ? new List<string> { root }
                : Directory.EnumerateFiles(root, "*.*", SearchOption.AllDirectories).Where(IsPluginDll).ToList();
            var list = LoadStored();
            foreach (var dll in dlls)
            {
                string pack = Path.Combine(Folder, Safe(Path.GetFileNameWithoutExtension(dll)));
                CopyPack(Path.GetDirectoryName(dll)!, pack, File.Exists(root) ? dll : null);
                string copied = Path.Combine(pack, Path.GetFileName(dll));
                var (code, json) = RunHost($"probe \"{copied}\"", 30_000);
                if (json == null) { report.Add($"{Path.GetFileName(dll)}: no visualization found ({code})"); continue; }
                using var doc = JsonDocument.Parse(json);
                foreach (var fx in doc.RootElement.GetProperty("effects").EnumerateArray())
                {
                    string clsid = fx.GetProperty("clsid").GetString()!;
                    string title = fx.GetProperty("title").GetString() is { Length: > 0 } t ? t.Trim() : Path.GetFileNameWithoutExtension(dll);
                    var presets = fx.GetProperty("presets").EnumerateArray().Select(p => p.GetString() ?? "").ToList();
                    if (!Draws(copied, clsid)) { report.Add($"{title}: draws nothing here, not added"); continue; }
                    list.RemoveAll(s => s.Clsid.Equals(clsid, StringComparison.OrdinalIgnoreCase));
                    title = UniqueTitle(title, dll, list);
                    list.Add(new Stored
                    {
                        Id = Regex.Replace(title, "[^A-Za-z0-9]", ""), Title = title, Clsid = clsid,
                        Dll = Path.GetRelativePath(Folder, copied), Presets = presets.Count > 0 ? presets : new() { title }, Source = label,
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

    /// <summary>Renders a couple of seconds of test music and checks that something appeared.</summary>
    static bool Draws(string dll, string clsid)
    {
        string bmp = Path.Combine(Path.GetTempPath(), $"wsp-viz-{Guid.NewGuid():N}.bmp");
        try
        {
            var (_, json) = RunHost($"snapshot \"{dll}\" {clsid} \"{bmp}\"", 30_000);
            if (json == null) return false;
            using var doc = JsonDocument.Parse(json);
            return doc.RootElement.GetProperty("lit").GetDouble() > 0.001;
        }
        catch { return false; }
        finally { try { File.Delete(bmp); } catch { } }
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
