using System.Globalization;
using System.Windows.Media;

namespace WebSkinPlayer.Visualization;

/// <summary>
/// Recolours a visualizer's finished picture, so it works alike for every visualization, ours and the hosted WMP
/// plug-ins: a palette (the picture's brightness mapped onto a gradient), a hue shift, or a colour of your own.
/// Kept per visualization in the settings as a spec: "palette:Fire", "hue:120" or "custom:#RRGGBB"; none means the
/// picture as the visualization drew it.
/// </summary>
public static class VisualizationColors
{
    /// <summary>Gradients from black (silence) to the brightest parts of the picture.</summary>
    public static readonly IReadOnlyList<(string Name, string[] Stops)> Palettes = new (string, string[])[]
    {
        ("Fire", new[] { "#000000", "#5A0000", "#C81E00", "#FF7A00", "#FFD24A", "#FFFBE6" }),
        ("Sunset", new[] { "#000000", "#2B0A3D", "#8C1C5A", "#E8505B", "#F9A03F", "#FFE7A0" }),
        ("Gold", new[] { "#000000", "#2E1F00", "#7A5200", "#D19A1E", "#F7D774", "#FFF8DC" }),
        ("Rose", new[] { "#000000", "#3D0A1E", "#9E1F4F", "#E85C8A", "#F9B8CF", "#FFF0F5" }),
        ("Neon", new[] { "#000000", "#2A0055", "#9B00D9", "#FF2FB3", "#3FF2FF", "#F0FFFF" }),
        ("Ice", new[] { "#000000", "#001A4D", "#0059B3", "#3FA9F5", "#BFE9FF", "#FFFFFF" }),
        ("Ocean", new[] { "#000000", "#021A2E", "#07507A", "#0E9AA7", "#7FE3D6", "#EFFFFB" }),
        ("Emerald", new[] { "#000000", "#00261A", "#00733F", "#19C26B", "#9CF5B5", "#F2FFF6" }),
        ("Green Screen", new[] { "#000000", "#002B0B", "#00A02E", "#33FF66", "#CCFFD9" }),
        ("Amber Screen", new[] { "#000000", "#2E1600", "#A85A00", "#FFB000", "#FFE6A8" }),
        ("Black and White", new[] { "#000000", "#FFFFFF" }),
    };

    public static readonly int[] HueShifts = { 60, 120, 180, 240, 300 };

    static Dictionary<string, string> Stored => App.Services.Settings.VisColors;
    static readonly Dictionary<string, (string Spec, ColorMap? Map)> _maps = new(StringComparer.OrdinalIgnoreCase);

    /// <summary>The spec chosen for a visualization ("" = its own colours).</summary>
    public static string Get(string effectId) => Stored.TryGetValue(effectId, out var s) ? s : "";

    public static void Set(string effectId, string spec)
    {
        if (spec.Length == 0) Stored.Remove(effectId);
        else Stored[effectId] = spec;
        App.Services.Settings.Save();
    }

    /// <summary>The recolouring for a visualization, or null to show it as drawn.</summary>
    public static ColorMap? MapFor(string effectId)
    {
        string spec = Get(effectId);
        if (spec.Length == 0) return null;
        if (_maps.TryGetValue(effectId, out var cached) && cached.Spec == spec) return cached.Map;
        var map = Parse(spec);
        _maps[effectId] = (spec, map);
        return map;
    }

    /// <summary>What a spec is called in menus.</summary>
    public static string Describe(string spec)
    {
        var (kind, value) = Split(spec);
        return kind switch
        {
            "palette" => value,
            "hue" => $"Colors Shifted {value}°",
            "custom" => $"Custom Color {value}",
            _ => "Original Colors",
        };
    }

    static (string Kind, string Value) Split(string spec)
    {
        int i = spec.IndexOf(':');
        return i < 0 ? (spec, "") : (spec[..i].ToLowerInvariant(), spec[(i + 1)..]);
    }

