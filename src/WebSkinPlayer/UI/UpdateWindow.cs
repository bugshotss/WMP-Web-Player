using System.Windows;
using System.Windows.Controls;
using System.Windows.Media;
using WebSkinPlayer.Skins;

namespace WebSkinPlayer.UI;

/// <summary>Downloads an update with a progress bar, then hands over to its installer.</summary>
public sealed class UpdateWindow : Window
{
    readonly UpdateInfo _update;
    readonly TextBlock _status = new() { Margin = new Thickness(0, 0, 0, 10), TextWrapping = TextWrapping.Wrap };
    readonly ProgressBar _bar = new() { Height = 10, Minimum = 0, Maximum = 1 };
    readonly CancellationTokenSource _cancel = new();
    static UpdateWindow? s_open;

    readonly bool _quiet;

    /// <summary>Asks, then updates; a copy the installer can't update (portable) gets the release page instead.</summary>
    /// <param name="quiet">Automation: no question, and a failure is only logged.</param>
    public static void Start(UpdateInfo update, Window? owner, bool quiet = false)
    {
        if (s_open != null) { s_open.Activate(); return; }
        if (!UpdateChecker.CanInstall(update))
        {
            if (quiet) Log.Write("Update: this copy wasn't installed by the installer");
            else WmpObjects.OpenInBrowser(update.PageUrl);
            return;
        }
        string ask = $"Install WebSkin Player {update.Version} now?\n\nThe player closes, updates and starts again. Your skins and settings are kept.";
        if (!quiet && (owner != null
                ? MessageBox.Show(owner, ask, "WebSkin Player update", MessageBoxButton.YesNo, MessageBoxImage.Question)
                : MessageBox.Show(ask, "WebSkin Player update", MessageBoxButton.YesNo, MessageBoxImage.Question)) != MessageBoxResult.Yes) return;
        s_open = new UpdateWindow(update, quiet);
        if (owner is { IsVisible: true }) s_open.Owner = owner;
        s_open.Show();
    }

    UpdateWindow(UpdateInfo update, bool quiet)
    {
        _update = update;
        _quiet = quiet;
        Title = "WebSkin Player update";
        Width = 380;
        SizeToContent = SizeToContent.Height;
        ResizeMode = ResizeMode.NoResize;
        WindowStartupLocation = WindowStartupLocation.CenterScreen;
        Topmost = App.Services.Settings.AlwaysOnTop;
        try { Icon = System.Windows.Media.Imaging.BitmapFrame.Create(new Uri("pack://application:,,,/app.ico")); } catch { }
        _status.Text = $"Downloading WebSkin Player {update.Version}…";
        var cancel = new Button { Content = "Cancel", Padding = new Thickness(16, 3, 16, 3), Margin = new Thickness(0, 14, 0, 0), HorizontalAlignment = HorizontalAlignment.Right };
        cancel.Click += (_, _) => Close();
        var panel = new StackPanel { Margin = new Thickness(18) };
        panel.Children.Add(_status);
        panel.Children.Add(_bar);
        panel.Children.Add(cancel);
        Content = panel;
        Background = SystemColors.WindowBrush;
        Loaded += async (_, _) => await Run(cancel);
        Closed += (_, _) => { _cancel.Cancel(); s_open = null; };
    }

    async Task Run(Button cancel)
    {
        try
        {
            var progress = new Progress<double>(p =>
            {
                _bar.Value = p;
                _status.Text = $"Downloading WebSkin Player {_update.Version}… {p:P0}";
            });
            string installer = await UpdateChecker.DownloadAsync(_update, progress, _cancel.Token);
            _status.Text = "Installing… WebSkin Player starts again in a moment.";
            cancel.IsEnabled = false;
            UpdateChecker.Install(installer);
        }
        catch (OperationCanceledException) { }
        catch (Exception ex)
        {
            Log.Write("Update failed: " + ex.Message);
            Hide();
            if (!_quiet && MessageBox.Show($"The update couldn't be installed: {ex.Message}.\n\nOpen the download page instead?", "WebSkin Player update",
                    MessageBoxButton.YesNo, MessageBoxImage.Warning) == MessageBoxResult.Yes)
                WmpObjects.OpenInBrowser(_update.PageUrl);
            Close();
        }
    }
}
