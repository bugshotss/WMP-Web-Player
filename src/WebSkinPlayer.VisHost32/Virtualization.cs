using System.Runtime.InteropServices;

namespace WebSkinPlayer.VisHost32;

/// <summary>
/// Turns on Windows' UAC virtualization for the helper: the compatibility mode Windows gives XP-era programs, where
/// writes to HKLM\Software and Program Files go to a per-user VirtualStore instead of failing. Plug-ins from that era
/// open machine keys with write access; WMP 10's own promotional visualizations (and others built on the same
/// engine) open WMP's Active Setup key that way to read its version, and when the open is refused they show only
/// "requires Windows Media Player 10 or above". Nothing outside the VirtualStore is changed.
/// </summary>
static class Virtualization
{
    public static bool Enable()
    {
        if (!OpenProcessToken(GetCurrentProcess(), 0x0080 /* TOKEN_ADJUST_DEFAULT */ | 0x0008 /* TOKEN_QUERY */, out IntPtr token)) return false;
        try
        {
            int on = 1;
            return SetTokenInformation(token, 24 /* TokenVirtualizationEnabled */, ref on, sizeof(int));
        }
        finally { CloseHandle(token); }
    }

    [DllImport("kernel32")] static extern IntPtr GetCurrentProcess();
    [DllImport("kernel32")] static extern bool CloseHandle(IntPtr handle);
    [DllImport("advapi32", SetLastError = true)] static extern bool OpenProcessToken(IntPtr process, uint access, out IntPtr token);
    [DllImport("advapi32", SetLastError = true)] static extern bool SetTokenInformation(IntPtr token, int infoClass, ref int info, int length);
}
