using System.IO;
using System.Net.Http;
using System.Text.Json;

namespace WebSkinPlayer.Skins;

/// <summary>A skin in the online collection.</summary>
public sealed class OnlineSkin
{
    public required string FileName { get; init; }
    public long Size { get; init; }
    public bool Installed { get; set; }
    public string DisplayName => SkinEntry.PrettyName(FileName);
    /// <summary>Screenshot of the skin taken in WMP, published with it.</summary>
    public string PreviewUrl => OnlineSkins.FileUrl(Path.ChangeExtension(FileName, ".png"));
    public override string ToString() => Installed ? DisplayName + "   ✓" : DisplayName;
}

/// <summary>
/// The "Windows Media Player Skins Collection" on archive.org (the WMP Skins Archive): 175 .wmz files,
/// each with a screenshot. The app ships without skins and downloads them from here on request.
/// </summary>
public static class OnlineSkins
{
    public const string Item = "windowsmediaplayerskinscollection";
    public const string PageUrl = "https://archive.org/details/" + Item;

    static readonly HttpClient Http = CreateClient();

    static HttpClient CreateClient()
    {
        var http = new HttpClient { Timeout = TimeSpan.FromMinutes(10) };
        http.DefaultRequestHeaders.UserAgent.ParseAdd(AppInfo.UserAgent);
        return http;
    }

    public static string FileUrl(string name) => $"https://archive.org/download/{Item}/{Uri.EscapeDataString(name)}";

    public static async Task<List<OnlineSkin>> ListAsync(CancellationToken ct)
    {
        string json = await Http.GetStringAsync($"https://archive.org/metadata/{Item}/files", ct);
        using var doc = JsonDocument.Parse(json);
        var list = new List<OnlineSkin>();
        foreach (var f in doc.RootElement.GetProperty("result").EnumerateArray())
        {
            string name = f.TryGetProperty("name", out var n) ? n.GetString() ?? "" : "";
            if (!name.EndsWith(".wmz", StringComparison.OrdinalIgnoreCase) || name.Contains('/')) continue;
            long size = f.TryGetProperty("size", out var s) && long.TryParse(s.GetString(), out var bytes) ? bytes : 0;
            list.Add(new OnlineSkin { FileName = name, Size = size });
        }
        return list.OrderBy(s => s.DisplayName, StringComparer.CurrentCultureIgnoreCase).ToList();
    }

    /// <summary>Downloads one skin into <paramref name="folder"/>, reporting bytes as they arrive.</summary>
    public static async Task<string> DownloadAsync(OnlineSkin skin, string folder, IProgress<long>? progress, CancellationToken ct)
    {
        string dest = Path.Combine(folder, skin.FileName);
        string part = dest + ".part";
        try
        {
            using (var resp = await Http.GetAsync(FileUrl(skin.FileName), HttpCompletionOption.ResponseHeadersRead, ct))
            {
                resp.EnsureSuccessStatusCode();
                await using var src = await resp.Content.ReadAsStreamAsync(ct);
                await using var dst = File.Create(part);
                var buffer = new byte[81920];
                int read;
                while ((read = await src.ReadAsync(buffer, ct)) > 0)
                {
                    await dst.WriteAsync(buffer.AsMemory(0, read), ct);
                    progress?.Report(read);
                }
            }
            // A .wmz is a zip; anything else is an error page or a broken transfer.
            using (var check = File.OpenRead(part))
            {
                var sig = new byte[2];
                if (check.Read(sig) < 2 || sig[0] != 'P' || sig[1] != 'K') throw new InvalidDataException("not a skin file");
            }
            File.Move(part, dest, overwrite: true);
            return dest;
        }
        finally
        {
            try { if (File.Exists(part)) File.Delete(part); } catch { }
        }
    }
}
