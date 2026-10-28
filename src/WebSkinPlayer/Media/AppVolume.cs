using System.Diagnostics;
using NAudio.CoreAudioApi;

namespace WebSkinPlayer.Media;

/// <summary>
/// Volume and mute for the app that owns the media session (e.g. chrome.exe), via the
/// Windows per-application mixer. Falls back to the master endpoint when the app has no
/// audio session yet.
/// </summary>
public sealed class AppVolume : IDisposable
{
    readonly MMDeviceEnumerator _enumerator = new();
    readonly Dictionary<uint, string> _processNames = new();
    MMDevice? _device;
    DateTime _deviceFetched;

    MMDevice? Device
    {
        get
        {
            // Re-resolve periodically so a headset/speaker switch is picked up.
            if (_device == null || DateTime.UtcNow - _deviceFetched > TimeSpan.FromSeconds(3))
            {
                try
                {
                    _device?.Dispose();
                    _device = _enumerator.GetDefaultAudioEndpoint(DataFlow.Render, Role.Multimedia);
                }
                catch { _device = null; }
                _deviceFetched = DateTime.UtcNow;
            }
            return _device;
        }
    }

    List<AudioSessionControl> Sessions(string processName)
    {
        var result = new List<AudioSessionControl>();
        if (string.IsNullOrEmpty(processName)) return result;
        try
        {
            var mgr = Device?.AudioSessionManager;
            if (mgr == null) return result;
            mgr.RefreshSessions();
            var sessions = mgr.Sessions;
            for (int i = 0; i < sessions.Count; i++)
            {
                var s = sessions[i];
                if (string.Equals(NameOf(s.GetProcessID), processName, StringComparison.OrdinalIgnoreCase))
                    result.Add(s);
            }
        }
        catch (Exception ex) { Log.Write("Audio sessions failed: " + ex.Message); }
        return result;
    }

    string NameOf(uint pid)
    {
        if (pid == 0) return "";
        if (_processNames.TryGetValue(pid, out var n)) return n;
        try { n = Process.GetProcessById((int)pid).ProcessName; } catch { n = ""; }
        _processNames[pid] = n;
        return n;
    }

    /// <summary>0..1</summary>
    public float GetVolume(string processName)
    {
        var s = Sessions(processName);
        if (s.Count > 0) return s.Max(x => x.SimpleAudioVolume.Volume);
        try { return Device?.AudioEndpointVolume.MasterVolumeLevelScalar ?? 1f; } catch { return 1f; }
    }

    public void SetVolume(string processName, float value)
    {
        if (Sandbox.Enabled) return;
        value = Math.Clamp(value, 0f, 1f);
        var s = Sessions(processName);
        try
        {
            if (s.Count > 0) foreach (var x in s) x.SimpleAudioVolume.Volume = value;
            else if (Device != null) Device.AudioEndpointVolume.MasterVolumeLevelScalar = value;
        }
        catch (Exception ex) { Log.Write("Set volume failed: " + ex.Message); }
    }

    public bool GetMute(string processName)
    {
        var s = Sessions(processName);
        if (s.Count > 0) return s.All(x => x.SimpleAudioVolume.Mute);
        try { return Device?.AudioEndpointVolume.Mute ?? false; } catch { return false; }
    }

    public void SetMute(string processName, bool mute)
    {
        if (Sandbox.Enabled) return;
        var s = Sessions(processName);
        try
        {
            if (s.Count > 0) foreach (var x in s) x.SimpleAudioVolume.Mute = mute;
            else if (Device != null) Device.AudioEndpointVolume.Mute = mute;
        }
        catch (Exception ex) { Log.Write("Set mute failed: " + ex.Message); }
    }

    public void Dispose()
    {
        _device?.Dispose();
        _enumerator.Dispose();
    }
}
