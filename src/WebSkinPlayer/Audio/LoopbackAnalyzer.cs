using NAudio.CoreAudioApi;
using NAudio.CoreAudioApi.Interfaces;
using NAudio.Wave;

namespace WebSkinPlayer.Audio;

/// <summary>
/// One analysis frame, shaped like WMP's TimedLevel (2 channels x 1024 bytes of spectrum and
/// waveform) plus float versions and band energies for the built-in visualizations.
/// </summary>
public sealed class VisFrame
{
    public const int Bins = 1024;
    /// <summary>[channel * 1024 + bin], 0..255, linear frequency 0..Nyquist.</summary>
    public readonly byte[] Frequency = new byte[2 * Bins];
    /// <summary>[channel * 1024 + sample], 0..255 with 128 = silence.</summary>
    public readonly byte[] Waveform = new byte[2 * Bins];
    /// <summary>Mono spectrum 0..1 (log-magnitude scaled).</summary>
    public readonly float[] Spectrum = new float[Bins];
    /// <summary>Per-channel waveform -1..1.</summary>
    public readonly float[] WaveL = new float[Bins], WaveR = new float[Bins];
    public float Bass, Mid, Treble, Level;
    /// <summary>True on the frame a bass transient is detected.</summary>
    public bool Beat;
    public bool Playing;
    /// <summary>Seconds since the analyzer started.</summary>
    public double Time;
    /// <summary>100ns units, as WMP's TimedLevel.timeStamp.</summary>
    public long TimeStamp;
    public int SampleRate = 48000;
}

/// <summary>
/// Captures whatever Windows is playing (WASAPI loopback on the default output) and turns it
/// into visualization frames. Restarts itself when the default device changes.
/// </summary>
public sealed class LoopbackAnalyzer : IDisposable, IMMNotificationClient
{
    const int FftSize = 2048;
    const int RingSize = 8192;

    readonly object _gate = new();
    readonly float[] _ringL = new float[RingSize], _ringR = new float[RingSize];
    long _written;
    long _lastDataTicks;
    WasapiLoopbackCapture? _capture;
    readonly MMDeviceEnumerator _enumerator = new();
    readonly System.Diagnostics.Stopwatch _clock = System.Diagnostics.Stopwatch.StartNew();
    int _sampleRate = 48000;
    volatile bool _restartPending;
    bool _disposed;

    // analysis state
    readonly float[] _window = new float[FftSize];
    readonly float[] _re = new float[FftSize], _im = new float[FftSize];
    readonly float[] _smoothSpectrum = new float[VisFrame.Bins];
    float _bassAvg;
    double _lastBeat;
    readonly VisFrame _frame = new();
    long _frameTick = -1;

    public LoopbackAnalyzer()
    {
        for (int i = 0; i < FftSize; i++) _window[i] = (float)(0.5 - 0.5 * Math.Cos(2 * Math.PI * i / (FftSize - 1)));
        try { _enumerator.RegisterEndpointNotificationCallback(this); } catch { }
        Start();
    }

    void Start()
    {
        lock (_gate)
        {
            if (_disposed) return;
            try
            {
                _capture?.Dispose();
                _capture = new WasapiLoopbackCapture();
                _sampleRate = _capture.WaveFormat.SampleRate;
                _capture.DataAvailable += OnData;
                _capture.RecordingStopped += (_, e) =>
                {
                    if (e.Exception != null) Log.Write("Loopback stopped: " + e.Exception.Message);
                    ScheduleRestart();
                };
                _capture.StartRecording();
                Log.Write($"Loopback capture: {_capture.WaveFormat}");
            }
            catch (Exception ex)
            {
                Log.Write("Loopback start failed: " + ex.Message);
                _capture = null;
                ScheduleRestart();
            }
        }
    }

    void ScheduleRestart()
    {
        if (_restartPending || _disposed) return;
        _restartPending = true;
        Task.Delay(1500).ContinueWith(_ => { _restartPending = false; Start(); });
    }

