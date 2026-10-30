using System.Windows;
using System.Windows.Media;
using WebSkinPlayer.Skins.Scripting;

namespace WebSkinPlayer.Skins.Elements;

/// <summary>SUBVIEW: a container with an optional background colour/image and clipping shape.</summary>
public class SubviewElement : SkinElement
{
    static readonly Dictionary<string, PType> S = new(StringComparer.OrdinalIgnoreCase)
    {
        ["backgroundcolor"] = PType.String, ["backgroundimage"] = PType.String, ["backgroundtiled"] = PType.Bool,
        ["resizebackgroundimage"] = PType.Bool, ["resizeimages"] = PType.Bool,
    };
    protected override Dictionary<string, PType>? Schema => S;

    public SubviewElement(SkinView? view, WmsNode node) : base(view, node) { }

    public override bool ClipsChildren => true;

    // A VIEW keeps its clippingColor pixels here: they are keyed out of the composed window
    // (so backgroundColor shows only through transparencyColor holes, as in WMP).
    SkinBitmap? Background =>
        View.Theme.Images.Get(GetString("backgroundimage"),
            SkinImages.ParseColor(GetString("transparencycolor")),
            GetNumber("hueshift"), GetNumber("saturation"),
            GetString("clippingimage").Length == 0 && (this is not SkinView || GetBool("titlebar")) ? SkinImages.ParseColor(GetString("clippingcolor")) : null);

    public override int NaturalWidth => Background?.Width ?? 0;
    public override int NaturalHeight => Background?.Height ?? 0;

    protected override string PrimaryImage => GetString("backgroundimage");
    public override bool MaskTiled => GetBool("backgroundtiled") && GetString("clippingimage").Length == 0;

    public override bool HitTest(Point local)
    {
        if (!base.HitTest(local)) return false;
        if (!MaskAllows(local)) return false;
        // Transparent background with no colour isn't hit (lets clicks reach the window behind).
        var bg = Background;
        bool hasColor = SkinImages.ParseColor(GetString("backgroundcolor")) != null;
        if (bg != null && !hasColor && !GetBool("backgroundtiled") && !GetBool("resizebackgroundimage"))
            return bg.Alpha((int)local.X, (int)local.Y) > 0;
        return true;
    }

    public override void RenderBackdrop(DrawingContext dc, Rect b)
    {
        int? color = SkinImages.ParseColor(GetString("backgroundcolor"));
        if (color == null && Values.TryGetValue("backgoundcolor", out var typo)) color = SkinImages.ParseColor(ToStr(typo));
        if (color != null)
            dc.DrawRectangle(new SolidColorBrush(Color.FromRgb((byte)(color >> 16), (byte)(color >> 8), (byte)color)), null, b);
    }

    public override void Render(DrawingContext dc, Rect b)
    {
        var bg = Background;
        if (bg == null) return;
        var src = FrameOf(bg, GetString("backgroundimage"));
        if (GetBool("resizebackgroundimage") || GetBool("resizeimages") || SkinImages.IsDynamic(GetString("backgroundimage")))
            dc.DrawImage(src, b);
        else if (GetBool("backgroundtiled"))
        {
            for (double y = b.Top; y < b.Bottom; y += bg.Height)
                for (double x = b.Left; x < b.Right; x += bg.Width)
                    dc.DrawImage(src, new Rect(x, y, bg.Width, bg.Height));
        }
        else
            dc.DrawImage(src, new Rect(b.X, b.Y, bg.Width, bg.Height));
    }
}
