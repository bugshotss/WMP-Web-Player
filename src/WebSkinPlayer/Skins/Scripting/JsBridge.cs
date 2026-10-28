using System.Runtime.CompilerServices;
using Jint;
using Jint.Native;
using Jint.Native.Object;
using Jint.Runtime;
using Jint.Runtime.Descriptors;
using Jint.Runtime.Interop;

namespace WebSkinPlayer.Skins.Scripting;

/// <summary>A compiled event handler or expression bound to one script engine.</summary>
public sealed class CompiledHandler
{
    public required JsValue Function { get; init; }
    public required string Source { get; init; }
    public required string Name { get; init; }
}

/// <summary>
/// One JavaScript engine per skin VIEW (as WMP had one JScript context per view), with
/// case-insensitive wrappers around <see cref="ScriptObject"/>s.
/// </summary>
public sealed class JsBridge
{
    public Engine Engine { get; }
    readonly ConditionalWeakTable<ScriptObject, JsHostObject> _wrappers = new();
    readonly JsValue _evalInScope;
    readonly Dictionary<string, JsValue> _compiled = new();
    readonly string _label;
    public int ErrorCount { get; private set; }
    public event Action<string>? ScriptError;

    public JsBridge(string label)
    {
        _label = label;
        Engine = new Engine(o =>
        {
            o.Strict = false;
            o.LimitRecursion(400);
            o.TimeoutInterval(TimeSpan.FromSeconds(4));
            o.CatchClrExceptions();
        });
        _evalInScope = Engine.Evaluate("(function(__self, __code){ with(__self){ return eval(__code); } })");
        // JScript host functions some skins probe.
        Engine.Execute(@"
            function ScriptEngine(){ return 'JScript'; }
            function ScriptEngineMajorVersion(){ return 5; }
            function ScriptEngineMinorVersion(){ return 8; }
            function ScriptEngineBuildVersion(){ return 1; }
            function Enumerator(c){ this._c=c; this._i=0; }
            Enumerator.prototype.atEnd=function(){ var n=this._c? (this._c.count!==undefined?this._c.count:this._c.length):0; return this._i>=n; };
            Enumerator.prototype.item=function(){ return this._c.item? this._c.item(this._i) : this._c[this._i]; };
            Enumerator.prototype.moveNext=function(){ this._i++; };
            Enumerator.prototype.moveFirst=function(){ this._i=0; };
            function ActiveXObject(n){ throw new Error('ActiveXObject is not available: ' + n); }
            function GetObject(){ return null; }
        ");
    }

    public JsHostObject Wrap(ScriptObject o) => _wrappers.GetValue(o, x => new JsHostObject(this, x));

    public void SetGlobal(string name, object? value) => Engine.SetValue(name, ToJs(value));

    public JsValue ToJs(object? v) => v switch
    {
        null => JsValue.Null,
        JsValue j => j,
        string s => new JsString(s),
        bool b => b ? JsBoolean.True : JsBoolean.False,
        double d => new JsNumber(d),
        int i => new JsNumber(i),
        long l => new JsNumber(l),
        float f => new JsNumber(f),
        ScriptObject o => Wrap(o),
        object?[] arr => new JsArray(Engine, arr.Select(ToJs).ToArray()),
        _ => JsValue.FromObject(Engine, v),
    };

    public object? FromJs(JsValue v)
    {
        if (v.IsNull() || v.IsUndefined()) return null;
        if (v.IsString()) return v.AsString();
        if (v.IsNumber()) return v.AsNumber();
        if (v.IsBoolean()) return v.AsBoolean();
        if (v is JsHostObject h) return h.Target;
        if (v is ObjectWrapper w) return w.Target;
        return v; // functions, arrays, plain objects stay as JS values
    }

    void Report(string where, Exception ex)
    {
        ErrorCount++;
        string msg = ex is JavaScriptException js ? $"{js.Error} (line {js.Location.Start.Line})" : ex.Message;
        string line = $"[{_label}] script error in {where}: {msg}";
        Log.Write(line);
        ScriptError?.Invoke(line);
    }

    /// <summary>Runs a script file in the global scope.</summary>
    public bool Execute(string code, string sourceName)
    {
        try
        {
            Engine.Execute(JScriptCompat.Prepare(code), sourceName);
            return true;
        }
        catch (Exception ex) { Report(sourceName, ex); return false; }
    }

    /// <summary>Evaluates a "jscript:" attribute in the element's scope.</summary>
    public object? EvaluateIn(ScriptObject scope, string expression, string where)
    {
        try
        {
            var r = Engine.Call(_evalInScope, JsValue.Undefined, new JsValue[] { Wrap(scope), new JsString(JScriptCompat.Prepare(expression)) });
            return FromJs(r);
        }
        catch (Exception ex) { Report(where, ex); return null; }
    }

    /// <summary>Compiles handler code into function(self, params...) { with(self) { code } }.</summary>
    public CompiledHandler? Compile(string code, string name, params string[] parameters)
    {
        code = JScriptCompat.StripPrefix(code);
        if (string.IsNullOrWhiteSpace(code)) return null;
        string paramList = string.Join(",", new[] { "__self" }.Concat(parameters));
        string src = $"(function({paramList}){{ with(__self){{ {JScriptCompat.Prepare(code)}\n}} }})";
        try
        {
            if (!_compiled.TryGetValue(src, out var fn))
            {
                fn = Engine.Evaluate(src, name);
                _compiled[src] = fn;
            }
            return new CompiledHandler { Function = fn, Source = code, Name = name };
        }
        catch (Exception ex) { Report(name + " (compile)", ex); return null; }
    }

    public object? Invoke(CompiledHandler h, ScriptObject self, params object?[] args)
    {
        try
        {
            var jsArgs = new JsValue[args.Length + 1];
            jsArgs[0] = Wrap(self);
            for (int i = 0; i < args.Length; i++) jsArgs[i + 1] = ToJs(args[i]);
            return FromJs(Engine.Call(h.Function, Wrap(self), jsArgs));
        }
        catch (Exception ex) { Report(h.Name, ex); return null; }
    }

    /// <summary>Calls a JS function value (e.g. one a script assigned to a property).</summary>
    public object? CallFunction(JsValue fn, ScriptObject? self, params object?[] args)
    {
        try
        {
            return FromJs(Engine.Call(fn, self != null ? Wrap(self) : JsValue.Undefined, args.Select(ToJs).ToArray()));
        }
        catch (Exception ex) { Report("callback", ex); return null; }
    }

    static readonly System.Text.RegularExpressions.Regex Identifier = new(@"(?<![\w$.])[A-Za-z_$][\w$]*", System.Text.RegularExpressions.RegexOptions.Compiled);

    /// <summary>
    /// WMP resolved global names case-insensitively (e.g. a skin calling UpdateMetaData() when it
    /// defines UpdateMetadata()). For each identifier the skin's code uses that differs only by case
    /// from a defined global, add an alias property that forwards to the real one.
    /// </summary>
    public void AddCaseAliases(IEnumerable<string> sources)
    {
        var global = Engine.Global;
        var defined = global.GetOwnPropertyKeys(Types.String).Select(k => k.ToString()).ToList();
        var exact = new HashSet<string>(defined, StringComparer.Ordinal);
        var byLower = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
        foreach (var d in defined) byLower.TryAdd(d, d);
        var alias = Engine.Evaluate("(function(g,a,t){ Object.defineProperty(g,a,{get:function(){return g[t];},set:function(v){g[t]=v;},configurable:true}); })");
        var done = new HashSet<string>(StringComparer.Ordinal);
        foreach (var src in sources)
            foreach (System.Text.RegularExpressions.Match m in Identifier.Matches(src))
            {
                string id = m.Value;
                if (exact.Contains(id) || !done.Add(id)) continue;
                if (!byLower.TryGetValue(id, out var target) || target == id) continue;
                try { Engine.Call(alias, JsValue.Undefined, new JsValue[] { global, new JsString(id), new JsString(target) }); }
                catch { }
            }
    }

    public JsValue GetGlobal(string name)
    {
        try { return Engine.GetValue(name); } catch { return JsValue.Undefined; }
    }

    public bool HasGlobalFunction(string name)
    {
        var v = GetGlobal(name);
        return v is Jint.Native.Function.Function;
    }

    public object? CallGlobal(string name, params object?[] args)
    {
        var fn = GetGlobal(name);
        if (fn is not Jint.Native.Function.Function) return null;
        try { return FromJs(Engine.Call(fn, JsValue.Undefined, args.Select(ToJs).ToArray())); }
        catch (Exception ex) { Report(name, ex); return null; }
    }
}

/// <summary>JS view of a <see cref="ScriptObject"/>: case-insensitive get/set, methods as functions.</summary>
public sealed class JsHostObject : ObjectInstance
{
    readonly JsBridge _bridge;
    readonly Dictionary<string, JsValue> _methods = new(StringComparer.OrdinalIgnoreCase);
    public ScriptObject Target { get; }

