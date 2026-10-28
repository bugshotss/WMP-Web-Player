using System.Runtime.InteropServices;
using System.Text.Json;

namespace WebSkinPlayer.VisHost32;

/// <summary>
/// WebSkinPlayer.VisHost32 probe &lt;dll&gt;                          describe the visualizations in a plug-in DLL (JSON)
/// WebSkinPlayer.VisHost32 snapshot &lt;dll&gt; &lt;clsid&gt; &lt;out.bmp&gt;   render a few seconds of test audio and save the result
/// </summary>
static unsafe class Program
{
    [STAThread]
    static int Main(string[] args)
    {
        CoInitializeEx(IntPtr.Zero, 0x2 /* COINIT_APARTMENTTHREADED */);
        try
        {
            switch (args.FirstOrDefault()?.ToLowerInvariant())
            {
                case "probe" when args.Length >= 2:
                {
                    var (hr, effects) = Plugin.Probe(args[1]);
                    WriteJson(w =>
                    {
                        w.WriteString("dll", Path.GetFullPath(args[1]));
                        w.WriteNumber("registerHr", hr);
                        w.WriteStartArray("effects");
                        foreach (var e in effects)
                        {
                            w.WriteStartObject();
                            w.WriteString("clsid", e.Clsid);
                            w.WriteString("name", e.Name);
                            w.WriteString("title", e.Title);
                            w.WriteStartArray("presets");
                            foreach (var p in e.Presets) w.WriteStringValue(p);
                            w.WriteEndArray();
                            w.WriteNumber("capabilities", e.Capabilities);
                            w.WriteBoolean("effects2", e.Effects2);
                            w.WriteEndObject();
                        }
                        w.WriteEndArray();
                    });
                    return effects.Count > 0 ? 0 : 2;
                }
                case "snapshot" when args.Length >= 4:
                    return Snapshot(args[1], Guid.Parse(args[2]), args[3], args.Length > 4 ? int.Parse(args[4]) : 0);
                case "serve" when args.Length >= 5:
                    return Serve.Run(args[1], Guid.Parse(args[2]), args[3], int.Parse(args[4]));
                default:
                    Console.Error.WriteLine("usage: probe <dll> | snapshot <dll> <clsid> <out.bmp> [preset] | serve <dll> <clsid> <map> <parentPid>");
                    return 64;
            }
        }
        catch (Exception ex)
        {
            Console.Error.WriteLine(ex.GetType().Name + ": " + ex.Message);
            return 1;
        }
    }

    static int Snapshot(string dll, Guid clsid, string outFile, int preset)
    {
        const int W = 640, H = 360;
        IntPtr module = Plugin.Load(dll);
        IntPtr fx = Plugin.Create(module, clsid);
        if (fx == IntPtr.Zero) { Console.Error.WriteLine("could not create the visualization"); return 3; }
        using var surface = new DibSurface(W, H);
        Plugin.MediaInfo(fx, "WebSkin Player");
        if (preset > 0) Plugin.SetPreset(fx, preset);
        var levels = new TimedLevel();
        int failures = 0;
        for (int frame = 0; frame < 150; frame++)
        {
            TestSignal.Fill(&levels, frame);
            if (Plugin.Render(fx, &levels, surface.Dc, W, H) < 0) failures++;
            Thread.Sleep(15);
        }
        surface.Flush();
        double lit = surface.LitShare();
        surface.SaveBmp(outFile);
        Plugin.Release(fx);
        WriteJson(w => { w.WriteString("clsid", clsid.ToString("B")); w.WriteNumber("lit", lit); w.WriteNumber("failures", failures); });
        return 0;
    }

    /// <summary>One JSON object on stdout (Utf8JsonWriter keeps the trimmed build free of reflection).</summary>
    static void WriteJson(Action<Utf8JsonWriter> body)
    {
        using var stdout = Console.OpenStandardOutput();
        using var w = new Utf8JsonWriter(stdout);
        w.WriteStartObject();
        body(w);
        w.WriteEndObject();
    }

    [DllImport("ole32")] static extern int CoInitializeEx(IntPtr reserved, uint flags);
}

