using System.IO;

namespace WebSkinPlayer;

/// <summary>Tiny append-only diagnostics log in %LOCALAPPDATA%\WebSkinPlayer\log.txt.</summary>
public static class Log
{
    static readonly object Gate = new();
    /// <summary>
    /// Settings, skins and the log. Normally %LOCALAPPDATA%\WebSkinPlayer; a "portable" file next to the exe
    /// keeps everything in a Data folder beside it, and WEBSKINPLAYER_DATA overrides both (used for clean-machine tests).
    /// </summary>
    public static string Folder { get; } = ResolveFolder();
    static readonly string FilePath = Path.Combine(Folder, "log.txt");

    static string ResolveFolder()
    {
        string? env = Environment.GetEnvironmentVariable("WEBSKINPLAYER_DATA");
        if (!string.IsNullOrWhiteSpace(env)) return Path.GetFullPath(env);
        string exeDir = AppContext.BaseDirectory;
        if (File.Exists(Path.Combine(exeDir, "portable")) || File.Exists(Path.Combine(exeDir, "portable.txt"))) return Path.Combine(exeDir, "Data");
        return Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "WebSkinPlayer");
    }

    static Log()
    {
        try
        {
            Directory.CreateDirectory(Folder);
            if (File.Exists(FilePath) && new FileInfo(FilePath).Length > 2_000_000) File.Delete(FilePath);
        }
        catch { }
    }

    public static event Action<string>? Written;

    public static void Write(string message)
    {
        string line = $"{DateTime.Now:HH:mm:ss.fff} {message}";
        System.Diagnostics.Debug.WriteLine(line);
        lock (Gate)
        {
            try { File.AppendAllText(FilePath, line + Environment.NewLine); } catch { }
        }
        Written?.Invoke(line);
    }
}
