using System.Runtime.InteropServices;
using Microsoft.Win32;

namespace WebSkinPlayer.VisHost32;

/// <summary>WMP's TimedLevel: 2 channels x 1024 spectrum bytes, 2 x 1024 waveform bytes, play state, time.</summary>
[StructLayout(LayoutKind.Explicit, Size = 4112)]
public unsafe struct TimedLevel
{
    [FieldOffset(0)] public fixed byte Frequency[2048];
    [FieldOffset(2048)] public fixed byte Waveform[2048];
    [FieldOffset(4096)] public int State;     // 0 stopped, 1 paused, 2 playing
    [FieldOffset(4104)] public long TimeStamp;
}

public sealed record EffectInfo(string Clsid, string Name, string Title, string[] Presets, uint Capabilities, bool Effects2);

/// <summary>
/// One WMP visualization plug-in DLL, loaded straight from its file: no regsvr32, no admin rights. Its class
/// ids come from running its own DllRegisterServer against private registry keys, which keeps what it would
/// have written out of the real registry.
/// </summary>
public static unsafe class Plugin
{
    static readonly Guid IID_IClassFactory = new("00000001-0000-0000-C000-000000000046");
    public static readonly Guid IID_IWMPEffects = new("D3984C13-C3CB-48e2-8BE5-5168340B4F35");
    static readonly Guid IID_IWMPEffects2 = new("695386EC-AA3C-4618-A5E1-DD9A8B987632");

    static readonly IntPtr HKEY_CLASSES_ROOT = unchecked((IntPtr)(int)0x80000000);
    static readonly IntPtr HKEY_CURRENT_USER = unchecked((IntPtr)(int)0x80000001);
    static readonly IntPtr HKEY_LOCAL_MACHINE = unchecked((IntPtr)(int)0x80000002);

    /// <summary>Loads the DLL, captures its registration, and describes every visualization it contains.</summary>
    public static (int RegisterHr, List<EffectInfo> Effects) Probe(string dll)
    {
        IntPtr module = Load(dll);
        var (hr, effects, classes) = CaptureRegistration(module);
        // Without an Effects entry, try every class the DLL registered.
        var candidates = effects.Count > 0 ? effects : classes.Select(c => (Clsid: c, Name: "")).ToList();
        var found = new List<EffectInfo>();
        foreach (var (clsid, name) in candidates)
        {
            IntPtr fx = Create(module, Guid.Parse(clsid));
            if (fx == IntPtr.Zero) continue;
            try { found.Add(Describe(fx, clsid, name)); }
            finally { Release(fx); }
        }
        return (hr, found);
    }

    public static IntPtr Load(string dll)
    {
        IntPtr module = LoadLibraryEx(Path.GetFullPath(dll), IntPtr.Zero, 0x00000008 /* LOAD_WITH_ALTERED_SEARCH_PATH */);
        if (module == IntPtr.Zero) throw new InvalidOperationException($"LoadLibrary failed ({Marshal.GetLastWin32Error()})");
        return module;
    }

