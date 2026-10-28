using System.IO;
using System.IO.Compression;

namespace WebSkinPlayer.Skins;

public sealed class SkinEntry
{
    public required string Path { get; init; }
    public required string FileName { get; init; }
    public string Title { get; set; } = "";
    public string Author { get; set; } = "";
    public string Copyright { get; set; } = "";
    public long Size { get; init; }
    public bool MetadataLoaded { get; set; }
    public string DisplayName => Title.Length > 0 ? Title : PrettyName(FileName);

    internal static string PrettyName(string file)
    {
        string n = System.IO.Path.GetFileNameWithoutExtension(file).Replace('_', ' ').Trim();
        while (n.Contains("  ")) n = n.Replace("  ", " ");
        return n;
    }
    public override string ToString() => DisplayName;
}

/// <summary>Installed skins: %LOCALAPPDATA%\WebSkinPlayer\Skins plus any WMP skins folder.</summary>
public sealed class SkinLibrary
{
    public static string InstallFolder { get; } = System.IO.Path.Combine(Log.Folder, "Skins");

    static readonly string[] ExtraFolders =
    {
        System.IO.Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles), "Windows Media Player", "Skins"),
        System.IO.Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ProgramFilesX86), "Windows Media Player", "Skins"),
    };

    readonly Dictionary<string, SkinEntry> _cache = new(StringComparer.OrdinalIgnoreCase);
    public event Action? Changed;

    public SkinLibrary()
    {
        Directory.CreateDirectory(InstallFolder);
    }

    public IReadOnlyList<SkinEntry> Scan()
    {
        var list = new List<SkinEntry>();
        foreach (var folder in ExtraFolders.Prepend(InstallFolder).Distinct())
        {
            if (!Directory.Exists(folder)) continue;
            foreach (var file in Directory.EnumerateFiles(folder, "*.wmz").Concat(Directory.EnumerateFiles(folder, "*.wms")))
            {
                if (!_cache.TryGetValue(file, out var entry))
                {
                    entry = new SkinEntry { Path = file, FileName = System.IO.Path.GetFileName(file), Size = new FileInfo(file).Length };
                    _cache[file] = entry;
                }
                list.Add(entry);
            }
        }
        return list.OrderBy(e => e.DisplayName, StringComparer.CurrentCultureIgnoreCase).ToList();
    }

    /// <summary>Reads the THEME title/author once (cheap: only parses the .wms header).</summary>
    public void EnsureMetadata(SkinEntry entry)
    {
        if (entry.MetadataLoaded) return;
        entry.MetadataLoaded = true;
        try
        {
            using var pkg = SkinPackage.Open(entry.Path);
            var root = WmsParser.Parse(pkg.ReadText(pkg.DefinitionFile));
            var theme = root.Children.FirstOrDefault(n => n.Tag == "THEME") ?? root;
            entry.Title = (theme.Attr("title") ?? "").Trim();
            entry.Author = (theme.Attr("author") ?? "").Trim();
            entry.Copyright = (theme.Attr("copyright") ?? "").Trim();
            // Some themes carry a resource id or placeholder instead of a real title.
            if (entry.Title.StartsWith("res:", StringComparison.OrdinalIgnoreCase) || entry.Title.StartsWith("jscript:", StringComparison.OrdinalIgnoreCase))
                entry.Title = "";
        }
        catch (Exception ex) { Log.Write($"Skin metadata {entry.FileName}: {ex.Message}"); }
    }

    /// <summary>Installs every .wmz inside a .zip archive (e.g. a downloaded skin collection).</summary>
    public int InstallFromZip(string zipPath)
    {
        int count = 0;
        using var zip = System.IO.Compression.ZipFile.OpenRead(zipPath);
        foreach (var entry in zip.Entries)
        {
            if (!entry.Name.EndsWith(".wmz", StringComparison.OrdinalIgnoreCase) || entry.Length == 0) continue;
            string dest = System.IO.Path.Combine(InstallFolder, entry.Name);
            try
            {
                entry.ExtractToFile(dest, overwrite: true);
                _cache.Remove(dest);
                count++;
            }
            catch (Exception ex) { Log.Write($"Install {entry.Name} failed: {ex.Message}"); }
        }
        if (count > 0) Changed?.Invoke();
        return count;
    }

    /// <summary>Copies a skin file into the install folder (like double-clicking a .wmz did).</summary>
    public string? Install(string sourcePath)
    {
        try
        {
            if (!File.Exists(sourcePath)) return null;
            if (sourcePath.EndsWith(".zip", StringComparison.OrdinalIgnoreCase)) { InstallFromZip(sourcePath); return null; }
            string full = System.IO.Path.GetFullPath(sourcePath);
            if (full.StartsWith(InstallFolder, StringComparison.OrdinalIgnoreCase)) return full;
            string dest = System.IO.Path.Combine(InstallFolder, System.IO.Path.GetFileName(full));
            File.Copy(full, dest, overwrite: true);
            _cache.Remove(dest);
            Changed?.Invoke();
            return dest;
        }
        catch (Exception ex)
        {
            Log.Write($"Install {sourcePath} failed: {ex.Message}");
            return null;
        }
    }

    /// <summary>Files that were placed in the install folder directly (downloads).</summary>
    public void NotifyInstalled(IEnumerable<string> paths)
    {
        foreach (var p in paths) _cache.Remove(p);
        Changed?.Invoke();
    }

    public bool Delete(SkinEntry entry)
    {
        try
        {
            if (!entry.Path.StartsWith(InstallFolder, StringComparison.OrdinalIgnoreCase)) return false;
            File.Delete(entry.Path);
            _cache.Remove(entry.Path);
            Changed?.Invoke();
            return true;
        }
        catch (Exception ex) { Log.Write($"Delete {entry.Path} failed: {ex.Message}"); return false; }
    }
}
