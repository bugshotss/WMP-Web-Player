using System.IO;
using System.Runtime.InteropServices;
using System.Text;

namespace WebSkinPlayer.Skins;

/// <summary>
/// Resolves "res://module/type/#id" URLs that skins use for WMP's built-in strings, images and
/// scripts, by reading resources from the system's wmploc.dll (and its language MUI file).
/// </summary>
public static class WmpResources
{
    const int RT_BITMAP = 2, RT_STRING = 6, RT_HTML = 23, RT_TEXT = 256, RT_IMAGE = 257;
    const uint LOAD_LIBRARY_AS_DATAFILE = 0x02, LOAD_LIBRARY_AS_IMAGE_RESOURCE = 0x20;

    static readonly Dictionary<string, IntPtr[]> Modules = new(StringComparer.OrdinalIgnoreCase);
    static readonly Dictionary<string, byte[]?> Cache = new(StringComparer.OrdinalIgnoreCase);

    public static bool IsResourceUrl(string? s) => s != null && s.TrimStart().StartsWith("res://", StringComparison.OrdinalIgnoreCase);

    /// <summary>Parses res://module/type/name. Type may be omitted ("res://wmploc/#1691").</summary>
    static bool TryParse(string url, out string module, out string? type, out string name)
    {
        module = ""; type = null; name = "";
        string rest = url.Trim()[6..];
        var parts = rest.Split('/', StringSplitOptions.RemoveEmptyEntries);
        if (parts.Length < 2) return false;
        module = parts[0];
        if (parts.Length == 2) name = parts[1];
        else { type = parts[1]; name = parts[^1]; }
        return name.Length > 0 && name != "#";
    }

    static IntPtr[] ModuleHandles(string module)
    {
        string m = module.ToLowerInvariant();
        if (m == "-" || m.Length == 0 || m == "wmploc") m = "wmploc.dll";
        if (!m.EndsWith(".dll")) m += ".dll";
        if (Modules.TryGetValue(m, out var handles)) return handles;
        var list = new List<IntPtr>();
        string sys = Environment.GetFolderPath(Environment.SpecialFolder.System);
        string main = Path.Combine(sys, m);
        if (File.Exists(main))
        {
            IntPtr h = LoadLibraryEx(main, IntPtr.Zero, LOAD_LIBRARY_AS_DATAFILE | LOAD_LIBRARY_AS_IMAGE_RESOURCE);
            if (h != IntPtr.Zero) list.Add(h);
            // String tables live in the MUI satellite.
            foreach (var culture in new[] { System.Globalization.CultureInfo.CurrentUICulture.Name, "en-US" }.Distinct())
            {
                string mui = Path.Combine(sys, culture, m + ".mui");
                if (!File.Exists(mui)) continue;
                IntPtr hm = LoadLibraryEx(mui, IntPtr.Zero, LOAD_LIBRARY_AS_DATAFILE | LOAD_LIBRARY_AS_IMAGE_RESOURCE);
                if (hm != IntPtr.Zero) list.Add(hm);
            }
        }
        handles = list.ToArray();
        Modules[m] = handles;
        return handles;
    }

    static int TypeId(string? type) => type?.ToUpperInvariant() switch
    {
        "RT_BITMAP" or "2" or "#2" => RT_BITMAP,
        "RT_STRING" or "6" or "#6" => RT_STRING,
        "RT_HTML" or "23" or "#23" => RT_HTML,
        "RT_TEXT" or "256" or "#256" => RT_TEXT,
        "RT_IMAGE" or "257" or "#257" => RT_IMAGE,
        null => 0,
        var t when t.StartsWith('#') && int.TryParse(t[1..], out int n) => n,
        var t when int.TryParse(t, out int n) => n,
        _ => -1,
    };