    /// <summary>Runs DllRegisterServer with HKCR/HKLM/HKCU redirected to a scratch key, then reads back what it wrote.</summary>
    static (int Hr, List<(string Clsid, string Name)> Effects, List<string> Classes) CaptureRegistration(IntPtr module)
    {
        var effects = new List<(string, string)>();
        var classes = new List<string>();
        IntPtr register = GetProcAddress(module, "DllRegisterServer");
        if (register == IntPtr.Zero) return (-1, effects, classes);
        string root = @"Software\WebSkinPlayer\RegistrationCapture\" + Guid.NewGuid().ToString("N");
        int hr;
        using (var scratch = Registry.CurrentUser.CreateSubKey(root))
        using (var hkcr = scratch.CreateSubKey("HKCR"))
        using (var hklm = scratch.CreateSubKey("HKLM"))
        using (var hkcu = scratch.CreateSubKey("HKCU"))
        {
            RegOverridePredefKey(HKEY_CLASSES_ROOT, hkcr.Handle.DangerousGetHandle());
            RegOverridePredefKey(HKEY_LOCAL_MACHINE, hklm.Handle.DangerousGetHandle());
            RegOverridePredefKey(HKEY_CURRENT_USER, hkcu.Handle.DangerousGetHandle());
            try { hr = ((delegate* unmanaged[Stdcall]<int>)register)(); }
            catch { hr = -2; }
            finally
            {
                RegOverridePredefKey(HKEY_CLASSES_ROOT, IntPtr.Zero);
                RegOverridePredefKey(HKEY_LOCAL_MACHINE, IntPtr.Zero);
                RegOverridePredefKey(HKEY_CURRENT_USER, IntPtr.Zero);
            }
            foreach (var hive in new[] { hklm, hkcu })
                using (var fxRoot = hive.OpenSubKey(@"SOFTWARE\Microsoft\MediaPlayer\Objects\Effects"))
                    if (fxRoot != null)
                        foreach (var name in fxRoot.GetSubKeyNames())
                            using (var props = fxRoot.OpenSubKey(name + @"\Properties"))
                                if (props?.GetValue("classid") is string cls && Guid.TryParse(cls, out var g))
                                    effects.Add((g.ToString("B").ToUpperInvariant(), name));
            foreach (var clsidRoot in new[] { hkcr.OpenSubKey("CLSID"), hklm.OpenSubKey(@"SOFTWARE\Classes\CLSID"), hkcu.OpenSubKey(@"SOFTWARE\Classes\CLSID") })
                using (clsidRoot)
                    if (clsidRoot != null)
                        foreach (var c in clsidRoot.GetSubKeyNames())
                            if (Guid.TryParse(c, out var g)) classes.Add(g.ToString("B").ToUpperInvariant());
        }
        try { Registry.CurrentUser.DeleteSubKeyTree(root, false); } catch { }
        return (hr, effects.DistinctBy(e => e.Item1).ToList(), classes.Distinct().ToList());
    }

    /// <summary>Creates the class through the DLL's own class factory and asks for IWMPEffects.</summary>
    public static IntPtr Create(IntPtr module, Guid clsid)
    {
        IntPtr get = GetProcAddress(module, "DllGetClassObject");
        if (get == IntPtr.Zero) return IntPtr.Zero;
        Guid iidFactory = IID_IClassFactory, iidFx = IID_IWMPEffects;
        IntPtr factory, fx = IntPtr.Zero;
        if (((delegate* unmanaged[Stdcall]<Guid*, Guid*, IntPtr*, int>)get)(&clsid, &iidFactory, &factory) < 0 || factory == IntPtr.Zero) return IntPtr.Zero;
        try { ((delegate* unmanaged[Stdcall]<IntPtr, IntPtr, Guid*, IntPtr*, int>)Slot(factory, 3))(factory, IntPtr.Zero, &iidFx, &fx); }
        finally { Release(factory); }
        return fx;
    }

    public static EffectInfo Describe(IntPtr fx, string clsid, string name)
    {
        string title = Bstr(p => ((delegate* unmanaged[Stdcall]<IntPtr, IntPtr, int>)Slot(fx, 6))(fx, p)) ?? name;
        int count = 0;
        ((delegate* unmanaged[Stdcall]<IntPtr, int*, int>)Slot(fx, 8))(fx, &count);
        var presets = new string[Math.Clamp(count, 0, 256)];
        for (int i = 0; i < presets.Length; i++)
        {
            int index = i;
            presets[i] = Bstr(p => ((delegate* unmanaged[Stdcall]<IntPtr, int, IntPtr, int>)Slot(fx, 7))(fx, index, p)) ?? $"Preset {i + 1}";
        }
        uint caps = 0;
        ((delegate* unmanaged[Stdcall]<IntPtr, uint*, int>)Slot(fx, 5))(fx, &caps);
        Guid iid2 = IID_IWMPEffects2;
        IntPtr fx2;
        bool effects2 = ((delegate* unmanaged[Stdcall]<IntPtr, Guid*, IntPtr*, int>)Slot(fx, 0))(fx, &iid2, &fx2) >= 0 && fx2 != IntPtr.Zero;
        if (effects2) Release(fx2);
        return new EffectInfo(clsid, name, title, presets, caps, effects2);
    }

