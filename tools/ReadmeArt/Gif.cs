using System.IO;
using System.Text;

namespace ReadmeArt;

/// <summary>
/// Writes a looping animated GIF from opaque BGRA frames. Each frame stores only the rectangle that changed, with
/// unchanged pixels inside it transparent, and gets its own 255-colour palette (median cut), optionally with
/// ordered dithering, which stays still from frame to frame where the picture does.
/// </summary>
sealed class GifWriter : IDisposable
{
    readonly Stream _out;
    readonly int _width, _height;
    readonly double _dither;
    int[]? _shown, _pending;
    int _pendingDelay;

    public GifWriter(string path, int width, int height, double dither)
    {
        Directory.CreateDirectory(Path.GetDirectoryName(Path.GetFullPath(path))!);
        _out = File.Create(path);
        (_width, _height, _dither) = (width, height, dither);
        Bytes(Encoding.ASCII.GetBytes("GIF89a"));
        U16(width); U16(height);
        Bytes(0x70, 0, 0); // no global palette
        Bytes(0x21, 0xFF, 11);
        Bytes(Encoding.ASCII.GetBytes("NETSCAPE2.0"));
        Bytes(3, 1, 0, 0, 0); // loop forever
    }

    /// <summary>Adds a frame shown for <paramref name="delay"/> hundredths of a second. A frame identical to the
    /// one before it just lengthens that one.</summary>
    public void Add(int[] bgra, int delay)
    {
        if (_pending != null && bgra.AsSpan().SequenceEqual(_pending)) { _pendingDelay += delay; return; }
        Flush();
        _pending = (int[])bgra.Clone();
        _pendingDelay = delay;
    }

    void Flush()
    {
        if (_pending is not { } px) return;
        int x0 = 0, y0 = 0, x1 = _width - 1, y1 = _height - 1;
        if (_shown != null)
        {
            (x0, y0, x1, y1) = (_width, _height, -1, -1);
            for (int y = 0; y < _height; y++)
                for (int x = 0; x < _width; x++)
                    if (px[y * _width + x] != _shown[y * _width + x])
                    {
                        x0 = Math.Min(x0, x); x1 = Math.Max(x1, x);
                        y0 = Math.Min(y0, y); y1 = Math.Max(y1, y);
                    }
            if (x1 < 0) (x0, y0, x1, y1) = (0, 0, 0, 0);
        }
        int w = x1 - x0 + 1, h = y1 - y0 + 1;
        var region = new int[w * h];
        var keep = new bool[w * h];
        bool anyKept = false;
        for (int y = 0; y < h; y++)
            for (int x = 0; x < w; x++)
            {
                int i = (y0 + y) * _width + x0 + x;
                region[y * w + x] = px[i];
                if (_shown != null && px[i] == _shown[i]) keep[y * w + x] = anyKept = true;
            }
        var (palette, indices) = Quantizer.Run(region, keep, w, x0, y0, _dither);

        Bytes(0x21, 0xF9, 4, (byte)(1 << 2 | (anyKept ? 1 : 0))); // leave each frame in place under the next
        U16(_pendingDelay);
        Bytes(Quantizer.Transparent, 0);
        Bytes(0x2C); U16(x0); U16(y0); U16(w); U16(h);
        Bytes(0x87); // local palette, 256 entries
        foreach (int c in palette) Bytes((byte)(c >> 16), (byte)(c >> 8), (byte)c);
        Lzw(indices);
        _shown = px;
        _pending = null;
    }

    /// <summary>GIF's variable-width LZW, 8-bit codes, in sub-blocks of up to 255 bytes.</summary>
    void Lzw(byte[] indices)
    {
        const int min = 8, clear = 1 << min, end = clear + 1;
        var block = new List<byte>(256);
        int bits = 0, bitCount = 0;
        void Put(int code, int size)
        {
            bits |= code << bitCount;
            bitCount += size;
            while (bitCount >= 8)
            {
                block.Add((byte)bits);
                bits >>= 8;
                bitCount -= 8;
                if (block.Count == 255) { _out.WriteByte(255); Bytes(block.ToArray()); block.Clear(); }
            }
        }

        _out.WriteByte(min);
        var table = new Dictionary<int, int>();
        int size = min + 1, next = end + 1;
        Put(clear, size);
        int prefix = indices[0];
        for (int i = 1; i < indices.Length; i++)
        {
            int k = indices[i], key = prefix << 8 | k;
            if (table.TryGetValue(key, out int code)) { prefix = code; continue; }
            Put(prefix, size);
            if (next < 4096)
            {
                table[key] = next++;
                if (next > 1 << size && size < 12) size++;
            }
            else
            {
                Put(clear, size);
                table.Clear();
                size = min + 1;
                next = end + 1;
            }
            prefix = k;
        }
        Put(prefix, size);
        Put(end, size);
        if (bitCount > 0) block.Add((byte)bits);
        if (block.Count > 0) { _out.WriteByte((byte)block.Count); Bytes(block.ToArray()); }
        _out.WriteByte(0);
    }

    void Bytes(params byte[] b) => _out.Write(b);
    void U16(int v) => Bytes((byte)v, (byte)(v >> 8));

    public void Dispose()
    {
        Flush();
        _out.WriteByte(0x3B);
        _out.Dispose();
    }
}

