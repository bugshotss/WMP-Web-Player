using System.Text.Json;
using System.Text.RegularExpressions;

namespace WebSkinPlayer.Skins;

/// <summary>
/// Who published each skin (the brand it was made for: Microsoft, Disney, Warner Bros...) and who designed it
/// (The Skins Factory, SkinWerkz...). Both come from the copyright and author a skin carries, through the rules
/// in SkinCatalog.json; its "skins" list corrects the skins whose own text says too little or misleads.
/// </summary>
public static class SkinCatalog
{
    public const string Independent = "Independent artists";
    public const string Other = "Other";

    sealed record Rule(Regex Match, string Name);

    static readonly Dictionary<string, (string Publisher, string? Designer)> Known = new();
    static readonly List<Rule> Publishers = new(), Designers = new();

    static SkinCatalog()
    {
        try
        {
            using var stream = typeof(SkinCatalog).Assembly.GetManifestResourceStream("WebSkinPlayer.Skins.SkinCatalog.json");
            if (stream == null) return;
            using var doc = JsonDocument.Parse(stream, new JsonDocumentOptions { CommentHandling = JsonCommentHandling.Skip, AllowTrailingCommas = true });
            var root = doc.RootElement;
            foreach (var r in root.GetProperty("publishers").EnumerateArray()) Publishers.Add(new Rule(new Regex(r[0].GetString()!, RegexOptions.IgnoreCase), r[1].GetString()!));
            foreach (var r in root.GetProperty("designers").EnumerateArray()) Designers.Add(new Rule(new Regex(r[0].GetString()!, RegexOptions.IgnoreCase), r[1].GetString()!));
            foreach (var p in root.GetProperty("skins").EnumerateObject())
                Known[p.Name] = (p.Value[0].GetString() ?? Other, p.Value.GetArrayLength() > 1 ? p.Value[1].GetString() : null);
        }
        catch (Exception ex) { Log.Write("Skin catalog: " + ex.Message); }
    }

    /// <summary>
    /// "Kenwood_MPXP.wmz", "Plus! Mecha" and "Alienware_Invader_by_The_Skins_Factory" become "kenwood", "mecha"
    /// and "alienwareinvader", so the different builds of one skin share an entry.
    /// </summary>
    public static string Key(string fileOrTitle)
    {
        string t = System.IO.Path.GetFileNameWithoutExtension(fileOrTitle).ToLowerInvariant();
        t = Regex.Replace(t, @"\(.*?\)", " ");
        t = Regex.Replace(t, @"^plus!?_?\s*", "");
        t = Regex.Replace(t, @"^the[ _]", "");
        t = t.Replace("_by_the_skins_factory", "");
        t = Regex.Replace(t, @"[ _-](mp7|mpxp|mp71|wmp1[01]|wmp[789]|v\d+(\.\d+)*)\s*$", "");
        return Regex.Replace(t, "[^a-z0-9]", "");
    }

    public static (string Publisher, string? Designer) Lookup(string file, string author = "", string copyright = "")
    {
        string? designer = Match(Designers, author) ?? Match(Designers, copyright) ?? Person(author);
        if (Known.TryGetValue(Key(file), out var known)) return (known.Publisher, known.Designer ?? designer);
        string? publisher = Match(Publishers, copyright) ?? Match(Publishers, author);
        if (publisher != null) return (publisher, designer);
        return designer != null ? (Independent, designer) : (Other, null);
    }

    static string? Match(List<Rule> rules, string text)
    {
        if (string.IsNullOrWhiteSpace(text)) return null;
        foreach (var rule in rules) if (rule.Match.IsMatch(text)) return rule.Name;
        return null;
    }

    /// <summary>An author line that names a person: "Designer: Joe Walker" -> "Joe Walker"; placeholders and URLs dropped.</summary>
    static string? Person(string author)
    {
        string a = author.Trim();
        if (a.Length == 0 || a.StartsWith("res:", StringComparison.OrdinalIgnoreCase) || a.Contains('@')) return null;
        a = Regex.Replace(a, @"^(designers?|design)\s*:\s*", "", RegexOptions.IgnoreCase);
        a = Regex.Replace(a, @"\s*[-/,(:].*$", "").Trim();
        return a.Length is > 0 and < 60 ? a : null;
    }

    /// <summary>Sorts publishers by name, with independent artists and unknown skins last.</summary>
    public static string SortKey(string publisher) => publisher switch
    {
        Independent => "￾",
        Other => "￿",
        _ => publisher,
    };
}