/// <summary>A 32-bit top-down DIB selected into a memory DC: what WMP handed plug-ins to draw into.</summary>
public sealed unsafe class DibSurface : IDisposable
{
    public int Width { get; }
    public int Height { get; }
    public IntPtr Dc { get; }
    public uint* Bits { get; }
    readonly IntPtr _bitmap, _old;

    public DibSurface(int width, int height)
    {
        Width = width; Height = height;
        Dc = CreateCompatibleDC(IntPtr.Zero);
        var info = new BITMAPINFOHEADER { biSize = 40, biWidth = width, biHeight = -height, biPlanes = 1, biBitCount = 32 };
        _bitmap = CreateDIBSection(Dc, ref info, 0, out IntPtr bits, IntPtr.Zero, 0);
        Bits = (uint*)bits;
        _old = SelectObject(Dc, _bitmap);
    }

    public void Flush() => GdiFlush();

    /// <summary>Share of pixels that aren't (near) black: 0 means the plug-in drew nothing.</summary>
    public double LitShare()
    {
        long lit = 0, n = (long)Width * Height;
        for (long i = 0; i < n; i++) { uint c = Bits[i]; if (((c >> 16) & 255) + ((c >> 8) & 255) + (c & 255) > 30) lit++; }
        return Math.Round(lit / (double)n, 3);
    }

    public void SaveBmp(string path)
    {
        using var f = File.Create(path);
        using var w = new BinaryWriter(f);
        int size = Width * Height * 4;
        w.Write((ushort)0x4D42); w.Write(54 + size); w.Write(0); w.Write(54);
        w.Write(40); w.Write(Width); w.Write(-Height); w.Write((ushort)1); w.Write((ushort)32); w.Write(0); w.Write(size); w.Write(2835); w.Write(2835); w.Write(0); w.Write(0);
        w.Write(new ReadOnlySpan<byte>(Bits, size));
    }

    public void Dispose()
    {
        SelectObject(Dc, _old);
        DeleteObject(_bitmap);
        DeleteDC(Dc);
    }

    [StructLayout(LayoutKind.Sequential)]
    struct BITMAPINFOHEADER { public int biSize, biWidth, biHeight; public short biPlanes, biBitCount; public int biCompression, biSizeImage, biXPelsPerMeter, biYPelsPerMeter, biClrUsed, biClrImportant; }

    [DllImport("gdi32")] static extern IntPtr CreateCompatibleDC(IntPtr dc);
    [DllImport("gdi32")] static extern IntPtr CreateDIBSection(IntPtr dc, ref BITMAPINFOHEADER info, uint usage, out IntPtr bits, IntPtr section, uint offset);
    [DllImport("gdi32")] static extern IntPtr SelectObject(IntPtr dc, IntPtr obj);
    [DllImport("gdi32")] static extern bool DeleteObject(IntPtr obj);
    [DllImport("gdi32")] static extern bool DeleteDC(IntPtr dc);
    [DllImport("gdi32")] static extern bool GdiFlush();
}

/// <summary>A few seconds of made-up music: a moving chord in the waveform and a pulsing spectrum.</summary>
public static unsafe class TestSignal
{
    public static void Fill(TimedLevel* l, int frame)
    {
        double t = frame / 30.0, beat = 0.5 + 0.5 * Math.Sin(t * Math.PI * 4);
        for (int ch = 0; ch < 2; ch++)
            for (int i = 0; i < 1024; i++)
            {
                double s = Math.Sin(i * 0.05 + t * 7 + ch) * 0.5 + Math.Sin(i * 0.013 + t * 3) * 0.3 * beat;
                l->Waveform[ch * 1024 + i] = (byte)Math.Clamp(128 + s * 110, 0, 255);
                double spectrum = 220 * Math.Exp(-i / 160.0) * (0.55 + 0.45 * Math.Sin(t * 5 + i * 0.03)) * (0.6 + 0.4 * beat);
                l->Frequency[ch * 1024 + i] = (byte)Math.Clamp(spectrum, 0, 255);
            }
        l->State = 2;
        l->TimeStamp = frame * 333_333L;
    }
}