/// <summary>Median-cut palettes over 6-bit-per-channel colour cells.</summary>
static class Quantizer
{
    public const byte Transparent = 255;
    const int Cells = 1 << 18;
    static readonly int[,] Bayer = { { 0, 8, 2, 10 }, { 12, 4, 14, 6 }, { 3, 11, 1, 9 }, { 15, 7, 13, 5 } };

    static int Cell(int r, int g, int b) => (r >> 2) << 12 | (g >> 2) << 6 | (b >> 2);

    /// <returns>256 RGB entries (the last unused, for transparency) and one index per pixel.</returns>
    public static (int[] Palette, byte[] Indices) Run(int[] bgra, bool[] keep, int width, int left, int top, double dither)
    {
        var count = new int[Cells];
        var sum = new long[Cells * 3];
        var used = new List<int>();
        for (int i = 0; i < bgra.Length; i++)
        {
            if (keep[i]) continue;
            int c = bgra[i], r = c >> 16 & 255, g = c >> 8 & 255, b = c & 255, cell = Cell(r, g, b);
            if (count[cell]++ == 0) used.Add(cell);
            sum[cell * 3] += r; sum[cell * 3 + 1] += g; sum[cell * 3 + 2] += b;
        }

        var boxes = new List<(int Start, int End)>();
        var cells = used.ToArray();
        if (cells.Length > 0) boxes.Add((0, cells.Length));
        while (boxes.Count < 255)
        {
            int best = -1, bestChannel = 0;
            double bestScore = 0;
            for (int bi = 0; bi < boxes.Count; bi++)
            {
                var (s, e) = boxes[bi];
                if (e - s < 2) continue;
                var (channel, range) = Widest(cells, s, e);
                long n = 0;
                for (int i = s; i < e; i++) n += count[cells[i]];
                double score = (double)range * range * n;
                if (score > bestScore) (bestScore, best, bestChannel) = (score, bi, channel);
            }
            if (best < 0) break;
            var (start, stop) = boxes[best];
            int shift = 12 - bestChannel * 6;
            Array.Sort(cells, start, stop - start, Comparer<int>.Create((a, b) => (a >> shift & 63).CompareTo(b >> shift & 63)));
            long total = 0, half = 0;
            for (int i = start; i < stop; i++) total += count[cells[i]];
            int cut = start + 1;
            for (int i = start; i < stop - 1; i++)
            {
                half += count[cells[i]];
                cut = i + 1;
                if (half * 2 >= total) break;
            }
            boxes[best] = (start, cut);
            boxes.Add((cut, stop));
        }

        var palette = new int[256];
        var colors = new (int R, int G, int B)[boxes.Count];
        for (int bi = 0; bi < boxes.Count; bi++)
        {
            long n = 0, r = 0, g = 0, b = 0;
            for (int i = boxes[bi].Start; i < boxes[bi].End; i++)
            {
                int c = cells[i];
                n += count[c]; r += sum[c * 3]; g += sum[c * 3 + 1]; b += sum[c * 3 + 2];
            }
            colors[bi] = ((int)(r / n), (int)(g / n), (int)(b / n));
            palette[bi] = colors[bi].R << 16 | colors[bi].G << 8 | colors[bi].B;
        }

        var nearest = new short[Cells];
        Array.Fill(nearest, (short)-1);
        var indices = new byte[bgra.Length];
        for (int i = 0; i < bgra.Length; i++)
        {
            if (keep[i]) { indices[i] = Transparent; continue; }
            int c = bgra[i], r = c >> 16 & 255, g = c >> 8 & 255, b = c & 255;
            if (dither > 0)
            {
                int x = left + i % width, y = top + i / width;
                int d = (int)Math.Round((Bayer[y & 3, x & 3] / 16.0 - 0.47) * dither);
                r = Math.Clamp(r + d, 0, 255); g = Math.Clamp(g + d, 0, 255); b = Math.Clamp(b + d, 0, 255);
            }
            int cell = Cell(r, g, b);
            if (nearest[cell] < 0) nearest[cell] = (short)Closest(colors, (r & ~3) + 2, (g & ~3) + 2, (b & ~3) + 2);
            indices[i] = (byte)nearest[cell];
        }
        return (palette, indices);
    }

    static (int Channel, int Range) Widest(int[] cells, int start, int end)
    {
        int best = 0, bestRange = -1;
        for (int ch = 0; ch < 3; ch++)
        {
            int shift = 12 - ch * 6, lo = 63, hi = 0;
            for (int i = start; i < end; i++) { int v = cells[i] >> shift & 63; lo = Math.Min(lo, v); hi = Math.Max(hi, v); }
            if (hi - lo > bestRange) (best, bestRange) = (ch, hi - lo);
        }
        return (best, bestRange);
    }

    static int Closest((int R, int G, int B)[] colors, int r, int g, int b)
    {
        int best = 0, bestDistance = int.MaxValue;
        for (int i = 0; i < colors.Length; i++)
        {
            int dr = colors[i].R - r, dg = colors[i].G - g, db = colors[i].B - b;
            int d = 3 * dr * dr + 4 * dg * dg + 2 * db * db;
            if (d < bestDistance) (best, bestDistance) = (i, d);
        }
        return best;
    }
}
