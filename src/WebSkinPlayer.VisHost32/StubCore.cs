using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;

namespace WebSkinPlayer.VisHost32;

/// <summary>
/// A stand-in for WMP's player object (IWMPCore3), handed to IWMPEffects2 plug-ins through SetCore. It reports an
/// open, playing track and declines everything else (no media object, library, playlist or settings), which plug-ins
/// already had to cope with in WMP between tracks. Every slot has its real argument count: on x86 the callee pops
/// the arguments, so a wrong count corrupts the caller's stack.
/// </summary>
static unsafe class StubCore
{
    const int S_OK = 0, E_NOINTERFACE = unchecked((int)0x80004002), E_NOTIMPL = unchecked((int)0x80004001), E_POINTER = unchecked((int)0x80004003);

    static readonly Guid[] Interfaces =
    {
        new("00000000-0000-0000-C000-000000000046"), // IUnknown
        new("00020400-0000-0000-C000-000000000046"), // IDispatch
        new("D84CCA99-CCE2-11D2-9ECC-0000F8085981"), // IWMPCore
        new("BC17E5B7-7561-4C18-BB90-17D485775659"), // IWMPCore2
        new("7587C667-628F-499F-88E7-6A6F4E888464"), // IWMPCore3
    };

    static IntPtr _instance;

    /// <summary>One shared object for the life of the process; AddRef and Release don't free it.</summary>
    public static IntPtr Instance => _instance != IntPtr.Zero ? _instance : (_instance = Build());

    static IntPtr Build()
    {
        var v = (IntPtr*)NativeMemory.AllocZeroed(31, (nuint)sizeof(IntPtr));
        v[0] = (IntPtr)(delegate* unmanaged[Stdcall]<IntPtr, Guid*, IntPtr*, int>)&QueryInterface;
        v[1] = (IntPtr)(delegate* unmanaged[Stdcall]<IntPtr, uint>)&AddRef;
        v[2] = (IntPtr)(delegate* unmanaged[Stdcall]<IntPtr, uint>)&AddRef;
        v[3] = (IntPtr)(delegate* unmanaged[Stdcall]<IntPtr, uint*, int>)&GetTypeInfoCount;
        v[4] = (IntPtr)(delegate* unmanaged[Stdcall]<IntPtr, uint, uint, IntPtr*, int>)&GetTypeInfo;
        v[5] = (IntPtr)(delegate* unmanaged[Stdcall]<IntPtr, Guid*, IntPtr, uint, uint, int*, int>)&GetIDsOfNames;
        v[6] = (IntPtr)(delegate* unmanaged[Stdcall]<IntPtr, int, Guid*, uint, ushort, IntPtr, IntPtr, IntPtr, IntPtr, int>)&Invoke;
        v[7] = (IntPtr)(delegate* unmanaged[Stdcall]<IntPtr, int>)&Close;
        var none = (IntPtr)(delegate* unmanaged[Stdcall]<IntPtr, IntPtr*, int>)&NoObject;
        var ignore = (IntPtr)(delegate* unmanaged[Stdcall]<IntPtr, IntPtr, int>)&Ignore;
        var empty = (IntPtr)(delegate* unmanaged[Stdcall]<IntPtr, IntPtr*, int>)&EmptyString;
        v[8] = empty;                                                                            // get_URL
        v[9] = ignore;                                                                           // put_URL
        v[10] = (IntPtr)(delegate* unmanaged[Stdcall]<IntPtr, int*, int>)&OpenState;           // get_openState
        v[11] = (IntPtr)(delegate* unmanaged[Stdcall]<IntPtr, int*, int>)&PlayState;           // get_playState
        for (int i = 12; i <= 28; i++) v[i] = none;   // controls, settings, currentMedia, collections, network, playlist, cdroms, captions, error, dvd
        v[15] = ignore;                                                                          // put_currentMedia
        v[18] = (IntPtr)(delegate* unmanaged[Stdcall]<IntPtr, IntPtr*, int>)&VersionInfo;      // get_versionInfo
        v[19] = ignore;                                                                          // launchURL
        v[22] = ignore;                                                                          // put_currentPlaylist
        v[25] = (IntPtr)(delegate* unmanaged[Stdcall]<IntPtr, short*, int>)&IsOnline;          // get_isOnline
        v[27] = empty;                                                                           // get_status
        v[29] = (IntPtr)(delegate* unmanaged[Stdcall]<IntPtr, IntPtr, IntPtr, IntPtr*, int>)&NewPlaylist;
        v[30] = (IntPtr)(delegate* unmanaged[Stdcall]<IntPtr, IntPtr, IntPtr*, int>)&NewMedia;
        var obj = (IntPtr*)NativeMemory.Alloc((nuint)sizeof(IntPtr));
        *obj = (IntPtr)v;
        return (IntPtr)obj;
    }

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvStdcall) })]
    static int QueryInterface(IntPtr self, Guid* iid, IntPtr* result)
    {
        if (result == null) return E_POINTER;
        foreach (var known in Interfaces)
            if (*iid == known) { *result = self; return S_OK; }
        *result = IntPtr.Zero;
        return E_NOINTERFACE;
    }

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvStdcall) })]
    static uint AddRef(IntPtr self) => 1;

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvStdcall) })]
    static int GetTypeInfoCount(IntPtr self, uint* count) { if (count != null) *count = 0; return S_OK; }

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvStdcall) })]
    static int GetTypeInfo(IntPtr self, uint index, uint lcid, IntPtr* info) { if (info != null) *info = IntPtr.Zero; return E_NOTIMPL; }

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvStdcall) })]
    static int GetIDsOfNames(IntPtr self, Guid* iid, IntPtr names, uint count, uint lcid, int* ids) => E_NOTIMPL;

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvStdcall) })]
    static int Invoke(IntPtr self, int id, Guid* iid, uint lcid, ushort flags, IntPtr parameters, IntPtr result, IntPtr exception, IntPtr argError) => E_NOTIMPL;

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvStdcall) })]
    static int Close(IntPtr self) => S_OK;

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvStdcall) })]
    static int NoObject(IntPtr self, IntPtr* result) { if (result != null) *result = IntPtr.Zero; return E_NOTIMPL; }

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvStdcall) })]
    static int Ignore(IntPtr self, IntPtr value) => S_OK;

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvStdcall) })]
    static int EmptyString(IntPtr self, IntPtr* result) { if (result == null) return E_POINTER; *result = Marshal.StringToBSTR(""); return S_OK; }

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvStdcall) })]
    static int VersionInfo(IntPtr self, IntPtr* result) { if (result == null) return E_POINTER; *result = Marshal.StringToBSTR("12.0.19041.1"); return S_OK; }

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvStdcall) })]
    static int OpenState(IntPtr self, int* state) { if (state == null) return E_POINTER; *state = 13; return S_OK; }  // wmposMediaOpen

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvStdcall) })]
    static int PlayState(IntPtr self, int* state) { if (state == null) return E_POINTER; *state = 3; return S_OK; }  // wmppsPlaying

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvStdcall) })]
    static int IsOnline(IntPtr self, short* online) { if (online == null) return E_POINTER; *online = 0; return S_OK; }

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvStdcall) })]
    static int NewPlaylist(IntPtr self, IntPtr name, IntPtr url, IntPtr* result) { if (result != null) *result = IntPtr.Zero; return E_NOTIMPL; }

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvStdcall) })]
    static int NewMedia(IntPtr self, IntPtr url, IntPtr* result) { if (result != null) *result = IntPtr.Zero; return E_NOTIMPL; }
}
