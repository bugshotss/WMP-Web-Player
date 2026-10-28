using System.Globalization;

namespace WebSkinPlayer.Skins.Scripting;

public enum PType { Any, Bool, Int, Number, String }

public delegate object? ScriptMethod(object?[] args);

/// <summary>
/// Anything skin scripts can see: elements, player, view, theme, ... Property names are
/// case-insensitive (WMP's objects were IDispatch), values are coerced to the declared type,
/// and every change raises <see cref="Changed"/> so wmpprop bindings and *_onchange handlers fire.
/// </summary>
public abstract class ScriptObject
{
    protected readonly Dictionary<string, object?> Values = new(StringComparer.OrdinalIgnoreCase);
    readonly Dictionary<string, ScriptMethod> _methods = new(StringComparer.OrdinalIgnoreCase);
    readonly Dictionary<string, (Func<object?> get, Action<object?>? set)> _computed = new(StringComparer.OrdinalIgnoreCase);

    /// <summary>(object, lower-case property name)</summary>
    public event Action<ScriptObject, string>? Changed;

    /// <summary>Declared property types; unknown names are stored as expandos (PType.Any).</summary>
    protected virtual PType TypeOf(string name) => PType.Any;
    protected virtual bool IsDeclared(string name) => false;
    protected virtual object? DefaultValue(string name) => null;
    protected virtual void OnValueChanged(string name) { }

    public virtual string ClassName => GetType().Name;

    protected void Method(string name, ScriptMethod m) => _methods[name] = m;
    protected void Computed(string name, Func<object?> get, Action<object?>? set = null) => _computed[name] = (get, set);

    public bool TryGetMethod(string name, out ScriptMethod method) => _methods.TryGetValue(name, out method!);

    public virtual bool HasMember(string name) =>
        _computed.ContainsKey(name) || _methods.ContainsKey(name) || Values.ContainsKey(name) || IsDeclared(name);

    public IEnumerable<string> MemberNames => _computed.Keys.Concat(_methods.Keys).Concat(Values.Keys).Distinct(StringComparer.OrdinalIgnoreCase);

    public object? Get(string name)
    {
        if (_computed.TryGetValue(name, out var c)) return c.get();
        if (Values.TryGetValue(name, out var v)) return v;
        return DefaultValue(name);
    }

    public void Set(string name, object? value)
    {
        if (_computed.TryGetValue(name, out var c))
        {
            if (c.set != null) c.set(value);
            return;
        }
        SetValue(name, value);
    }

    /// <summary>Stores a (coerced) value and raises change notifications if it differs.</summary>
    public void SetValue(string name, object? value, bool notify = true)
    {
        var t = TypeOf(name);
        object? coerced = Coerce(value, t);
        bool had = Values.TryGetValue(name, out var old);
        if (!had) old = DefaultValue(name);
        if (had && Same(old, coerced)) return;
        Values[name] = coerced;
        if (!had && Same(old, coerced)) return;
        OnValueChanged(name);
        if (notify) Notify(name);
    }

    public void Notify(string name) => Changed?.Invoke(this, name.ToLowerInvariant());

    public bool HasValue(string name) => Values.ContainsKey(name);

    // ---- typed helpers -------------------------------------------------------------------

    public bool GetBool(string name) => ToBool(Get(name));
    public int GetInt(string name) => (int)ToNumber(Get(name));
    public double GetNumber(string name) => ToNumber(Get(name));
    public string GetString(string name) => ToStr(Get(name));

    static bool Same(object? a, object? b)
    {
        if (a == null || b == null) return a == null && b == null;
        if (a is double da && b is double db) return da.Equals(db);
        return a.Equals(b);
    }

    public static object? Coerce(object? v, PType t) => t switch
    {
        PType.Bool => ToBool(v),
        PType.Int => (double)(long)Math.Truncate(ToNumber(v)),
        PType.Number => ToNumber(v),
        PType.String => ToStr(v),
        _ => v is int or long or float or decimal ? Convert.ToDouble(v, CultureInfo.InvariantCulture) : v,
    };

    public static bool ToBool(object? v) => v switch
    {
        null => false,
        bool b => b,
        double d => d != 0 && !double.IsNaN(d),
        int i => i != 0,
        string s => ParseBoolString(s),
        _ => true,
    };

    static bool ParseBoolString(string s)
    {
        s = s.Trim();
        if (s.Length == 0) return false;
        if (s.Equals("false", StringComparison.OrdinalIgnoreCase) || s.Equals("no", StringComparison.OrdinalIgnoreCase) ||
            s.Equals("off", StringComparison.OrdinalIgnoreCase) || s == "0") return false;
        return true;
    }

    public static double ToNumber(object? v) => v switch
    {
        null => 0,
        double d => double.IsNaN(d) ? 0 : d,
        int i => i,
        long l => l,
        float f => f,
        bool b => b ? 1 : 0,
        string s => ParseNumber(s),
        _ => 0,
    };

    static double ParseNumber(string s)
    {
        s = s.Trim();
        if (s.Length == 0) return 0;
        if (s.StartsWith("0x", StringComparison.OrdinalIgnoreCase) && long.TryParse(s[2..], NumberStyles.HexNumber, CultureInfo.InvariantCulture, out long hx)) return hx;
        if (s.EndsWith("px", StringComparison.OrdinalIgnoreCase)) s = s[..^2];
        if (double.TryParse(s, NumberStyles.Float, CultureInfo.InvariantCulture, out double d)) return d;
        if (s.Equals("true", StringComparison.OrdinalIgnoreCase)) return 1;
        // Leading numeric prefix, like parseFloat.
        int n = 0;
        while (n < s.Length && (char.IsDigit(s[n]) || s[n] == '.' || (n == 0 && (s[n] == '-' || s[n] == '+')))) n++;
        return n > 0 && double.TryParse(s[..n], NumberStyles.Float, CultureInfo.InvariantCulture, out d) ? d : 0;
    }

    public static string ToStr(object? v) => v switch
    {
        null => "",
        string s => s,
        bool b => b ? "true" : "false",
        double d => FormatNumber(d),
        int i => i.ToString(CultureInfo.InvariantCulture),
        ScriptObject o => o.ClassName,
        _ => v.ToString() ?? "",
    };

    public static string FormatNumber(double d)
    {
        if (double.IsNaN(d)) return "NaN";
        if (Math.Abs(d) < 1e15 && d == Math.Floor(d)) return ((long)d).ToString(CultureInfo.InvariantCulture);
        return d.ToString("R", CultureInfo.InvariantCulture);
    }
}

/// <summary>A plain scriptable object with a fixed set of members, used for player sub-objects.</summary>
public class HostObject : ScriptObject
{
    readonly string _name;
    public HostObject(string name) => _name = name;
    public override string ClassName => _name;
    public void AddMethod(string name, ScriptMethod m) => Method(name, m);
    public void AddProperty(string name, Func<object?> get, Action<object?>? set = null) => Computed(name, get, set);
}