    void OnData(object? sender, WaveInEventArgs e)
    {
        var cap = _capture;
        if (cap == null || e.BytesRecorded == 0) return;
        var fmt = cap.WaveFormat;
        int channels = fmt.Channels;
        int bytesPerSample = fmt.BitsPerSample / 8;
        int frameBytes = bytesPerSample * channels;
        int frames = e.BytesRecorded / frameBytes;
        // The shared-mode mix format is 32-bit float in practice; only integer PCM is special-cased.
        bool isFloat = bytesPerSample == 4 && fmt.Encoding != WaveFormatEncoding.Pcm;
        var buf = e.Buffer;

        lock (_gate)
        {
            for (int f = 0; f < frames; f++)
            {
                int o = f * frameBytes;
                float l = Read(buf, o, bytesPerSample, isFloat);
                float r = channels > 1 ? Read(buf, o + bytesPerSample, bytesPerSample, isFloat) : l;
                int idx = (int)(_written % RingSize);
                _ringL[idx] = l;
                _ringR[idx] = r;
                _written++;
            }
            _lastDataTicks = _clock.ElapsedTicks;
        }
    }

    static float Read(byte[] b, int o, int bytes, bool isFloat)
    {
        switch (bytes)
        {
            case 4 when isFloat: return BitConverter.ToSingle(b, o);
            case 4: return BitConverter.ToInt32(b, o) / 2147483648f;
            case 3: return ((b[o] << 8 | b[o + 1] << 16 | b[o + 2] << 24) >> 8) / 8388608f;
            case 2: return BitConverter.ToInt16(b, o) / 32768f;
            default: return 0;
        }
    }

    /// <summary>
    /// Returns the analysis for the current UI frame; repeated calls within the same
    /// rendering tick share one result so every visualizer sees identical data.
    /// </summary>
    public VisFrame GetFrame(long renderTick)
    {
        if (renderTick == _frameTick) return _frame;
        _frameTick = renderTick;
        Analyze(_frame);
        if (TestSignal) Synthesize(_frame, renderTick);
        return _frame;
    }

    /// <summary>WEBSKINPLAYER_TESTSIGNAL=1 feeds made-up music to the visualizers (tests without sound).</summary>
    static readonly bool TestSignal = Environment.GetEnvironmentVariable("WEBSKINPLAYER_TESTSIGNAL") == "1";

    static void Synthesize(VisFrame f, long tick)
    {
        double t = tick / 10_000_000.0, beat = 0.5 + 0.5 * Math.Sin(t * Math.PI * 4);
        for (int ch = 0; ch < 2; ch++)
            for (int i = 0; i < VisFrame.Bins; i++)
            {
                double s = Math.Sin(i * 0.05 + t * 7 + ch) * 0.5 + Math.Sin(i * 0.013 + t * 3) * 0.3 * beat;
                f.Waveform[ch * VisFrame.Bins + i] = (byte)Math.Clamp(128 + s * 110, 0, 255);
                f.Frequency[ch * VisFrame.Bins + i] = (byte)Math.Clamp(220 * Math.Exp(-i / 160.0) * (0.55 + 0.45 * Math.Sin(t * 5 + i * 0.03)) * (0.6 + 0.4 * beat), 0, 255);
            }
        f.Playing = true;
    }

