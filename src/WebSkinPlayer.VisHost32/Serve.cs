using System.Diagnostics;
using System.IO.MemoryMappedFiles;

namespace WebSkinPlayer.VisHost32;

/// <summary>
/// "serve": renders one visualization for the player through shared memory, laid out as in the player's
/// Hosted32Visualization: a header of ints, then WMP's TimedLevel, then the picture (BGRX rows, top down).
/// </summary>
static unsafe class Serve
{
    public const int LevelsOffset = 64, PixelsOffset = 4224, MaxWidth = 1280, MaxHeight = 720;
    // Header ints: 0 magic, 1 width wanted, 2 height wanted, 3 preset wanted, 4 input sequence,
    // 5 output sequence, 6 output width, 7 output height, 8 quit, 9 state (1 running, -1 failed).

    /// <summary>
    /// <paramref name="output"/> is where the plug-in's picture ends up, found when it was added: "dc" (the DC
    /// handed to Render, as WMP 7/8 plug-ins do), "window" (DirectX plug-ins that draw onto the window from
    /// IWMPEffects2::Create whatever DC they're given) or "windowed" (IWMPEffects2::RenderWindowed).
    /// </summary>
    public static int Run(string dll, Guid clsid, string mapName, int parentPid, string output)
    {
        using var map = MemoryMappedFile.OpenExisting(mapName);
        using var view = map.CreateViewAccessor();
        byte* basePtr = null;
        view.SafeMemoryMappedViewHandle.AcquirePointer(ref basePtr);
        try
        {
            int* header = (int*)basePtr;
            var levels = (TimedLevel*)(basePtr + LevelsOffset);
            var pixels = basePtr + PixelsOffset;
            using var input = EventWaitHandle.OpenExisting(mapName + ".in");
            using var parent = Process.GetProcessById(parentPid);

            IntPtr fx = Plugin.Create(Plugin.Load(dll), clsid);
            if (fx == IntPtr.Zero) { header[9] = -1; return 3; }
            int w = Math.Clamp(header[1], 16, MaxWidth), h = Math.Clamp(header[2], 16, MaxHeight);
            using var window = new HostWindow(w, h);
            IntPtr fx2 = Plugin.Attach(fx, window.Handle);
            bool windowed = output == "windowed" && fx2 != IntPtr.Zero;
            bool fromWindow = windowed || output == "window";
            Plugin.MediaInfo(fx, "WebSkin Player");
            DibSurface? surface = null;
            int preset = -1;
            header[9] = 1;
            try
            {
                while (Volatile.Read(ref header[8]) == 0 && !parent.HasExited)
                {
                    // Draw for each new audio frame, and at least 10 times a second while paused.
                    input.WaitOne(100);
                    HostWindow.Pump();
                    w = Math.Clamp(header[1], 16, MaxWidth);
                    h = Math.Clamp(header[2], 16, MaxHeight);
                    if (surface == null || surface.Width != w || surface.Height != h)
                    {
                        surface?.Dispose();
                        surface = new DibSurface(w, h);
                        window.Resize(w, h);
                    }
                    if (header[3] != preset) { preset = header[3]; Plugin.SetPreset(fx, preset); }
                    if (windowed) Plugin.RenderWindowed(fx2, levels);
                    else Plugin.Render(fx, levels, surface.Dc, w, h);
                    if (fromWindow) window.CaptureTo(surface);
                    surface.Flush();
                    Buffer.MemoryCopy(surface.Bits, pixels, (long)MaxWidth * MaxHeight * 4, (long)w * h * 4);
                    header[6] = w;
                    header[7] = h;
                    Interlocked.Increment(ref header[5]);
                }
            }
            finally
            {
                surface?.Dispose();
                Plugin.Detach(fx2);
                Plugin.Release(fx);
            }
            return 0;
        }
        finally { view.SafeMemoryMappedViewHandle.ReleasePointer(); }
    }
}