    static byte[]? Find(IntPtr module, int type, string name)
    {
        IntPtr res;
        if (name.StartsWith('#') && int.TryParse(name[1..], out int id)) res = FindResource(module, (IntPtr)id, (IntPtr)type);
        else if (int.TryParse(name, out id)) res = FindResource(module, (IntPtr)id, (IntPtr)type);
        else res = FindResourceS(module, name, (IntPtr)type);
        if (res == IntPtr.Zero) return null;
        IntPtr data = LockResource(LoadResource(module, res));
        uint size = SizeofResource(module, res);
        if (data == IntPtr.Zero || size == 0) return null;
        var bytes = new byte[size];
        Marshal.Copy(data, bytes, 0, (int)size);
        return bytes;
    }

    /// <summary>Raw resource bytes (images, scripts). RT_BITMAP data is turned into a .bmp file.</summary>
    public static byte[]? ReadBytes(string url)
    {
        if (Cache.TryGetValue(url, out var cached)) return cached;
        byte[]? result = null;
        if (TryParse(url, out var module, out var type, out var name))
        {
            int t = TypeId(type);
            var types = t > 0 ? new[] { t } : new[] { RT_IMAGE, RT_BITMAP, RT_TEXT, RT_HTML };
            foreach (var h in ModuleHandles(module))
            {
                foreach (int ty in types)
                {
                    var b = Find(h, ty, name);
                    if (b == null) continue;
                    result = ty == RT_BITMAP ? DibToBmp(b) : b;
                    break;
                }
                if (result != null) break;
            }
        }
        Cache[url] = result;
        return result;
    }

    public static string? ReadText(string url)
    {
        if (TryParse(url, out var module, out var type, out var name) && TypeId(type) == RT_STRING)
            return LoadString(module, name);
        var bytes = ReadBytes(url);
        return bytes == null ? null : SkinPackage.DecodeText(bytes);
    }

    static string? LoadString(string module, string name)
    {
        if (!int.TryParse(name.TrimStart('#'), out int id)) return null;
        int block = (id >> 4) + 1, index = id & 15;
        foreach (var h in ModuleHandles(module))
        {
            var data = Find(h, RT_STRING, "#" + block);
            if (data == null) continue;
            int pos = 0;
            for (int i = 0; i < 16 && pos + 2 <= data.Length; i++)
            {
                int len = BitConverter.ToUInt16(data, pos);
                pos += 2;
                if (i == index) return Encoding.Unicode.GetString(data, pos, Math.Min(len * 2, data.Length - pos));
                pos += len * 2;
            }
        }
        return null;
    }

    static byte[] DibToBmp(byte[] dib)
    {
        int headerSize = BitConverter.ToInt32(dib, 0);
        int bitCount = BitConverter.ToInt16(dib, 14);
        int clrUsed = headerSize >= 36 ? BitConverter.ToInt32(dib, 32) : 0;
        int compression = BitConverter.ToInt32(dib, 16);
        int palette = bitCount <= 8 ? (clrUsed > 0 ? clrUsed : 1 << bitCount) * 4 : 0;
        if (compression == 3 && headerSize == 40) palette = 12; // BI_BITFIELDS masks
        var bmp = new byte[14 + dib.Length];
        bmp[0] = (byte)'B'; bmp[1] = (byte)'M';
        BitConverter.GetBytes(bmp.Length).CopyTo(bmp, 2);
        BitConverter.GetBytes(14 + headerSize + palette).CopyTo(bmp, 10);
        Buffer.BlockCopy(dib, 0, bmp, 14, dib.Length);
        return bmp;
    }

    [DllImport("kernel32", SetLastError = true, CharSet = CharSet.Unicode)] static extern IntPtr LoadLibraryEx(string file, IntPtr h, uint flags);
    [DllImport("kernel32", CharSet = CharSet.Unicode)] static extern IntPtr FindResource(IntPtr h, IntPtr name, IntPtr type);
    [DllImport("kernel32", CharSet = CharSet.Unicode, EntryPoint = "FindResourceW")] static extern IntPtr FindResourceS(IntPtr h, string name, IntPtr type);
    [DllImport("kernel32")] static extern IntPtr LoadResource(IntPtr h, IntPtr res);
    [DllImport("kernel32")] static extern IntPtr LockResource(IntPtr h);
    [DllImport("kernel32")] static extern uint SizeofResource(IntPtr h, IntPtr res);
}
