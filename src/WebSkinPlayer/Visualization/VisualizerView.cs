using System.Windows;
using System.Windows.Media;
using WebSkinPlayer.Audio;

namespace WebSkinPlayer.Visualization;

/// <summary>WPF element that shows the globally selected visualization (full-mode Now Playing).</summary>
public sealed class VisualizerView : FrameworkElement
{
    readonly VisualizerRenderer _renderer = new() { MaxPixels = 1280 * 720 };
    VisualizationService? _service;

    public VisualizerView()
    {
        Loaded += (_, _) => Attach();
        Unloaded += (_, _) => Detach();
        IsVisibleChanged += (_, _) => { if (IsVisible) Attach(); else Detach(); };
        _renderer.BitmapChanged += InvalidateVisual;
        RenderOptions.SetBitmapScalingMode(this, BitmapScalingMode.Linear);
    }

    void Attach()
    {
        if (_service != null || !IsVisible) return;
        _service = App.Services.Visualizations;
        _service.EffectChanged += OnEffectChanged;
        OnEffectChanged();
        _service.Subscribe(OnFrame);
    }

    void Detach()
    {
        if (_service == null) return;
        _service.Unsubscribe(OnFrame);
        _service.EffectChanged -= OnEffectChanged;
        _service = null;
    }

    void OnEffectChanged()
    {
        if (_service == null) return;
        _renderer.SetEffect(_service.Current, _service.Preset);
    }

    void OnFrame(VisFrame frame)
    {
        if (_service == null || ActualWidth < 1 || ActualHeight < 1) return;
        var dpi = VisualTreeHelper.GetDpi(this);
        _renderer.Resize((int)(ActualWidth * dpi.DpiScaleX), (int)(ActualHeight * dpi.DpiScaleY));
        _renderer.Render(frame, _service.Context);
    }

    protected override void OnRender(DrawingContext dc)
    {
        dc.DrawRectangle(Brushes.Black, null, new Rect(RenderSize));
        if (_renderer.Bitmap != null) dc.DrawImage(_renderer.Bitmap, new Rect(RenderSize));
    }

    protected override void OnRenderSizeChanged(SizeChangedInfo sizeInfo)
    {
        base.OnRenderSizeChanged(sizeInfo);
        InvalidateVisual();
    }
}
