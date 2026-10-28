using System.IO;
using System.IO.Pipes;
using System.Text;

namespace WebSkinPlayer;

/// <summary>
/// One player per user: a second launch (e.g. double-clicking a .wmz) hands its arguments to the
/// running instance over a per-user named pipe and exits.
/// </summary>
public static class SingleInstance
{
    const string End = "\u0004";
    static Mutex? _mutex;
    static string PipeName => "WebSkinPlayer.Instance." + Environment.UserName + Scope;

    /// <summary>A separate data folder (WEBSKINPLAYER_DATA, used for clean-profile tests) is a separate player.</summary>
    static readonly string Scope = Environment.GetEnvironmentVariable("WEBSKINPLAYER_DATA") is { Length: > 0 } dir
        ? "." + Convert.ToHexString(System.Security.Cryptography.SHA256.HashData(Encoding.UTF8.GetBytes(Path.GetFullPath(dir).ToLowerInvariant())))[..12]
        : "";

    /// <summary>True when this process is the first instance.</summary>
    public static bool TryBecomePrimary()
    {
        _mutex = new Mutex(true, @"Local\WebSkinPlayer.SingleInstance" + Scope, out bool created);
        return created;
    }

    public static bool ForwardToPrimary(string[] args)
    {
        // The running copy may still be starting up, or busy with another launch's arguments.
        var deadline = DateTime.UtcNow.AddSeconds(15);
        while (true)
        {
            try
            {
                using var client = new NamedPipeClientStream(".", PipeName, PipeDirection.Out);
                client.Connect(1000);
                using var w = new StreamWriter(client, new UTF8Encoding(false));
                foreach (var a in args) w.WriteLine(a);
                w.WriteLine(End);
                w.Flush();
                return true;
            }
            catch (Exception ex) when (ex is TimeoutException or IOException && DateTime.UtcNow < deadline)
            {
                Thread.Sleep(200);
            }
            catch (Exception ex)
            {
                Log.Write("Couldn't hand off to the running instance: " + ex.Message);
                return false;
            }
        }
    }

    /// <summary>Receives forwarded argument lists on a background thread.</summary>
    public static void Listen(Action<string[]> onArgs)
    {
        Task.Run(async () =>
        {
            while (true)
            {
                try
                {
                    using var server = new NamedPipeServerStream(PipeName, PipeDirection.In, 1, PipeTransmissionMode.Byte, PipeOptions.Asynchronous);
                    await server.WaitForConnectionAsync();
                    using var r = new StreamReader(server, Encoding.UTF8);
                    var args = new List<string>();
                    string? line;
                    while ((line = await r.ReadLineAsync()) != null && line != End && args.Count < 32) args.Add(line);
                    onArgs(args.ToArray());
                }
                catch (Exception ex)
                {
                    Log.Write("Instance pipe error: " + ex.Message);
                    await Task.Delay(1000);
                }
            }
        });
    }
}