    public JsHostObject(JsBridge bridge, ScriptObject target) : base(bridge.Engine)
    {
        _bridge = bridge;
        Target = target;
    }

    protected override ObjectInstance GetPrototypeOf() => _bridge.Engine.Intrinsics.Object.PrototypeObject;

    static string? Name(JsValue p) => p.IsString() ? p.AsString() : null;

    JsValue MethodFunction(string name, ScriptMethod m)
    {
        if (_methods.TryGetValue(name, out var fn)) return fn;
        fn = new ClrFunction(_bridge.Engine, name, (thisObj, args) =>
        {
            var conv = new object?[args.Length];
            for (int i = 0; i < args.Length; i++) conv[i] = _bridge.FromJs(args[i]);
            return _bridge.ToJs(m(conv));
        });
        _methods[name] = fn;
        return fn;
    }

    public override JsValue Get(JsValue property, JsValue receiver)
    {
        string? name = Name(property);
        if (name != null)
        {
            if (Target.TryGetMethod(name, out var m)) return MethodFunction(name, m);
            if (Target.HasMember(name)) return _bridge.ToJs(Target.Get(name));
        }
        return base.Get(property, receiver);
    }

    public override bool Set(JsValue property, JsValue value, JsValue receiver)
    {
        string? name = Name(property);
        if (name == null) return base.Set(property, value, receiver);
        Target.Set(name, _bridge.FromJs(value));
        return true;
    }

    public override bool HasProperty(JsValue property)
    {
        string? name = Name(property);
        if (name != null && (Target.HasMember(name) || Target.TryGetMethod(name, out _))) return true;
        return base.HasProperty(property);
    }

    public override PropertyDescriptor GetOwnProperty(JsValue property)
    {
        string? name = Name(property);
        if (name != null)
        {
            if (Target.TryGetMethod(name, out var m)) return new PropertyDescriptor(MethodFunction(name, m), true, false, true);
            if (Target.HasMember(name)) return new PropertyDescriptor(_bridge.ToJs(Target.Get(name)), true, true, true);
        }
        return base.GetOwnProperty(property);
    }

    public override bool Delete(JsValue property) => true;

    public override List<JsValue> GetOwnPropertyKeys(Types types = Types.String | Types.Symbol)
    {
        var keys = base.GetOwnPropertyKeys(types);
        if ((types & Types.String) != 0)
            foreach (var n in Target.MemberNames) keys.Add(new JsString(n));
        return keys;
    }

    public override object ToObject() => Target;
}
