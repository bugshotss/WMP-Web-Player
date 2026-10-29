using System.Windows.Threading;

namespace WebSkinPlayer.Media;

/// <summary>WMP numeric constants (wmpcore.h) that skins compare against.</summary>
public static class WmpState
{
    public const int psUndefined = 0, psStopped = 1, psPaused = 2, psPlaying = 3, psScanForward = 4, psScanReverse = 5,
        psBuffering = 6, psWaiting = 7, psMediaEnded = 8, psTransitioning = 9, psReady = 10, psReconnecting = 11;
    public const int osUndefined = 0, osPlaylistChanging = 1, osPlaylistOpenNoMedia = 6, osMediaChanging = 8, osMediaOpening = 12, osMediaOpen = 13;
}

/// <summary>
/// The single shared "player" all skins drive: translates the browser's media session into the
/// state WMP skins expect (playState, openState, currentPosition, volume 0-100, ...).
/// </summary>
public sealed class PlayerModel
{
    readonly MediaBridge _media;
    readonly AppVolume _volume;
    readonly DispatcherTimer _timer;
    int _volumeCache = 50;
    bool _muteCache;
    int _balance;
    DateTime _lastVolumePoll;

    public int PlayState { get; private set; } = WmpState.psUndefined;
    public int OpenState { get; private set; } = WmpState.osUndefined;
    public string Status { get; private set; } = "";
    public MediaSnapshot Media => _media.Current;
    public double Position { get; private set; }
    public double Duration => _media.Current.Duration.TotalSeconds;

    /// <summary>(object, property) changed, e.g. ("controls", "currentPosition"). Raised on the UI thread.</summary>
    public event Action<string, string>? PropertyChanged;
    /// <summary>Player events, e.g. ("PlayStateChange", [newState]).</summary>
    public event Action<string, object?[]>? PlayerEvent;

    public PlayerModel(MediaBridge media, AppVolume volume)
    {
        _media = media;
        _volume = volume;
        _media.Changed += OnMediaChanged;
        _media.TrackChanged += OnTrackChanged;
        _timer = new DispatcherTimer(DispatcherPriority.Background) { Interval = TimeSpan.FromMilliseconds(250) };
        _timer.Tick += (_, _) => Poll();
        _timer.Start();
        OnTrackChanged();
        OnMediaChanged();
        PollVolume(force: true);
    }

    string ProcessName => MediaBridge.ProcessNameForApp(_media.Current.SourceApp);

    void Raise(string obj, string prop) => PropertyChanged?.Invoke(obj, prop);

    void OnTrackChanged()
    {
        var m = _media.Current;
        int newOpen = m.HasSession && m.Title.Length > 0 ? WmpState.osMediaOpen : (m.HasSession ? WmpState.osPlaylistOpenNoMedia : WmpState.osUndefined);
        // Mimic WMP's open sequence so skins that refresh metadata on osMediaOpen do so.
        if (m.HasSession) SetOpenState(WmpState.osMediaChanging);
        SetOpenState(newOpen);
        Raise("currentMedia", "name");
        Raise("currentMedia", "duration");
        Raise("currentMedia", "durationString");
        Raise("player", "currentMedia");
        PlayerEvent?.Invoke("MediaChange", new object?[] { null });
        PlayerEvent?.Invoke("CurrentItemChange", new object?[] { null });
    }

    void SetOpenState(int s)
    {
        if (OpenState == s) return;
        OpenState = s;
        Raise("player", "openState");
        PlayerEvent?.Invoke("OpenStateChange", new object?[] { s });
    }

    /// <summary>When the player's own pause/stop was last pressed: that pause is shown at once.</summary>
    DateTime _commandedAt;
    /// <summary>When the player's own next/previous was last pressed: the stop that follows is a song change.</summary>
    DateTime _skippedAt;
    // A skip made outside the player (media keys, the Windows media flyout) pauses while the next song loads and only
    // then shows its title, so a plain pause is held long enough for that; a song change can take several seconds
    // (Chrome has been seen without a session for 6 s while SoundCloud loaded the next song).
    static readonly TimeSpan PauseHold = TimeSpan.FromMilliseconds(1500), SongChangeHold = TimeSpan.FromSeconds(15);
    DispatcherTimer? _gapTimer;
    DateTime _gapSince;
    bool _gapIsSongChange;
    string _playingTitle = "";