    void Analyze(VisFrame f)
    {
        bool silentTimeout;
        lock (_gate)
        {
            long end = _written;
            // Loopback delivers no packets while nothing plays; treat that as silence.
            silentTimeout = (_clock.ElapsedTicks - _lastDataTicks) > System.Diagnostics.Stopwatch.Frequency / 5;
            for (int i = 0; i < FftSize; i++)
            {
                long src = end - FftSize + i;
                float l = 0, r = 0;
                if (src >= 0 && !silentTimeout)
                {
                    int idx = (int)(src % RingSize);
                    l = _ringL[idx];
                    r = _ringR[idx];
                }
                _re[i] = (l + r) * 0.5f * _window[i];
                _im[i] = 0;
                if (i >= FftSize - VisFrame.Bins)
                {
                    int w = i - (FftSize - VisFrame.Bins);
                    f.WaveL[w] = l;
                    f.WaveR[w] = r;
                }
            }
        }

        Fft.Transform(_re, _im);

        float bass = 0, mid = 0, treble = 0, level = 0;
        float binHz = _sampleRate / (float)FftSize;
        for (int i = 0; i < VisFrame.Bins; i++)
        {
            float mag = MathF.Sqrt(_re[i] * _re[i] + _im[i] * _im[i]) * (4f / FftSize);
            float db = 20f * MathF.Log10(mag + 1e-9f);
            float v = Math.Clamp((db + 75f) / 65f, 0f, 1f);
            // Fast attack, slower release, like WMP's own analyser.
            float s = _smoothSpectrum[i];
            s = v > s ? v : s * 0.82f + v * 0.18f;
            _smoothSpectrum[i] = s;
            f.Spectrum[i] = s;
            byte b = (byte)(s * 255f);
            f.Frequency[i] = b;
            f.Frequency[VisFrame.Bins + i] = b;

            float hz = i * binHz;
            if (hz < 250) bass = MathF.Max(bass, v);
            else if (hz < 2500) mid += v;
            else if (hz < 12000) treble += v;
        }
        mid /= Math.Max(1, (int)(2250 / binHz));
        treble /= Math.Max(1, (int)(9500 / binHz));

        for (int i = 0; i < VisFrame.Bins; i++)
        {
            float l = f.WaveL[i], r = f.WaveR[i];
            level = MathF.Max(level, MathF.Max(MathF.Abs(l), MathF.Abs(r)));
            f.Waveform[i] = (byte)Math.Clamp(128 + l * 127f, 0, 255);
            f.Waveform[VisFrame.Bins + i] = (byte)Math.Clamp(128 + r * 127f, 0, 255);
        }

        double now = _clock.Elapsed.TotalSeconds;
        _bassAvg = _bassAvg * 0.92f + bass * 0.08f;
        f.Beat = bass > 0.35f && bass > _bassAvg * 1.25f && now - _lastBeat > 0.18;
        if (f.Beat) _lastBeat = now;

        f.Bass = bass;
        f.Mid = Math.Clamp(mid * 1.6f, 0, 1);
        f.Treble = Math.Clamp(treble * 2.2f, 0, 1);
        f.Level = Math.Clamp(level, 0, 1);
        f.Playing = !silentTimeout;
        f.Time = now;
        f.TimeStamp = (long)(now * 10_000_000);
        f.SampleRate = _sampleRate;
    }

    // IMMNotificationClient: restart capture when the default output changes.
    public void OnDefaultDeviceChanged(DataFlow flow, Role role, string defaultDeviceId)
    {
        if (flow == DataFlow.Render && role == Role.Multimedia) ScheduleRestart();
    }
    public void OnDeviceStateChanged(string deviceId, DeviceState newState) { }
    public void OnDeviceAdded(string pwstrDeviceId) { }
    public void OnDeviceRemoved(string deviceId) { }
    public void OnPropertyValueChanged(string pwstrDeviceId, PropertyKey key) { }

    public void Dispose()
    {
        lock (_gate)
        {
            _disposed = true;
            try { _enumerator.UnregisterEndpointNotificationCallback(this); } catch { }
            try { _capture?.StopRecording(); } catch { }
            _capture?.Dispose();
            _capture = null;
        }
    }
}

static class Fft
{
    /// <summary>In-place iterative radix-2 FFT. Length must be a power of two.</summary>
    public static void Transform(float[] re, float[] im)
    {
        int n = re.Length;
        for (int i = 1, j = 0; i < n; i++)
        {
            int bit = n >> 1;
            for (; (j & bit) != 0; bit >>= 1) j ^= bit;
            j ^= bit;
            if (i < j) { (re[i], re[j]) = (re[j], re[i]); (im[i], im[j]) = (im[j], im[i]); }
        }
        for (int len = 2; len <= n; len <<= 1)
        {
            double ang = -2 * Math.PI / len;
            float wr = (float)Math.Cos(ang), wi = (float)Math.Sin(ang);
            for (int i = 0; i < n; i += len)
            {
                float cr = 1, ci = 0;
                for (int k = 0; k < len / 2; k++)
                {
                    int a = i + k, b = i + k + len / 2;
                    float xr = re[b] * cr - im[b] * ci;
                    float xi = re[b] * ci + im[b] * cr;
                    re[b] = re[a] - xr; im[b] = im[a] - xi;
                    re[a] += xr; im[a] += xi;
                    float t = cr * wr - ci * wi;
                    ci = cr * wi + ci * wr;
                    cr = t;
                }
            }
        }
    }
}