    public static void MediaInfo(IntPtr fx, string title)
    {
        IntPtr b = Marshal.StringToBSTR(title);
        try { ((delegate* unmanaged[Stdcall]<IntPtr, int, int, IntPtr, int>)Slot(fx, 4))(fx, 2, 44100, b); }
        finally { Marshal.FreeBSTR(b); }
    }

    public static int Render(IntPtr fx, TimedLevel* levels, IntPtr hdc, int width, int height)
    {
        var rect = stackalloc int[4] { 0, 0, width, height };
        return ((delegate* unmanaged[Stdcall]<IntPtr, TimedLevel*, IntPtr, int*, int>)Slot(fx, 3))(fx, levels, hdc, rect);
    }

    public static void SetPreset(IntPtr fx, int preset) => ((delegate* unmanaged[Stdcall]<IntPtr, int, int>)Slot(fx, 9))(fx, preset);

    /// <summary>
    /// What WMP 9 and later do with every IWMPEffects2 plug-in before it draws: hand it the player (SetCore) and
    /// the visualization window (Create). Returns the IWMPEffects2 pointer, or zero for an IWMPEffects-only plug-in.
    /// </summary>
    public static IntPtr Attach(IntPtr fx, IntPtr hwnd)
    {
        Guid iid2 = IID_IWMPEffects2;
        IntPtr fx2;
        if (((delegate* unmanaged[Stdcall]<IntPtr, Guid*, IntPtr*, int>)Slot(fx, 0))(fx, &iid2, &fx2) < 0 || fx2 == IntPtr.Zero) return IntPtr.Zero;
        ((delegate* unmanaged[Stdcall]<IntPtr, IntPtr, int>)Slot(fx2, 14))(fx2, StubCore.Instance);
        ((delegate* unmanaged[Stdcall]<IntPtr, IntPtr, int>)Slot(fx2, 15))(fx2, hwnd);
        return fx2;
    }

    /// <summary>IWMPEffects2::Destroy, then the player is taken back, as WMP does when switching visualizations.</summary>
    public static void Detach(IntPtr fx2)
    {
        if (fx2 == IntPtr.Zero) return;
        ((delegate* unmanaged[Stdcall]<IntPtr, int>)Slot(fx2, 16))(fx2);
        ((delegate* unmanaged[Stdcall]<IntPtr, IntPtr, int>)Slot(fx2, 14))(fx2, IntPtr.Zero);
        Release(fx2);
    }

    /// <summary>IWMPEffects2::RenderWindowed: the plug-in draws into the window it was given (or its own child of it).</summary>
    public static int RenderWindowed(IntPtr fx2, TimedLevel* levels) =>
        ((delegate* unmanaged[Stdcall]<IntPtr, TimedLevel*, int, int>)Slot(fx2, 19))(fx2, levels, 1);

    public static void Release(IntPtr unknown) => ((delegate* unmanaged[Stdcall]<IntPtr, uint>)Slot(unknown, 2))(unknown);

    static IntPtr Slot(IntPtr obj, int index) => (*(IntPtr**)obj)[index];

    static string? Bstr(Func<IntPtr, int> call)
    {
        IntPtr b = IntPtr.Zero;
        if (call((IntPtr)(&b)) < 0 || b == IntPtr.Zero) return null;
        try { return Marshal.PtrToStringBSTR(b); }
        finally { Marshal.FreeBSTR(b); }
    }

    [DllImport("kernel32", SetLastError = true, CharSet = CharSet.Unicode)] static extern IntPtr LoadLibraryEx(string file, IntPtr reserved, uint flags);
    [DllImport("kernel32", CharSet = CharSet.Ansi)] static extern IntPtr GetProcAddress(IntPtr module, string name);
    [DllImport("advapi32")] static extern int RegOverridePredefKey(IntPtr key, IntPtr newKey);
}
