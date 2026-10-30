using System.IO;
using System.Net.Http;
using System.Reflection;
using System.Text.Json;
using System.Text.RegularExpressions;

namespace WebSkinPlayer;

/// <summary>Build version and the GitHub repository a release build came from.</summary>
public static class AppInfo
{
    static readonly Assembly Asm = typeof(AppInfo).Assembly;

    /// <summary>"0.2.0" (or "0.2.0-ci.14" for a CI build), without the commit suffix.</summary>
    public static string Version { get; } =
        (Asm.GetCustomAttribute<AssemblyInformationalVersionAttribute>()?.InformationalVersion ?? "0.0.0").Split('+')[0];

    /// <summary>Set by the release workflow (-p:RepositoryUrl=...); null for local builds, which then never check for updates.</summary>
    public static string? RepositoryUrl { get; } =
        Asm.GetCustomAttributes<AssemblyMetadataAttribute>().FirstOrDefault(a => a.Key == "RepositoryUrl")?.Value is { Length: > 0 } url ? url.TrimEnd('/') : null;

    public static string UserAgent => "WebSkinPlayer/" + Version;
}

/// <param name="InstallerUrl">The release's Setup.exe, when it has one.</param>
/// <param name="Sha256">The installer's checksum as GitHub reports it (hex), if it does.</param>
public sealed record UpdateInfo(string Version, string PageUrl, string? InstallerUrl, long InstallerSize, string? Sha256);

/// <summary>
/// Looks for a newer GitHub release once per start. An installed player can then download the release's Setup.exe,
/// check it, and run it: the installer closes the player, upgrades it in place and starts it again.
/// </summary>
public static class UpdateChecker
{
    public static UpdateInfo? Available { get; private set; }
    public static event Action? Found;

    /// <summary>Test hook: WEBSKINPLAYER_UPDATES=&lt;url&gt; reads a releases/latest-shaped answer from there instead.</summary>
    static readonly string? FeedOverride = Environment.GetEnvironmentVariable("WEBSKINPLAYER_UPDATES") is { Length: > 0 } f ? f : null;

    static string DownloadFolder => Path.Combine(Path.GetTempPath(), "WebSkinPlayer-Update");

    public static async Task CheckAsync()
    {
        if (Sandbox.Enabled) return;
        // The last update's installer, if any, has done its job (one still running is in use and stays).
        try { if (Directory.Exists(DownloadFolder)) Directory.Delete(DownloadFolder, true); } catch (IOException) { } catch (UnauthorizedAccessException) { }
        string? feed = FeedOverride;
        string repo = AppInfo.RepositoryUrl ?? "";
        if (feed == null)
        {
            var m = Regex.Match(repo, @"github\.com/([^/]+/[^/]+?)(?:\.git)?$", RegexOptions.IgnoreCase);
            if (!m.Success) return;
            feed = $"https://api.github.com/repos/{m.Groups[1].Value}/releases/latest";
        }
        try
        {
            using var http = Client(TimeSpan.FromSeconds(20));
            http.DefaultRequestHeaders.Accept.ParseAdd("application/vnd.github+json");
            using var doc = JsonDocument.Parse(await http.GetStringAsync(feed));
            var root = doc.RootElement;
            string tag = root.GetProperty("tag_name").GetString() ?? "";
            string page = root.TryGetProperty("html_url", out var u) && u.GetString() is { } s ? s : repo + "/releases/latest";
            if (!IsNewer(tag, AppInfo.Version)) return;
            var (url, size, sha) = FindInstaller(root);
            Available = new UpdateInfo(tag.TrimStart('v', 'V'), page, url, size, sha);
            Log.Write($"Update available: {tag} (running {AppInfo.Version}){(url != null ? "" : ", without an installer")}");
            Found?.Invoke();
        }
        catch (Exception ex) { Log.Write("Update check failed: " + ex.Message); }
    }

