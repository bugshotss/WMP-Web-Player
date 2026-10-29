using System.IO;
using System.Diagnostics;
using System.IO.MemoryMappedFiles;
using WebSkinPlayer.Audio;

namespace WebSkinPlayer.Visualization;

/// <summary>
/// A 32-bit WMP visualization plug-in, run by WebSkinPlayer.VisHost32 because a 64-bit process can't load it.
/// Each frame the audio goes to the helper through shared memory and the helper's latest picture comes back
/// (one frame behind, so the player never waits on it). The layout matches the helper's Serve class.
/// </summary>
public sealed unsafe class Hosted32Visualization : IVisualization
{
    const int LevelsOffset = 64, PixelsOffset = 4224, MaxWidth = 1280, MaxHeight = 720;
    static int s_count;

    readonly PluginEntry _entry;
    MemoryMappedFile? _map;
    MemoryMappedViewAccessor? _view;
    byte* _base;
    EventWaitHandle? _input;
    Process? _host;
    int _lastOutput, _preset;
    bool _failed;

    public Hosted32Visualization(PluginEntry entry) => _entry = entry;

    public string Id => _entry.Id;
    public string Title => _entry.Title;
    public IReadOnlyList<string> Presets => _entry.Presets;
    /// <summary>These were written for 2000-era windows; they're drawn smaller and scaled up.</summary>
    public int MaxPixelsHint => 800 * 450;

    public int CurrentPreset
    {
        get => _preset;
        set => _preset = Math.Clamp(value, 0, Math.Max(0, _entry.Presets.Count - 1));
    }

    void Start(int width, int height)
    {
        string name = $"WebSkinPlayer.Vis.{Environment.ProcessId}.{Interlocked.Increment(ref s_count)}";
        _map = MemoryMappedFile.CreateNew(name, PixelsOffset + (long)MaxWidth * MaxHeight * 4);
        _view = _map.CreateViewAccessor();
        _view.SafeMemoryMappedViewHandle.AcquirePointer(ref _base);
        // The size is there before the helper starts: DirectX plug-ins size their device to the first window.
        ((int*)_base)[1] = width;
        ((int*)_base)[2] = height;
        _input = new EventWaitHandle(false, EventResetMode.AutoReset, name + ".in");
        var psi = new ProcessStartInfo(PluginLibrary.HostPath, $"serve \"{_entry.DllPath}\" {_entry.Clsid} {name} {Environment.ProcessId} {_entry.Output}")
        {
            UseShellExecute = false,
            CreateNoWindow = true,
            WorkingDirectory = Path.GetDirectoryName(_entry.DllPath)!,
        };
        _host = Process.Start(psi);
        Log.Write($"Visualization helper started for {Title} ({Path.GetFileName(_entry.DllPath)})");
    }

    public void Render(VisFrame frame, PixelBuffer target, VisContext context)
    {
        if (_failed) { target.Clear(0); return; }
        try
        {
            if (_host == null) Start(target.Width, target.Height);
            int* header = (int*)_base;
            header[1] = target.Width;
            header[2] = target.Height;
            header[3] = _preset;
            fixed (byte* f = frame.Frequency, w = frame.Waveform)
            {
                Buffer.MemoryCopy(f, _base + LevelsOffset, 2048, 2048);
                Buffer.MemoryCopy(w, _base + LevelsOffset + 2048, 2048, 2048);
            }
            *(int*)(_base + LevelsOffset + 4096) = frame.Playing ? 2 : 0;
            *(long*)(_base + LevelsOffset + 4104) = frame.TimeStamp;
            header[4]++;
            _input!.Set();

            int output = Volatile.Read(ref header[5]);
            if (output != _lastOutput)
            {
                _lastOutput = output;
                CopyPicture(header[6], header[7], target);
            }
            if (header[9] < 0 || _host!.HasExited) Fail(header[9] < 0 ? "couldn't create it" : $"helper exited ({_host!.ExitCode})", target);
        }
        catch (Exception ex) { Fail(ex.Message, target); }
    }

    /// <summary>The helper's picture into the target, scaled (nearest) if the size changed in between.</summary>
    void CopyPicture(int w, int h, PixelBuffer target)
    {
        if (w <= 0 || h <= 0) return;
        int* src = (int*)(_base + PixelsOffset);
        if (w == target.Width && h == target.Height) { Buffer.MemoryCopy(src, target.Pixels, (long)target.Length * 4, (long)w * h * 4); return; }
        for (int y = 0; y < target.Height; y++)
        {
            int* row = src + (long)(y * h / target.Height) * w;
            int* dst = target.Pixels + (long)y * target.Width;
            for (int x = 0; x < target.Width; x++) dst[x] = row[x * w / target.Width];
        }
    }

    void Fail(string why, PixelBuffer target)
    {
        _failed = true;
        target.Clear(0);
        Log.Write($"Visualization {Title}: {why}");
    }

    public void Dispose()
    {
        try
        {
            if (_base != null) ((int*)_base)[8] = 1;
            _input?.Set();
            if (_host != null && !_host.WaitForExit(500)) _host.Kill();
        }
        catch { }
        _host?.Dispose();
        _input?.Dispose();
        if (_base != null) { _view!.SafeMemoryMappedViewHandle.ReleasePointer(); _base = null; }
        _view?.Dispose();
        _map?.Dispose();
    }
}
