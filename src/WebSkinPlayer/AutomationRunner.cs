using System.IO;
using System.Windows;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using System.Windows.Threading;
using WebSkinPlayer.Skins;

namespace WebSkinPlayer;

/// <summary>
/// Developer test hook: "--automate script.txt" replays input against the active skin view
/// through the same code paths real mouse input uses. One command per line (view coordinates):
///   skin &lt;path&gt; | click x y | rclick x y | move x y | drag x1 y1 x2 y2 | wheel x y delta | key Name
///   wait ms | capture file.png | eval js-expression | vis id preset | fullmode | exit
/// Results go to the app log.
/// </summary>
public static class AutomationRunner
{
    public static async void Run(string scriptPath)
    {
        var lines = File.ReadAllLines(scriptPath);
        foreach (var raw in lines)
        {
            string line = raw.Trim();
            if (line.Length == 0 || line.StartsWith('#')) continue;
            var parts = line.Split(' ', 2);
            string cmd = parts[0].ToLowerInvariant();
            string arg = parts.Length > 1 ? parts[1] : "";
            double[] n = arg.Split(' ', StringSplitOptions.RemoveEmptyEntries).Select(s => double.TryParse(s, System.Globalization.NumberStyles.Float, System.Globalization.CultureInfo.InvariantCulture, out var d) ? d : 0).ToArray();
            var view = App.Modes.CurrentTheme?.ActiveView;
            try
            {
                switch (cmd)
                {
                    case "skin": App.Modes.ShowSkin(arg); break;
                    case "fullmode": App.Modes.ShowFullMode(); break;
                    case "wait": await Task.Delay((int)n[0]); break;
                    case "move": view?.HandleMouseMove(new Point(n[0], n[1])); break;
                    case "click":
                        if (view == null) break;
                        view.HandleMouseMove(new Point(n[0], n[1]));
                        bool taken = view.HandleMouseDown(new Point(n[0], n[1]), MouseButton.Left, 1);
                        await Task.Delay(60);
                        view.HandleMouseUp(new Point(n[0], n[1]), MouseButton.Left);
                        Log.Write($"[auto] click {n[0]},{n[1]} taken={taken}");
                        break;
                    case "rclick":
                        view?.HandleMouseDown(new Point(n[0], n[1]), MouseButton.Right, 1);
                        break;
                    case "drag":
                        if (view == null) break;
                        view.HandleMouseMove(new Point(n[0], n[1]));
                        view.HandleMouseDown(new Point(n[0], n[1]), MouseButton.Left, 1);
                        for (int i = 1; i <= 10; i++)
                        {
                            await Task.Delay(30);
                            view.HandleMouseMove(new Point(n[0] + (n[2] - n[0]) * i / 10, n[1] + (n[3] - n[1]) * i / 10));
                        }
                        view.HandleMouseUp(new Point(n[2], n[3]), MouseButton.Left);
                        Log.Write($"[auto] drag {n[0]},{n[1]} -> {n[2]},{n[3]}");
                        break;
                    case "wheel": view?.HandleMouseWheel(new Point(n[0], n[1]), (int)n[2]); break;
                    case "key":
                        if (view != null && Enum.TryParse<Key>(arg, true, out var k)) view.HandleKeyDown(k);
                        break;
                    case "eval":
                        if (view == null) break;
                        var v = view.Bridge.EvaluateIn(view, arg, "automation");
                        Log.Write($"[auto] eval {arg} = {Skins.Scripting.ScriptObject.ToStr(v)}");
                        break;
                    case "vis":
                        App.Services.Visualizations.SetEffect(parts.Length > 1 ? arg.Split(' ')[0] : null, n.Length > 1 ? (int)n[1] : 0);
                        break;
                    case "capture":
                        if (view != null) Capture(view, arg);
                        break;
                    case "capturefull":
                        if (App.Modes.FullWindow?.Content is FrameworkElement root)
                        {
                            var dpi = VisualTreeHelper.GetDpi(root);
                            var rtb = new RenderTargetBitmap((int)(root.ActualWidth * dpi.DpiScaleX), (int)(root.ActualHeight * dpi.DpiScaleY), dpi.PixelsPerInchX, dpi.PixelsPerInchY, PixelFormats.Pbgra32);
                            rtb.Render(root);
                            var enc = new PngBitmapEncoder();
                            enc.Frames.Add(BitmapFrame.Create(rtb));
                            using var fs = File.Create(arg);
                            enc.Save(fs);
                        }
                        break;
                    case "state":
                        Log.Write($"[auto] state {arg}: widgetMode={App.Modes.InWidgetMode} widgetShown={App.Modes.WidgetShown} " +
                                  $"full={(App.Modes.FullWindow?.IsVisible ?? false)} skin={(App.Modes.CurrentTheme?.ActiveView?.Window?.IsVisible ?? false)}");
                        break;
                    case "widget": App.Modes.CollapseToWidget(); break;
                    case "restore": App.Modes.RestoreFromWidget(null); break;
                    case "tab":
                        App.Modes.FullWindow?.SelectTab(arg);
                        break;
                    case "uiclick": // uiclick ButtonName (full mode)
                        if (App.Modes.FullWindow?.FindName(arg) is System.Windows.Controls.Primitives.ButtonBase b)
                            b.RaiseEvent(new RoutedEventArgs(System.Windows.Controls.Primitives.ButtonBase.ClickEvent));
                        break;
                    case "uiselect": // uiselect ListName item text
                    {
                        var sel = arg.Split(' ', 2);
                        if (sel.Length == 2 && App.Modes.FullWindow?.FindName(sel[0]) is System.Windows.Controls.Primitives.Selector list)
                            list.SelectedItem = list.Items.Cast<object>().FirstOrDefault(i => i.ToString()!.Trim().StartsWith(sel[1], StringComparison.OrdinalIgnoreCase));
                        break;
                    }
                    case "exit":
                        App.Modes.Exit();
                        return;
                }
            }
            catch (Exception ex) { Log.Write($"[auto] {line} failed: {ex.Message}"); }
            await Dispatcher.Yield(DispatcherPriority.Background);
        }
    }

    static void Capture(SkinView view, string path)
    {
        var dv = new DrawingVisual();
        using (var dc = dv.RenderOpen()) view.RenderTree(dc);
        var rtb = new RenderTargetBitmap(Math.Max(1, view.Width), Math.Max(1, view.Height), 96, 96, PixelFormats.Pbgra32);
        rtb.Render(dv);
        var enc = new PngBitmapEncoder();
        enc.Frames.Add(BitmapFrame.Create(rtb));
        using var fs = File.Create(path);
        enc.Save(fs);
        Log.Write($"[auto] captured {path}");
    }
}
