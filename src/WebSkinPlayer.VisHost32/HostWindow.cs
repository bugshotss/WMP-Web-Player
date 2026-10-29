using System.Runtime.InteropServices;

namespace WebSkinPlayer.VisHost32;

/// <summary>
/// The window WMP would have given a visualization (IWMPEffects2::Create). It sits on screen but cloaked, so DWM
/// still composes whatever the plug-in draws into it, GDI or DirectX, and PrintWindow can copy that out for the
/// player. Nobody sees it and it takes no input.
/// </summary>
sealed unsafe class HostWindow : IDisposable
{
    const string ClassName = "WebSkinPlayer.VisHost32.Window";
    static bool _registered;

    public IntPtr Handle { get; }
    public int Width { get; private set; }
    public int Height { get; private set; }

    public HostWindow(int width, int height)
    {
        if (!_registered)
        {
            var wc = new WNDCLASSEX
            {
                cbSize = Marshal.SizeOf<WNDCLASSEX>(),
                lpfnWndProc = GetProcAddress(GetModuleHandle("user32"), "DefWindowProcW"),
                hInstance = GetModuleHandle(null),
                // No background brush: erasing on WM_PAINT would paint black over what DirectX presented.
                lpszClassName = ClassName,
            };
            RegisterClassEx(ref wc);
            _registered = true;
        }
        // Cloaked on screen rather than moved off screen or kept behind other windows: in those two places a
        // Direct3D present never reaches what PrintWindow copies.
        Handle = CreateWindowEx(0x80 /* WS_EX_TOOLWINDOW */ | 0x08000000 /* WS_EX_NOACTIVATE */, ClassName, "WebSkin Player visualization",
                                0x80000000 /* WS_POPUP */ | 0x02000000 /* WS_CLIPCHILDREN */, 0, 0, width, height, IntPtr.Zero, IntPtr.Zero, GetModuleHandle(null), IntPtr.Zero);
        if (Handle == IntPtr.Zero) throw new InvalidOperationException($"CreateWindow failed ({Marshal.GetLastWin32Error()})");
        int cloak = 1;
        DwmSetWindowAttribute(Handle, 13 /* DWMWA_CLOAK */, &cloak, sizeof(int));
        ShowWindow(Handle, 4 /* SW_SHOWNOACTIVATE */);
        Width = width;
        Height = height;
    }

    public void Resize(int width, int height)
    {
        if (width == Width && height == Height) return;
        SetWindowPos(Handle, IntPtr.Zero, 0, 0, width, height, 0x0002 /* SWP_NOMOVE */ | 0x0004 /* SWP_NOZORDER */ | 0x0010 /* SWP_NOACTIVATE */);
        Width = width;
        Height = height;
    }

    /// <summary>The plug-in's own window (if it made one) and timers run on this thread, so their messages are handled here.</summary>
    public static void Pump()
    {
        MSG msg;
        while (PeekMessage(&msg, IntPtr.Zero, 0, 0, 1 /* PM_REMOVE */))
        {
            TranslateMessage(&msg);
            DispatchMessage(&msg);
        }
    }

    /// <summary>Copies what the window shows, child windows and DirectX content included.</summary>
    public bool CaptureTo(DibSurface surface) => PrintWindow(Handle, surface.Dc, 0x1 /* PW_CLIENTONLY */ | 0x2 /* PW_RENDERFULLCONTENT */);

    public void Dispose() => DestroyWindow(Handle);

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    struct WNDCLASSEX
    {
        public int cbSize;
        public uint style;
        public IntPtr lpfnWndProc;
        public int cbClsExtra, cbWndExtra;
        public IntPtr hInstance, hIcon, hCursor, hbrBackground;
        public string? lpszMenuName;
        public string lpszClassName;
        public IntPtr hIconSm;
    }

    [StructLayout(LayoutKind.Sequential)]
    struct MSG { public IntPtr hwnd; public uint message; public IntPtr wParam, lParam; public uint time; public int x, y; }

    [DllImport("user32", CharSet = CharSet.Unicode, SetLastError = true)] static extern ushort RegisterClassEx(ref WNDCLASSEX wc);
    [DllImport("user32", CharSet = CharSet.Unicode, SetLastError = true)]
    static extern IntPtr CreateWindowEx(uint exStyle, string className, string title, uint style, int x, int y, int w, int h, IntPtr parent, IntPtr menu, IntPtr instance, IntPtr param);
    [DllImport("user32")] static extern bool ShowWindow(IntPtr hwnd, int cmd);
    [DllImport("user32")] static extern bool SetWindowPos(IntPtr hwnd, IntPtr after, int x, int y, int w, int h, uint flags);
    [DllImport("user32")] static extern bool DestroyWindow(IntPtr hwnd);
    [DllImport("user32")] static extern bool PeekMessage(MSG* msg, IntPtr hwnd, uint min, uint max, uint remove);
    [DllImport("user32")] static extern bool TranslateMessage(MSG* msg);
    [DllImport("user32")] static extern IntPtr DispatchMessage(MSG* msg);
    [DllImport("user32")] static extern bool PrintWindow(IntPtr hwnd, IntPtr dc, uint flags);
    [DllImport("dwmapi")] static extern int DwmSetWindowAttribute(IntPtr hwnd, int attribute, void* value, int size);
    [DllImport("kernel32", CharSet = CharSet.Unicode)] static extern IntPtr GetModuleHandle(string? name);
    [DllImport("kernel32", CharSet = CharSet.Ansi)] static extern IntPtr GetProcAddress(IntPtr module, string name);
}
