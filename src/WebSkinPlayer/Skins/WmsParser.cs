using System.Net;
using System.Text;

namespace WebSkinPlayer.Skins;

/// <summary>One element of a .wms skin definition. Tag is upper-case; attribute names keep their spelling.</summary>
public sealed class WmsNode
{
    public string Tag { get; init; } = "";
    public int Line { get; init; }
    public List<KeyValuePair<string, string>> Attributes { get; } = new();
    public List<WmsNode> Children { get; } = new();
    public WmsNode? Parent { get; set; }

    public string? Attr(string name)
    {
        for (int i = Attributes.Count - 1; i >= 0; i--)
            if (Attributes[i].Key.Equals(name, StringComparison.OrdinalIgnoreCase)) return Attributes[i].Value;
        return null;
    }

    public override string ToString() => $"<{Tag}> line {Line}";
}

/// <summary>
/// Forgiving parser for the XML-like .wms format: case-insensitive tags, unquoted attribute
/// values, stray '&amp;', mismatched or missing close tags are all tolerated as WMP did.
/// </summary>
public static class WmsParser
{
    public static WmsNode Parse(string text)
    {
        var root = new WmsNode { Tag = "#ROOT" };
        var stack = new Stack<WmsNode>();
        stack.Push(root);
        int i = 0, n = text.Length, line = 1;

        while (i < n)
        {
            int lt = text.IndexOf('<', i);
            if (lt < 0) break;
            line += Count(text, i, lt, '\n');
            i = lt;

            if (Starts(text, i, "<!--"))
            {
                int end = text.IndexOf("-->", i + 4, StringComparison.Ordinal);
                end = end < 0 ? n : end + 3;
                line += Count(text, i, end, '\n');
                i = end;
                continue;
            }
            if (Starts(text, i, "<![CDATA["))
            {
                int end = text.IndexOf("]]>", i, StringComparison.Ordinal);
                i = end < 0 ? n : end + 3;
                continue;
            }
            if (Starts(text, i, "<?") || Starts(text, i, "<!"))
            {
                int end = text.IndexOf('>', i);
                i = end < 0 ? n : end + 1;
                continue;
            }
            if (Starts(text, i, "</"))
            {
                int end = text.IndexOf('>', i);
                string name = text.Substring(i + 2, (end < 0 ? n : end) - i - 2).Trim().ToUpperInvariant();
                i = end < 0 ? n : end + 1;
                // Pop to the matching open tag if there is one; otherwise ignore the stray close.
                if (stack.Any(s => s.Tag == name))
                    while (stack.Count > 1) { var top = stack.Pop(); if (top.Tag == name) break; }
                continue;
            }

            // Start tag.
            int p = i + 1;
            int nameStart = p;
            while (p < n && (char.IsLetterOrDigit(text[p]) || text[p] == '_' || text[p] == ':' || text[p] == '-' || text[p] == '.')) p++;
            if (p == nameStart) { i++; continue; }
            var node = new WmsNode { Tag = text[nameStart..p].ToUpperInvariant(), Line = line };
            bool selfClose = false;

            while (p < n)
            {
                while (p < n && char.IsWhiteSpace(text[p])) { if (text[p] == '\n') line++; p++; }
                if (p >= n) break;
                if (text[p] == '>') { p++; break; }
                if (text[p] == '/' && p + 1 < n && text[p + 1] == '>') { selfClose = true; p += 2; break; }
                if (text[p] == '/') { p++; continue; }

                int an = p;
                while (p < n && !char.IsWhiteSpace(text[p]) && text[p] != '=' && text[p] != '>' && !(text[p] == '/' && p + 1 < n && text[p + 1] == '>')) p++;
                string attrName = text[an..p];
                while (p < n && char.IsWhiteSpace(text[p])) { if (text[p] == '\n') line++; p++; }
                string value = "";
                if (p < n && text[p] == '=')
                {
                    p++;
                    while (p < n && char.IsWhiteSpace(text[p])) { if (text[p] == '\n') line++; p++; }
                    if (p < n && (text[p] == '"' || text[p] == '\''))
                    {
                        char q = text[p++];
                        int vs = p;
                        int ve = text.IndexOf(q, p);
                        if (ve < 0) ve = n;
                        value = text[vs..ve];
                        line += Count(text, vs, ve, '\n');
                        p = Math.Min(n, ve + 1);
                    }
                    else
                    {
                        int vs = p;
                        while (p < n && !char.IsWhiteSpace(text[p]) && text[p] != '>') p++;
                        if (p > vs && text[p - 1] == '/' && p < n && text[p] == '>') p--;
                        value = text[vs..p];
                    }
                }
                if (attrName.Length > 0) node.Attributes.Add(new(attrName, Decode(value)));
            }
            i = p;

            var parent = stack.Peek();
            node.Parent = parent;
            parent.Children.Add(node);
            if (!selfClose) stack.Push(node);
        }
        return root;
    }

    static bool Starts(string s, int i, string prefix) => string.CompareOrdinal(s, i, prefix, 0, prefix.Length) == 0;

    static int Count(string s, int from, int to, char c)
    {
        int k = 0;
        for (int j = from; j < to && j < s.Length; j++) if (s[j] == c) k++;
        return k;
    }

    static string Decode(string v) => v.IndexOf('&') < 0 ? v : WebUtility.HtmlDecode(v);
}