    /// <summary>The release's public WebSkinPlayer-x.y.z-Setup.exe.</summary>
    static (string? Url, long Size, string? Sha256) FindInstaller(JsonElement release)
    {
        if (!release.TryGetProperty("assets", out var assets)) return (null, 0, null);
        foreach (var a in assets.EnumerateArray())
        {
            string name = a.TryGetProperty("name", out var n) ? n.GetString() ?? "" : "";
            if (!name.EndsWith("-Setup.exe", StringComparison.OrdinalIgnoreCase) || name.Contains("Personal", StringComparison.OrdinalIgnoreCase)) continue;
            string? url = a.TryGetProperty("browser_download_url", out var d) ? d.GetString() : null;
            long size = a.TryGetProperty("size", out var z) && z.TryGetInt64(out long bytes) ? bytes : 0;
            string? sha = a.TryGetProperty("digest", out var g) && g.GetString() is { } digest && digest.StartsWith("sha256:", StringComparison.OrdinalIgnoreCase) ? digest[7..] : null;
            return (url, size, sha);
        }
        return (null, 0, null);
    }

    /// <summary>The installer can update this copy: it was installed by it (portable copies go to the download page).</summary>
    public static bool CanInstall(UpdateInfo update) =>
        update.InstallerUrl != null && File.Exists(Path.Combine(AppContext.BaseDirectory, "unins000.exe"))
        && !Log.Folder.Equals(Path.Combine(AppContext.BaseDirectory, "Data"), StringComparison.OrdinalIgnoreCase);

    /// <summary>Downloads the installer into %TEMP%, checks its size and checksum, and returns its path.</summary>
    public static async Task<string> DownloadAsync(UpdateInfo update, IProgress<double> progress, CancellationToken cancel)
    {
        string dir = DownloadFolder;
        Directory.CreateDirectory(dir);
        string file = Path.Combine(dir, $"WebSkinPlayer-{update.Version}-Setup.exe"), part = file + ".part";
        using var http = Client(TimeSpan.FromMinutes(15));
        using (var response = await http.GetAsync(update.InstallerUrl, HttpCompletionOption.ResponseHeadersRead, cancel))
        {
            response.EnsureSuccessStatusCode();
            long total = response.Content.Headers.ContentLength ?? update.InstallerSize;
            await using var source = await response.Content.ReadAsStreamAsync(cancel);
            await using var target = File.Create(part);
            var buffer = new byte[81920];
            long done = 0;
            for (int read; (read = await source.ReadAsync(buffer, cancel)) > 0;)
            {
                await target.WriteAsync(buffer.AsMemory(0, read), cancel);
                done += read;
                if (total > 0) progress.Report((double)done / total);
            }
        }
        long length = new FileInfo(part).Length;
        if (update.InstallerSize > 0 && length != update.InstallerSize)
            throw new InvalidDataException($"the download is {length:N0} bytes, the release says {update.InstallerSize:N0}");
        if (update.Sha256 != null)
        {
            await using var check = File.OpenRead(part);
            string actual = Convert.ToHexString(await System.Security.Cryptography.SHA256.HashDataAsync(check, cancel));
            if (!actual.Equals(update.Sha256, StringComparison.OrdinalIgnoreCase))
                throw new InvalidDataException("the download doesn't match the release's checksum");
        }
        File.Move(part, file, overwrite: true);
        Log.Write($"Update {update.Version} downloaded ({length:N0} bytes{(update.Sha256 != null ? ", checksum verified" : "")})");
        return file;
    }

    /// <summary>Runs the installer (it closes this player, upgrades it and starts it again) and quits.</summary>
    public static void Install(string installer)
    {
        Log.Write($"Running {Path.GetFileName(installer)} to update");
        System.Diagnostics.Process.Start(new System.Diagnostics.ProcessStartInfo(installer, "/SILENT /SUPPRESSMSGBOXES /NORESTART /restart=1")
        {
            UseShellExecute = false,
        });
        App.Modes.Exit();
    }

    static HttpClient Client(TimeSpan timeout)
    {
        var http = new HttpClient { Timeout = timeout };
        http.DefaultRequestHeaders.UserAgent.ParseAdd(AppInfo.UserAgent);
        return http;
    }

    static bool IsNewer(string tag, string current) =>
        System.Version.TryParse(Core(tag), out var latest) && System.Version.TryParse(Core(current), out var mine) && latest > mine;

    static string Core(string v)
    {
        v = v.Trim().TrimStart('v', 'V');
        int cut = v.IndexOfAny(['-', '+']);
        return cut >= 0 ? v[..cut] : v;
    }
}