    static ColorMap? Parse(string spec)
    {
        try
        {
            var (kind, value) = Split(spec);
            switch (kind)
            {
                case "palette":
                    var palette = Palettes.FirstOrDefault(p => p.Name.Equals(value, StringComparison.OrdinalIgnoreCase));
                    return palette.Stops == null ? null : ColorMap.Gradient(palette.Stops.Select(Rgb).ToArray());
                case "hue":
                    return ColorMap.HueShift(double.Parse(value, CultureInfo.InvariantCulture));
                case "custom":
                    // Dark parts fade to black, the middle takes the colour, the brightest go towards white.
                    var c = Rgb(value);
                    return ColorMap.Gradient(new[] { Colors.Black, Scale(c, 0.35), c, Lighten(c, 0.6) });
            }
        }
        catch (Exception ex) { Log.Write($"Visualization colors '{spec}': {ex.Message}"); }
        return null;
    }

    static Color Rgb(string hex) => (Color)ColorConverter.ConvertFromString(hex);
    static Color Scale(Color c, double k) => Color.FromRgb((byte)(c.R * k), (byte)(c.G * k), (byte)(c.B * k));
    static Color Lighten(Color c, double k) => Color.FromRgb((byte)(c.R + (255 - c.R) * k), (byte)(c.G + (255 - c.G) * k), (byte)(c.B + (255 - c.B) * k));
}

/// <summary>A per-pixel colour transform over 0x00RRGGBB pixels, built from lookup tables.</summary>
public sealed unsafe class ColorMap
{
    // Gradient: brightness (0-255) to colour. Hue shift: each output channel as the sum of three tables, one per input
    // channel, in 1/1024ths.
    readonly int[]? _gradient;
    readonly int[]? _hue;

    ColorMap(int[]? gradient, int[]? hue) => (_gradient, _hue) = (gradient, hue);

    public static ColorMap Gradient(Color[] stops)
    {
        var lut = new int[256];
        for (int i = 0; i < 256; i++)
        {
            double t = i / 255.0 * (stops.Length - 1);
            int k = Math.Min((int)t, stops.Length - 2);
            double f = t - k;
            Color a = stops[k], b = stops[k + 1];
            lut[i] = (int)Math.Round(a.R + (b.R - a.R) * f) << 16 | (int)Math.Round(a.G + (b.G - a.G) * f) << 8 | (int)Math.Round(a.B + (b.B - a.B) * f);
        }
        return new ColorMap(lut, null);
    }

    /// <summary>Rotates hue and keeps brightness (the matrix of SVG's hueRotate filter).</summary>
    public static ColorMap HueShift(double degrees)
    {
        double a = degrees * Math.PI / 180, cos = Math.Cos(a), sin = Math.Sin(a);
        double[,] m =
        {
            { 0.213 + cos * 0.787 - sin * 0.213, 0.715 - cos * 0.715 - sin * 0.715, 0.072 - cos * 0.072 + sin * 0.928 },
            { 0.213 - cos * 0.213 + sin * 0.143, 0.715 + cos * 0.285 + sin * 0.140, 0.072 - cos * 0.072 - sin * 0.283 },
            { 0.213 - cos * 0.213 - sin * 0.787, 0.715 - cos * 0.715 + sin * 0.715, 0.072 + cos * 0.928 + sin * 0.072 },
        };
        var tables = new int[9 * 256];
        for (int row = 0; row < 3; row++)
            for (int col = 0; col < 3; col++)
                for (int v = 0; v < 256; v++)
                    tables[(row * 3 + col) * 256 + v] = (int)Math.Round(m[row, col] * v * 1024);
        return new ColorMap(null, tables);
    }

    public void Apply(int* source, int[] target)
    {
        fixed (int* dst = target)
        {
            int n = target.Length;
            if (_gradient != null)
            {
                fixed (int* lut = _gradient)
                    for (int i = 0; i < n; i++)
                    {
                        int c = source[i];
                        dst[i] = lut[((c >> 16 & 255) * 77 + (c >> 8 & 255) * 150 + (c & 255) * 29) >> 8];
                    }
            }
            else if (_hue != null)
            {
                fixed (int* t = _hue)
                    for (int i = 0; i < n; i++)
                    {
                        int c = source[i], r = c >> 16 & 255, g = c >> 8 & 255, b = c & 255;
                        int nr = (t[r] + t[256 + g] + t[512 + b]) >> 10;
                        int ng = (t[768 + r] + t[1024 + g] + t[1280 + b]) >> 10;
                        int nb = (t[1536 + r] + t[1792 + g] + t[2048 + b]) >> 10;
                        dst[i] = Math.Clamp(nr, 0, 255) << 16 | Math.Clamp(ng, 0, 255) << 8 | Math.Clamp(nb, 0, 255);
                    }
            }
        }
    }
}