    void OnMediaChanged()
    {
        var m = _media.Current;
        int ps = m.Status switch
        {
            MediaStatus.Playing => WmpState.psPlaying,
            MediaStatus.Paused => WmpState.psPaused,
            MediaStatus.Stopped => WmpState.psStopped,
            MediaStatus.Changing => WmpState.psTransitioning,
            _ => m.HasSession ? WmpState.psReady : WmpState.psUndefined,
        };
        // Web players stop for a moment between songs where WMP went straight on: SoundCloud reports Paused for
        // ~0.2 s, then Playing with the next title, and at some song changes and skips Chrome drops its media session
        // for up to a few seconds and brings it back. Skins that react to every play state change would treat each of
        // those as a pause and resume (Kenwood drops back from its visualizer to the song info), so a stop in mid-play
        // that the player didn't ask for is held and forgotten if playback carries on: briefly for a plain pause, and
        // longer when it looks like a song change (the session gone, a new title, a skip, the song at its end).
        var now = DateTime.UtcNow;
        bool held = false;
        if (PlayState == WmpState.psPlaying && ps != WmpState.psPlaying && now - _commandedAt > TimeSpan.FromSeconds(2))
        {
            if (_gapSince == default) (_gapSince, _gapIsSongChange) = (now, Duration > 0 && Duration - Position < 4);
            _gapIsSongChange |= !m.HasSession || m.Status != MediaStatus.Paused || m.Title != _playingTitle || now - _skippedAt < SongChangeHold;
            var until = _gapSince + (_gapIsSongChange ? SongChangeHold : PauseHold);
            if (now < until)
            {
                held = true;
                ps = WmpState.psPlaying;
                if (_gapTimer == null)
                {
                    _gapTimer = new DispatcherTimer(DispatcherPriority.Normal);
                    _gapTimer.Tick += (_, _) => { _gapTimer.Stop(); OnMediaChanged(); };
                }
                _gapTimer.Stop();
                _gapTimer.Interval = until - now + TimeSpan.FromMilliseconds(20);
                _gapTimer.Start();
            }
        }
        if (!held)
        {
            if (_gapSince != default && now - _gapSince > TimeSpan.FromSeconds(1))
                Log.Write($"Playback stopped for {(now - _gapSince).TotalSeconds:0.0} s ({(_gapIsSongChange ? "song change" : "pause")}): " +
                          (ps == WmpState.psPlaying ? "carried on" : $"shown as play state {ps}"));
            _gapSince = default;
            _gapTimer?.Stop();
        }
        if (m.Status == MediaStatus.Playing) _playingTitle = m.Title;
        if (ps != PlayState)
        {
            PlayState = ps;
            Raise("player", "playState");
            PlayerEvent?.Invoke("PlayStateChange", new object?[] { ps });
        }
        string status = ps switch
        {
            WmpState.psPlaying => m.SourceDisplayName.Length > 0 ? $"Playing" : "Playing",
            WmpState.psPaused => "Paused",
            WmpState.psStopped => "Stopped",
            WmpState.psTransitioning => "Transitioning",
            WmpState.psReady => "Ready",
            _ => m.HasSession ? "Ready" : "Open SoundCloud or Spotify in your browser",
        };
        if (status != Status)
        {
            Status = status;
            Raise("player", "status");
            PlayerEvent?.Invoke("StatusChange", Array.Empty<object?>());
        }
        Raise("controls", "isAvailable");
        Raise("currentMedia", "duration");
        Raise("currentMedia", "durationString");
        Poll();
        PollVolume(force: true);
    }

    void Poll()
    {
        double pos = _media.Position.TotalSeconds;
        if (Math.Abs(pos - Position) > 0.05)
        {
            double old = Position;
            Position = pos;
            Raise("controls", "currentPosition");
            Raise("controls", "currentPositionString");
            Raise("controls", "currentPositionTimecode");
            if (Math.Abs(pos - old) > 1.5) PlayerEvent?.Invoke("PositionChange", new object?[] { old, pos });
        }
        if (DateTime.UtcNow - _lastVolumePoll > TimeSpan.FromSeconds(1)) PollVolume(false);
    }

    void PollVolume(bool force)
    {
        _lastVolumePoll = DateTime.UtcNow;
        int v = (int)Math.Round(_volume.GetVolume(ProcessName) * 100);
        bool mute = _volume.GetMute(ProcessName);
        if (v != _volumeCache || force) { _volumeCache = v; Raise("settings", "volume"); }
        if (mute != _muteCache || force) { _muteCache = mute; Raise("settings", "mute"); }
    }

