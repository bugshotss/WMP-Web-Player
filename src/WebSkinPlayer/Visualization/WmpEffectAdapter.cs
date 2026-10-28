using System.Runtime.InteropServices;
using Microsoft.Win32;
using WebSkinPlayer.Audio;

namespace WebSkinPlayer.Visualization;

/// <summary>
/// Hosts a real Windows Media Player visualization (IWMPEffects COM plug-in such as Alchemy)
/// and feeds it loopback audio in WMP's TimedLevel format.
/// </summary>
public sealed unsafe class WmpEffectAdapter : IVisualization
{
    readonly IWMPEffects _fx;
    readonly object _com;
    readonly List<string> _presets = new();
    TimedLevel* _level;
    int _preset;
    int _w, _h;

    public string Id { get; }
    public string Title { get; }
    public IReadOnlyList<string> Presets => _presets;

    WmpEffectAdapter(string id, object com)
    {
        Id = id;
        _com = com;
        _fx = (IWMPEffects)com;
        _fx.GetTitle(out string title);
        Title = string.IsNullOrWhiteSpace(title) ? id : title;
        if (_fx.GetPresetCount(out int n) == 0)
            for (int i = 0; i < n; i++)
                _presets.Add(_fx.GetPresetTitle(i, out string pt) == 0 ? pt : $"Preset {i + 1}");
        if (_presets.Count == 0) _presets.Add(Title);
        _fx.MediaInfo(2, 48000, "");
        _level = (TimedLevel*)NativeMemory.AllocZeroed((nuint)sizeof(TimedLevel));
    }

    public static WmpEffectAdapter? TryCreate(string id, Guid clsid)
    {
        try
        {
            var type = Type.GetTypeFromCLSID(clsid, throwOnError: false);
            if (type == null) return null;
            object com = Activator.CreateInstance(type)!;
            if (com is not IWMPEffects) { Marshal.ReleaseComObject(com); return null; }
            return new WmpEffectAdapter(id, com);
        }
        catch (Exception ex)
        {
            Log.Write($"Hosted visualization {id} failed: {ex.Message}");
            return null;
        }
    }

    public int CurrentPreset
    {
        get => _preset;
        set
        {
            _preset = Math.Clamp(value, 0, Math.Max(0, _presets.Count - 1));
            try { _fx.SetCurrentPreset(_preset); } catch { }
        }
    }

    public void Render(VisFrame frame, PixelBuffer target, VisContext context)
    {
        if (_w != target.Width || _h != target.Height)
        {
            _w = target.Width;
            _h = target.Height;
        }
        fixed (byte* f = frame.Frequency, w = frame.Waveform)
        {
            Buffer.MemoryCopy(f, _level->frequency, 2048, 2048);
            Buffer.MemoryCopy(w, _level->waveform, 2048, 2048);
        }
        _level->state = frame.Playing ? 2 : 0;
        _level->timeStamp = frame.TimeStamp;
        var rc = new RECT { right = target.Width, bottom = target.Height };
        try { _fx.Render(_level, target.Hdc, &rc); }
        catch (Exception ex) { Log.Write($"{Id} render failed: {ex.Message}"); }
        PixelBuffer.Flush();
    }

    public void Dispose()
    {
        if (_level != null) { NativeMemory.Free(_level); _level = null; }
        try { Marshal.ReleaseComObject(_com); } catch { }
    }

    /// <summary>Visualizations registered with WMP that can be created in-process (64-bit).</summary>
    public static IEnumerable<(string Id, Guid ClassId)> EnumerateInstalled()
    {
        using var root = Registry.LocalMachine.OpenSubKey(@"SOFTWARE\Microsoft\MediaPlayer\Objects\Effects");
        if (root == null) yield break;
        foreach (var name in root.GetSubKeyNames())
        {
            using var props = root.OpenSubKey(name + @"\Properties");
            if (props?.GetValue("classid") is not string cls || !Guid.TryParse(cls, out var clsid)) continue;
            using var server = Registry.ClassesRoot.OpenSubKey($@"CLSID\{{{clsid}}}\InprocServer32");
            if (server == null) continue;
            yield return (name, clsid);
        }
    }

    [StructLayout(LayoutKind.Sequential)]
    struct RECT { public int left, top, right, bottom; }

    [StructLayout(LayoutKind.Sequential)]
    struct TimedLevel
    {
        public fixed byte frequency[2048];
        public fixed byte waveform[2048];
        public int state;
        public long timeStamp;
    }

    [ComImport, Guid("D3984C13-C3CB-48e2-8BE5-5168340B4F35"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    interface IWMPEffects
    {
        [PreserveSig] int Render(TimedLevel* levels, IntPtr hdc, RECT* rc);
        [PreserveSig] int MediaInfo(int channelCount, int sampleRate, [MarshalAs(UnmanagedType.BStr)] string title);
        [PreserveSig] int GetCapabilities(out uint caps);
        [PreserveSig] int GetTitle([MarshalAs(UnmanagedType.BStr)] out string title);
        [PreserveSig] int GetPresetTitle(int preset, [MarshalAs(UnmanagedType.BStr)] out string title);
        [PreserveSig] int GetPresetCount(out int count);
        [PreserveSig] int SetCurrentPreset(int preset);
        [PreserveSig] int GetCurrentPreset(out int preset);
        [PreserveSig] int DisplayPropertyPage(IntPtr hwndOwner);
        [PreserveSig] int GoFullscreen([MarshalAs(UnmanagedType.Bool)] bool full);
        [PreserveSig] int RenderFullScreen(TimedLevel* levels);
    }
}
