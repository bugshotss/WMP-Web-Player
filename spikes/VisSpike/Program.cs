// Spike: can we host the stock WMP visualizations (IWMPEffects) outside WMP?
using System.Drawing;
using System.Drawing.Imaging;
using System.Runtime.InteropServices;

unsafe
{
    var targets = new (string Name, string Dll, Guid Clsid)[]
    {
        ("Alchemy", @"C:\Program Files\Windows Media Player\mpvis.dll", new Guid("0AA02E8D-F851-4CB0-9F64-BBA9BE7A983D")),
        ("Bars", @"C:\Windows\System32\wmp.dll", new Guid("48501FF0-F6A9-11D2-9435-00A0C92A2F2D")),
        ("Battery", @"C:\Windows\System32\wmp.dll", new Guid("8D2A317B-D98B-4efa-89D6-E49F0ACF98A1")),
        ("Bars (mpvis)", @"C:\Program Files\Windows Media Player\mpvis.dll", new Guid("48501FF0-F6A9-11D2-9435-00A0C92A2F2D")),
        ("Battery (mpvis)", @"C:\Program Files\Windows Media Player\mpvis.dll", new Guid("8D2A317B-D98B-4efa-89D6-E49F0ACF98A1")),
    };
    Directory.CreateDirectory("out");
    foreach (var t in targets)
    {
        Console.WriteLine($"=== {t.Name}");
        try
        {
            IntPtr h = Native.LoadLibrary(t.Dll);
            if (h == IntPtr.Zero) { Console.WriteLine($"LoadLibrary failed {Marshal.GetLastWin32Error()}"); continue; }
            var getClass = (delegate* unmanaged[Stdcall]<Guid*, Guid*, IntPtr*, int>)Native.GetProcAddress(h, "DllGetClassObject");
            Guid clsid = t.Clsid, iidFactory = typeof(IClassFactory).GUID;
            IntPtr pFactory;
            int hr = getClass(&clsid, &iidFactory, &pFactory);
            Console.WriteLine($"DllGetClassObject hr=0x{hr:X8}");
            if (hr != 0) continue;
            var factory = (IClassFactory)Marshal.GetObjectForIUnknown(pFactory);
            Guid iidFx = typeof(IWMPEffects).GUID;
            hr = factory.CreateInstance(IntPtr.Zero, ref iidFx, out object obj);
            Console.WriteLine($"CreateInstance hr=0x{hr:X8}");
            if (hr != 0) continue;
            var fx = (IWMPEffects)obj;
            fx.GetTitle(out string title);
            fx.GetCapabilities(out uint caps);
            fx.GetPresetCount(out int count);
            Console.WriteLine($"title='{title}' caps=0x{caps:X} presets={count} effects2={obj is IWMPEffects2}");
            for (int i = 0; i < count; i++) { fx.GetPresetTitle(i, out string pt); Console.WriteLine($"  [{i}] {pt}"); }
            fx.MediaInfo(2, 44100, "Spike");

            // Render a few frames of synthetic audio into a memory DC.
            const int W = 320, H = 240;
            using var bmp = new Bitmap(W, H, PixelFormat.Format32bppRgb);
            var level = new TimedLevel { state = 2 };
            for (int p = 0; p < Math.Min(count, 3); p++)
            {
                fx.SetCurrentPreset(p);
                for (int frame = 0; frame < 40; frame++)
                {
                    for (int i = 0; i < 1024; i++)
                    {
                        double env = 1.0 - i / 1024.0;
                        byte f = (byte)Math.Clamp(255 * env * (0.6 + 0.4 * Math.Sin(i * 0.05 + frame * 0.3)), 0, 255);
                        level.frequency[i] = f; level.frequency[1024 + i] = f;
                        byte w = (byte)(128 + 100 * Math.Sin(i * 0.07 + frame * 0.5) * Math.Sin(i * 0.011));
                        level.waveform[i] = w; level.waveform[1024 + i] = w;
                    }
                    level.timeStamp = frame * 333_333L;
                    using var g = Graphics.FromImage(bmp);
                    IntPtr hdc = g.GetHdc();
                    var rc = new RECT { right = W, bottom = H };
                    hr = fx.Render(&level, hdc, &rc);
                    g.ReleaseHdc(hdc);
                    if (frame == 0 && hr != 0) Console.WriteLine($"  Render hr=0x{hr:X8}");
                }
                bmp.Save(Path.Combine("out", $"{t.Name.Replace(' ', '_')}_{p}.png"), ImageFormat.Png);
            }
            Console.WriteLine("  rendered OK");
        }
        catch (Exception ex) { Console.WriteLine($"  EXCEPTION {ex.GetType().Name}: {ex.Message}"); }
    }
}

static class Native
{
    [DllImport("kernel32", SetLastError = true, CharSet = CharSet.Unicode)] public static extern IntPtr LoadLibrary(string path);
    [DllImport("kernel32", CharSet = CharSet.Ansi)] public static extern IntPtr GetProcAddress(IntPtr h, string name);
}

[StructLayout(LayoutKind.Sequential)]
struct RECT { public int left, top, right, bottom; }

[StructLayout(LayoutKind.Sequential)]
unsafe struct TimedLevel
{
    public fixed byte frequency[2048];
    public fixed byte waveform[2048];
    public int state;
    public long timeStamp;
}

[ComImport, Guid("00000001-0000-0000-C000-000000000046"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IClassFactory
{
    [PreserveSig] int CreateInstance(IntPtr outer, ref Guid iid, [MarshalAs(UnmanagedType.IUnknown)] out object obj);
    [PreserveSig] int LockServer(bool fLock);
}

[ComImport, Guid("D3984C13-C3CB-48e2-8BE5-5168340B4F35"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
unsafe interface IWMPEffects
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
    [PreserveSig] int GoFullscreen(bool full);
    [PreserveSig] int RenderFullScreen(TimedLevel* levels);
}

[ComImport, Guid("695386EC-AA3C-4618-A5E1-DD9A8B987632"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IWMPEffects2 { }