    // ---- WMP object model surface --------------------------------------------------------

    public int Volume
    {
        get => _volumeCache;
        set
        {
            value = Math.Clamp(value, 0, 100);
            if (value == _volumeCache) return;
            _volumeCache = value;
            _volume.SetVolume(ProcessName, value / 100f);
            Raise("settings", "volume");
        }
    }

    public bool Mute
    {
        get => _muteCache;
        set
        {
            if (value == _muteCache) return;
            _muteCache = value;
            _volume.SetMute(ProcessName, value);
            Raise("settings", "mute");
        }
    }

    /// <summary>Stored only: the browser's stream balance can't be changed from outside.</summary>
    public int Balance
    {
        get => _balance;
        set { value = Math.Clamp(value, -100, 100); if (value != _balance) { _balance = value; Raise("settings", "balance"); } }
    }

    public void SetPosition(double seconds)
    {
        if (!_media.Current.CanSeek) return;
        Position = Math.Clamp(seconds, 0, Math.Max(0, Duration));
        _media.Seek(TimeSpan.FromSeconds(Position));
        Raise("controls", "currentPosition");
        Raise("controls", "currentPositionString");
    }

    public void Play() { if (_media.Current.Status == MediaStatus.Playing) return; _media.Play(); }
    public void Pause() { _commandedAt = DateTime.UtcNow; _media.Pause(); }
    public void Stop() { _commandedAt = DateTime.UtcNow; _media.Stop(); }
    public void Next() { _skippedAt = DateTime.UtcNow; _media.Next(); }
    public void Previous() { _skippedAt = DateTime.UtcNow; _media.Previous(); }
    public void FastForward() => _media.FastForward();
    public void FastReverse() => _media.Rewind();
    public void TogglePlayPause() { _commandedAt = DateTime.UtcNow; _media.TogglePlayPause(); }

    public bool IsAvailable(string what)
    {
        var m = _media.Current;
        return what.ToLowerInvariant() switch
        {
            "play" => m.HasSession && m.Status != MediaStatus.Playing && (m.CanPlay || m.Status == MediaStatus.Paused),
            "pause" => m.Status == MediaStatus.Playing && m.CanPause,
            "stop" => m.Status is MediaStatus.Playing or MediaStatus.Paused,
            "next" => m.CanNext,
            "previous" => m.CanPrevious,
            "fastforward" => m.CanFastForward && m.Status == MediaStatus.Playing,
            "fastreverse" => m.CanRewind && m.Status == MediaStatus.Playing,
            "currentposition" or "seek" => m.CanSeek,
            "currentitem" => m.HasSession,
            _ => false,
        };
    }

    public bool Shuffle
    {
        get => _media.Current.Shuffle;
        set { if (_media.Current.CanShuffle) _media.SetShuffle(value); Raise("settings", "shuffle"); PlayerEvent?.Invoke("ModeChange", new object?[] { "shuffle", value }); }
    }

    public bool Loop
    {
        get => _media.Current.Repeat;
        set { if (_media.Current.CanRepeat) _media.SetRepeat(value); Raise("settings", "loop"); PlayerEvent?.Invoke("ModeChange", new object?[] { "loop", value }); }
    }

    /// <summary>WMP getItemInfo attribute names mapped onto the session metadata.</summary>
    public string GetItemInfo(string attribute)
    {
        var m = _media.Current;
        return attribute.ToLowerInvariant() switch
        {
            "title" or "name" => m.Title,
            "author" or "artist" or "wm/albumartist" or "displayartist" => m.Artist.Length > 0 ? m.Artist : m.AlbumArtist,
            "album" or "wm/albumtitle" or "albumid" => m.Album,
            "wm/genre" or "genre" => "",
            "copyright" => "",
            "duration" => ((int)Duration).ToString(),
            "sourceurl" => SourceUrl,
            "mediatype" => "audio",
            "filetype" => "",
            "bitrate" => "",
            _ => "",
        };
    }

    public string SourceUrl => _media.Current.HasSession ? "web:" + _media.Current.SourceDisplayName : "";

    public static string FormatTime(double seconds)
    {
        if (seconds <= 0 || double.IsNaN(seconds)) return "00:00";
        var t = TimeSpan.FromSeconds(seconds);
        return t.TotalHours >= 1 ? $"{(int)t.TotalHours}:{t.Minutes:00}:{t.Seconds:00}" : $"{t.Minutes:00}:{t.Seconds:00}";
    }
}
