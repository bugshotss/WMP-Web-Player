using System.IO;
using System.IO.Compression;
using System.Text;

namespace WebSkinPlayer.Skins;

/// <summary>
/// The files of one skin: a .wmz (zip) archive or a loose folder containing a .wms.
/// File names are matched case-insensitively and with or without sub-folders, as WMP did.
/// </summary>
public sealed class SkinPackage : IDisposable
{
    readonly Dictionary<string, byte[]> _files = new(StringComparer.OrdinalIgnoreCase);
    readonly Dictionary<string, string> _byBaseName = new(StringComparer.OrdinalIgnoreCase);

    public string SourcePath { get; }
    public string DefinitionFile { get; private set; } = "";

    static SkinPackage() => Encoding.RegisterProvider(CodePagesEncodingProvider.Instance);

    SkinPackage(string source) => SourcePath = source;

    public static SkinPackage Open(string path)
    {
        var pkg = new SkinPackage(Path.GetFullPath(path));
        if (path.EndsWith(".wms", StringComparison.OrdinalIgnoreCase))
        {
            string dir = Path.GetDirectoryName(pkg.SourcePath)!;
            foreach (var f in Directory.EnumerateFiles(dir, "*", SearchOption.AllDirectories))
            {
                var info = new FileInfo(f);
                if (info.Length > 64 * 1024 * 1024) continue;
                pkg.Add(Path.GetRelativePath(dir, f), File.ReadAllBytes(f));
            }
            pkg.DefinitionFile = Path.GetFileName(path);
        }
        else
        {
            var bytes = File.ReadAllBytes(path);
            var order = new List<string>();
            ZipArchive zip;
            try { zip = new ZipArchive(new MemoryStream(bytes), ZipArchiveMode.Read); _ = zip.Entries.Count; ReadAll(zip); }
            catch (InvalidDataException)
            {
                // Some skins have damaged local file headers that WMP never checked; repair and retry.
                if (!RepairLocalHeaders(bytes)) throw;
                pkg._files.Clear(); pkg._byBaseName.Clear(); order.Clear();
                zip = new ZipArchive(new MemoryStream(bytes), ZipArchiveMode.Read);
                ReadAll(zip);
            }
            zip.Dispose();

            void ReadAll(ZipArchive z)
            {
                foreach (var entry in z.Entries)
                {
                    if (entry.FullName.EndsWith('/') || entry.Length > 64 * 1024 * 1024) continue;
                    using var s = entry.Open();
                    var ms = new MemoryStream((int)entry.Length);
                    s.CopyTo(ms);
                    pkg.Add(entry.FullName, ms.ToArray());
                    order.Add(Normalize(entry.FullName));
                }
            }

            // Prefer the .wms named like the .wmz, then the first one at the shallowest level.
            var defs = order.Where(k => k.EndsWith(".wms", StringComparison.OrdinalIgnoreCase)).ToList();
            string stem = Path.GetFileNameWithoutExtension(path);
            pkg.DefinitionFile = defs.FirstOrDefault(k => Path.GetFileNameWithoutExtension(k).Equals(stem, StringComparison.OrdinalIgnoreCase))
                                 ?? defs.OrderBy(k => k.Count(c => c == '/')).FirstOrDefault()
                                 ?? throw new InvalidDataException("The skin archive has no .wms skin definition.");
        }
        return pkg;
    }

    /// <summary>Rewrites "PK\x03\x04" at every local header offset listed in the central directory.</summary>
    static bool RepairLocalHeaders(byte[] b)
    {
        int eocd = -1;
        for (int i = b.Length - 22; i >= Math.Max(0, b.Length - 70_000); i--)
            if (b[i] == 0x50 && b[i + 1] == 0x4B && b[i + 2] == 5 && b[i + 3] == 6) { eocd = i; break; }
        if (eocd < 0) return false;
        int count = BitConverter.ToUInt16(b, eocd + 10);
        long p = BitConverter.ToUInt32(b, eocd + 16);
        bool changed = false;
        for (int k = 0; k < count && p + 46 <= b.Length; k++)
        {
            if (BitConverter.ToUInt32(b, (int)p) != 0x02014B50) return changed;
            long local = BitConverter.ToUInt32(b, (int)p + 42);
            if (local + 4 <= b.Length && BitConverter.ToUInt32(b, (int)local) != 0x04034B50)
            {
                b[local] = 0x50; b[local + 1] = 0x4B; b[local + 2] = 3; b[local + 3] = 4;
                changed = true;
            }
            p += 46 + BitConverter.ToUInt16(b, (int)p + 28) + BitConverter.ToUInt16(b, (int)p + 30) + BitConverter.ToUInt16(b, (int)p + 32);
        }
        return changed;
    }

    void Add(string name, byte[] data)
    {
        string key = Normalize(name);
        _files[key] = data;
        string baseName = key.Contains('/') ? key[(key.LastIndexOf('/') + 1)..] : key;
        _byBaseName.TryAdd(baseName, key);
    }

    static string Normalize(string name)
    {
        name = name.Replace('\\', '/').Trim();
        while (name.StartsWith("./")) name = name[2..];
        return name.TrimStart('/');
    }

    public IEnumerable<string> Files => _files.Keys;

    public bool Exists(string name) => Resolve(name) != null;

    string? Resolve(string name)
    {
        if (string.IsNullOrWhiteSpace(name)) return null;
        string key = Normalize(name);
        if (_files.ContainsKey(key)) return key;
        string baseName = key.Contains('/') ? key[(key.LastIndexOf('/') + 1)..] : key;
        return _byBaseName.TryGetValue(baseName, out var k) ? k : null;
    }

    public byte[]? ReadBytes(string name)
    {
        var key = Resolve(name);
        return key != null ? _files[key] : null;
    }

    public string ReadText(string name)
    {
        var bytes = ReadBytes(name) ?? throw new FileNotFoundException($"'{name}' is not in the skin.");
        return DecodeText(bytes);
    }

    /// <summary>Skin text files are UTF-16 (with BOM), UTF-8 (with BOM) or the ANSI code page.</summary>
    public static string DecodeText(byte[] bytes)
    {
        if (bytes.Length >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE) return Encoding.Unicode.GetString(bytes, 2, bytes.Length - 2);
        if (bytes.Length >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF) return Encoding.BigEndianUnicode.GetString(bytes, 2, bytes.Length - 2);
        if (bytes.Length >= 3 && bytes[0] == 0xEF && bytes[1] == 0xBB && bytes[2] == 0xBF) return Encoding.UTF8.GetString(bytes, 3, bytes.Length - 3);
        // UTF-16 without BOM: lots of zero high bytes.
        if (bytes.Length > 8 && bytes[1] == 0 && bytes[3] == 0 && bytes[0] != 0) return Encoding.Unicode.GetString(bytes);
        return Encoding.GetEncoding(1252).GetString(bytes);
    }

    public void Dispose() => _files.Clear();
}
