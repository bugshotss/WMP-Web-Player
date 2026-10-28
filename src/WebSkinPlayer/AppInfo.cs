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

public sealed record UpdateInfo(string Version, string PageUrl);

/// <summary>Looks for a newer GitHub release once per start; the UI offers its download page.</summary>
public static class UpdateChecker
{
    public static UpdateInfo? Available { get; private set; }
    public static event Action? Found;

    public static async Task CheckAsync()
    {
        if (Sandbox.Enabled || AppInfo.RepositoryUrl is not { } repo) return;
        var m = Regex.Match(repo, @"github\.com/([^/]+/[^/]+?)(?:\.git)?$", RegexOptions.IgnoreCase);
        if (!m.Success) return;
        try
        {
            using var http = new HttpClient { Timeout = TimeSpan.FromSeconds(20) };
            http.DefaultRequestHeaders.UserAgent.ParseAdd(AppInfo.UserAgent);
            http.DefaultRequestHeaders.Accept.ParseAdd("application/vnd.github+json");
            using var doc = JsonDocument.Parse(await http.GetStringAsync($"https://api.github.com/repos/{m.Groups[1].Value}/releases/latest"));
            string tag = doc.RootElement.GetProperty("tag_name").GetString() ?? "";
            string page = doc.RootElement.TryGetProperty("html_url", out var u) && u.GetString() is { } s ? s : repo + "/releases/latest";
            if (!IsNewer(tag, AppInfo.Version)) return;
            Available = new UpdateInfo(tag.TrimStart('v', 'V'), page);
            Log.Write($"Update available: {tag} (running {AppInfo.Version})");
            Found?.Invoke();
        }
        catch (Exception ex) { Log.Write("Update check failed: " + ex.Message); }
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
