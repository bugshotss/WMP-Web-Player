// Test media source: plays a quiet synthesized loop through Windows.Media.Playback so it shows up
// as a System Media Transport Controls session (like Chrome does for SoundCloud).
// Usage: SmtcTestSource [seconds] [volume0to1] [songEverySeconds]
// songEverySeconds: change track SoundCloud-style (Paused for 0.2 s, then Playing the next title) that often; the
// third change is a real pause (3 s) instead.
using Windows.Media;
using Windows.Media.Core;
using Windows.Media.Playback;
using Windows.Storage;

int seconds = args.Length > 0 ? int.Parse(args[0]) : 60;
double volume = args.Length > 1 ? double.Parse(args[1], System.Globalization.CultureInfo.InvariantCulture) : 0.12;

string wav = Path.Combine(Path.GetTempPath(), "webskin-test-loop.wav");
WriteLoop(wav, 30);

var player = new MediaPlayer { Volume = volume, IsLoopingEnabled = true };
var file = await StorageFile.GetFileFromPathAsync(wav);
player.Source = MediaSource.CreateFromStorageFile(file);
var smtc = player.SystemMediaTransportControls;
player.CommandManager.IsEnabled = true;
player.CommandManager.NextBehavior.EnablingRule = MediaCommandEnablingRule.Always;
player.CommandManager.PreviousBehavior.EnablingRule = MediaCommandEnablingRule.Always;
int track = 1;
void SetMeta()
{
    var u = smtc.DisplayUpdater;
    u.Type = MediaPlaybackType.Music;
    u.MusicProperties.Title = $"Test Track {track}";
    u.MusicProperties.Artist = "WebSkin Test Source";
    u.MusicProperties.AlbumTitle = "Synth Loops";
    u.Update();
}
player.CommandManager.NextReceived += (_, e) => { track++; SetMeta(); Console.WriteLine("NEXT -> track " + track); };
player.CommandManager.PreviousReceived += (_, e) => { track = Math.Max(1, track - 1); SetMeta(); Console.WriteLine("PREVIOUS -> track " + track); };
player.CommandManager.PlayReceived += (_, _) => Console.WriteLine("PLAY");
player.CommandManager.PauseReceived += (_, _) => Console.WriteLine("PAUSE");
player.CommandManager.PositionReceived += (_, e) => Console.WriteLine("SEEK " + e.Position);
player.CurrentStateChanged += (s, _) => Console.WriteLine("state " + s.CurrentState);
player.Play();
await Task.Delay(500);
SetMeta();
Console.WriteLine($"Playing for {seconds}s at volume {volume}");
int songEvery = args.Length > 2 ? int.Parse(args[2]) : 0;
var end = DateTime.Now.AddSeconds(seconds);
for (int change = 1; songEvery > 0 && DateTime.Now.AddSeconds(songEvery) < end; change++)
{
    await Task.Delay(TimeSpan.FromSeconds(songEvery));
    bool realPause = change == 3;
    player.Pause();
    Console.WriteLine($"{DateTime.Now:HH:mm:ss.fff} {(realPause ? "real pause" : "song change")}");
    await Task.Delay(realPause ? 3000 : 200);
    if (!realPause) { track++; SetMeta(); }
    player.Play();
}
var left = end - DateTime.Now;
if (left > TimeSpan.Zero) await Task.Delay(left);
player.Pause();
player.Dispose();

static void WriteLoop(string path, int secs)
{
    const int rate = 44100;
    int n = rate * secs;
    using var w = new BinaryWriter(File.Create(path));
    w.Write("RIFF"u8); w.Write(36 + n * 4); w.Write("WAVE"u8);
    w.Write("fmt "u8); w.Write(16); w.Write((short)1); w.Write((short)2); w.Write(rate); w.Write(rate * 4); w.Write((short)4); w.Write((short)16);
    w.Write("data"u8); w.Write(n * 4);
    double[] chord = { 110, 138.6, 164.8, 220 };
    var rng = new Random(1);
    for (int i = 0; i < n; i++)
    {
        double t = i / (double)rate;
        double beat = t * 2 % 1;                   // 120 bpm
        double kick = Math.Sin(2 * Math.PI * (60 + 90 * Math.Exp(-beat * 30)) * t) * Math.Exp(-beat * 9);
        double hat = (rng.NextDouble() * 2 - 1) * Math.Exp(-((t * 4) % 1) * 40) * 0.25;
        double pad = 0;
        foreach (var f in chord) pad += Math.Sin(2 * Math.PI * f * (1 + 0.5 * ((int)(t / 2) % 2)) * t) * 0.12;
        double lead = Math.Sin(2 * Math.PI * (440 + 220 * Math.Sin(t * 1.3)) * t) * 0.1 * (0.5 + 0.5 * Math.Sin(t * 3));
        double l = (kick * 0.7 + hat + pad + lead) * 0.6, r = (kick * 0.7 + hat * 0.6 + pad + lead * 0.6) * 0.6;
        w.Write((short)(Math.Clamp(l, -1, 1) * 32000)); w.Write((short)(Math.Clamp(r, -1, 1) * 32000));
    }
}
