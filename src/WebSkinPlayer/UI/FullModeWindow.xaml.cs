using System.Runtime.InteropServices;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Data;
using System.Windows.Input;
using System.Windows.Interop;
using System.Windows.Media.Imaging;
using System.Windows.Threading;
using Microsoft.Win32;
using WebSkinPlayer.Media;
using WebSkinPlayer.Skins;
using WebSkinPlayer.Visualization;

namespace WebSkinPlayer.UI;

public partial class FullModeWindow : Window
{
    readonly DispatcherTimer _timer = new() { Interval = TimeSpan.FromMilliseconds(250) };
    bool _seeking, _updating;
    IReadOnlyList<SkinEntry> _allSkins = Array.Empty<SkinEntry>();
    int _previewVersion;

    static Services S => App.Services;

    public FullModeWindow()
    {
        InitializeComponent();
        Topmost = S.Settings.AlwaysOnTop;
        var place = S.Settings.FullMode;
        if (!double.IsNaN(place.Width)) { Width = place.Width; Height = place.Height; }
        if (!double.IsNaN(place.Left)) { WindowStartupLocation = WindowStartupLocation.Manual; Left = place.Left; Top = place.Top; }

        SourceInitialized += (_, _) => UseDarkTitleBar(new WindowInteropHelper(this).Handle);
        Loaded += (_, _) =>
        {
            S.Media.Changed += UpdateMedia;
            S.Player.PropertyChanged += OnPlayerProperty;
            S.Visualizations.EffectChanged += UpdateEffectBoxes;
            S.Skins.Changed += RefreshSkins;
            UpdateChecker.Found += ShowUpdate;
            App.Modes.AlwaysOnTopChanged += UpdateOnTop;
            UpdateOnTop();
            UpdateMedia();
            UpdateEffectBoxes();
            RefreshSources();
            ShowUpdate();
            _timer.Tick += (_, _) => UpdatePosition();
            _timer.Start();
            // A new machine has no skins yet: start where they can be fetched.
            if (S.Skins.Scan().Count == 0) { TabSkins.IsChecked = true; SkinsOnline.IsChecked = true; }
        };
        Closing += (_, _) =>
        {
            S.Settings.FullMode = new WindowPlacement { Left = RestoreBounds.Left, Top = RestoreBounds.Top, Width = RestoreBounds.Width, Height = RestoreBounds.Height };
            S.Media.Changed -= UpdateMedia;
            S.Player.PropertyChanged -= OnPlayerProperty;
            S.Visualizations.EffectChanged -= UpdateEffectBoxes;
            S.Skins.Changed -= RefreshSkins;
            UpdateChecker.Found -= ShowUpdate;
            App.Modes.AlwaysOnTopChanged -= UpdateOnTop;
            _downloadCts?.Cancel();
            _timer.Stop();
        };
        PreviewKeyDown += OnKey;
    }

    void OnKey(object sender, KeyEventArgs e)
    {
        bool ctrl = Keyboard.Modifiers.HasFlag(ModifierKeys.Control);
        bool alt = Keyboard.Modifiers.HasFlag(ModifierKeys.Alt);
        if (e.Key == Key.Escape && _fullScreen) { ToggleFullScreen(); e.Handled = true; }
        else if (alt && (e.Key == Key.Enter || e.SystemKey == Key.Enter) && NowPlayingPanel.Visibility == Visibility.Visible) { ToggleFullScreen(); e.Handled = true; }
        else if (ctrl && e.Key == Key.D2) { OnSkinMode(this, e); e.Handled = true; }
        else if (ctrl && e.Key == Key.D3) { App.Modes.CollapseToWidget(); e.Handled = true; }
        else if (ctrl && e.Key == Key.P) { S.Player.TogglePlayPause(); e.Handled = true; }
        else if (ctrl && e.Key == Key.F) { S.Player.Next(); e.Handled = true; }
        else if (ctrl && e.Key == Key.B) { S.Player.Previous(); e.Handled = true; }
        else if (e.Key == Key.F8) { S.Player.Mute = !S.Player.Mute; e.Handled = true; }
        else if (e.Key == Key.F9) { S.Player.Volume -= 5; e.Handled = true; }
        else if (e.Key == Key.F10) { S.Player.Volume += 5; e.Handled = true; }
    }

    // ---- media -------------------------------------------------------------------------

    void UpdateMedia()
    {
        var m = S.Media.Current;
        NowTitle.Text = m.Title;
        NowArtist.Text = m.Album.Length > 0 && m.Artist.Length > 0 ? $"{m.Artist} — {m.Album}" : m.Artist;
        BarTitle.Text = m.HasSession ? (m.Title.Length > 0 ? m.Title : m.SourceDisplayName) : "Nothing playing";
        BarStatus.Text = m.HasSession ? $"{S.Player.Status} · {m.SourceDisplayName}" : "Waiting for a browser or app";
        NoSessionHint.Visibility = m.HasSession ? Visibility.Collapsed : Visibility.Visible;
        ArtThumb.Source = m.Artwork;
        bool playing = m.Status == MediaStatus.Playing;
        PlayBtn.Content = playing ? "" : "";
        PlayBtn.ToolTip = playing ? "Pause" : "Play";
        PlayBtn.IsEnabled = m.HasSession;
        PrevBtn.IsEnabled = m.CanPrevious;
        NextBtn.IsEnabled = m.CanNext;
        StopBtn.IsEnabled = m.HasSession;
        SeekBar.IsEnabled = m.CanSeek;
        DurationText.Text = m.Duration > TimeSpan.Zero ? PlayerModel.FormatTime(m.Duration.TotalSeconds) : "";
        Title = m.HasSession && m.Title.Length > 0 ? $"{m.Title} - WebSkin Player" : "WebSkin Player";
        UpdatePosition();
        UpdateVolume();
    }

    void OnPlayerProperty(string obj, string prop)
    {
        if (obj == "settings") UpdateVolume();
    }

    void UpdateVolume()
    {
        _updating = true;
        VolumeBar.Value = S.Player.Volume;
        MuteBtn.Content = S.Player.Mute ? "" : "";
        _updating = false;
    }

    void UpdatePosition()
    {
        if (_seeking) return;
        var m = S.Media.Current;
        double dur = m.Duration.TotalSeconds;
        PositionText.Text = m.HasSession ? PlayerModel.FormatTime(S.Player.Position) : "";
        _updating = true;
        SeekBar.Maximum = dur > 0 ? dur : 1;
        SeekBar.Value = dur > 0 ? Math.Min(S.Player.Position, dur) : 0;
        _updating = false;
    }

    void OnPlayPause(object sender, RoutedEventArgs e) => S.Player.TogglePlayPause();
    void OnStop(object sender, RoutedEventArgs e) => S.Player.Stop();
    void OnPrev(object sender, RoutedEventArgs e) => S.Player.Previous();
    void OnNext(object sender, RoutedEventArgs e) => S.Player.Next();
    void OnMute(object sender, RoutedEventArgs e) => S.Player.Mute = !S.Player.Mute;
    void OnVolumeChanged(object sender, RoutedPropertyChangedEventArgs<double> e)
    {
        if (!_updating) S.Player.Volume = (int)e.NewValue;
    }
    void OnSeekStart(object sender, System.Windows.Controls.Primitives.DragStartedEventArgs e) => _seeking = true;
    void OnSeekEnd(object sender, System.Windows.Controls.Primitives.DragCompletedEventArgs e)
    {
        _seeking = false;
        S.Player.SetPosition(SeekBar.Value);
    }
    void OnSeekClick(object sender, MouseButtonEventArgs e)
    {
        if (!_seeking && SeekBar.IsEnabled) S.Player.SetPosition(SeekBar.Value);
    }

    // ---- sources -----------------------------------------------------------------------

    sealed record SourceItem(string? AppId, string Label) { public override string ToString() => Label; }

    void OnSourceDropDown(object? sender, EventArgs e) => RefreshSources();

    void RefreshSources()
    {
        _updating = true;
        var items = new List<SourceItem> { new(null, "Follow what's playing") };
        foreach (var s in S.Media.GetSessions())
            items.Add(new SourceItem(s.AppId, $"{s.DisplayName}{(s.Status == MediaStatus.Playing ? "  ▶" : "")}"));
        if (S.Media.PreferredApp != null && items.All(i => i.AppId != S.Media.PreferredApp))
            items.Add(new SourceItem(S.Media.PreferredApp, MediaBridge.FriendlyAppName(S.Media.PreferredApp) + " (not running)"));
        SourceBox.ItemsSource = items;
        SourceBox.SelectedItem = items.FirstOrDefault(i => i.AppId == S.Media.PreferredApp) ?? items[0];
        _updating = false;
    }

    void OnSourceChanged(object sender, SelectionChangedEventArgs e)
    {
        if (_updating || SourceBox.SelectedItem is not SourceItem item) return;
        S.Media.PreferredApp = item.AppId;
        S.Settings.PreferredApp = item.AppId;
        S.Settings.Save();
        S.Media.Reselect();
    }

    // ---- visualizations ----------------------------------------------------------------

    void UpdateEffectBoxes()
    {
        _updating = true;
        EffectBox.ItemsSource = VisualizationCatalog.All.Select(d => d.Title).ToList();
        var cur = S.Visualizations.Current;
        EffectBox.SelectedItem = cur.Title;
        PresetBox.ItemsSource = cur.Presets;
        PresetBox.SelectedIndex = S.Visualizations.Preset;
        _updating = false;
    }

    void OnEffectSelected(object sender, SelectionChangedEventArgs e)
    {
        if (_updating || EffectBox.SelectedItem is not string title) return;
        S.Visualizations.Choose(VisualizationCatalog.All.First(d => d.Title == title).Id, 0);
    }

    void OnPresetSelected(object sender, SelectionChangedEventArgs e)
    {
        if (_updating || PresetBox.SelectedIndex < 0) return;
        S.Visualizations.Choose(S.Visualizations.EffectId, PresetBox.SelectedIndex);
    }

    void OnPrevEffect(object sender, RoutedEventArgs e) => S.Visualizations.NextEffect(-1);
    void OnNextPreset(object sender, RoutedEventArgs e) => S.Visualizations.NextPreset(1);

    void OnColors(object sender, RoutedEventArgs e)
    {
        var menu = new ContextMenu { PlacementTarget = (UIElement)sender, Placement = System.Windows.Controls.Primitives.PlacementMode.Top };
        foreach (var item in VisualizationColorMenu.Items(this)) menu.Items.Add(item);
        menu.IsOpen = true;
    }

    void OnVisualizerClick(object sender, MouseButtonEventArgs e)
    {
        if (e.ClickCount == 2) ToggleFullScreen();
    }

    void OnVisualizerRightClick(object sender, MouseButtonEventArgs e)
    {
        var menu = new ContextMenu();
        foreach (var d in VisualizationCatalog.All)
        {
            var sub = new MenuItem { Header = d.Title };
            for (int i = 0; i < d.Presets.Count; i++)
            {
                int preset = i;
                var mi = new MenuItem { Header = d.Presets[i], IsCheckable = true, IsChecked = S.Visualizations.EffectId == d.Id && S.Visualizations.Preset == i };
                mi.Click += (_, _) => S.Visualizations.Choose(d.Id, preset);
                sub.Items.Add(mi);
            }
            menu.Items.Add(sub);
        }
        menu.Items.Add(new Separator());
        menu.Items.Add(VisualizationColorMenu.Build(this));
        var onTop = new MenuItem { Header = "Always on Top", IsCheckable = true, IsChecked = App.Modes.AlwaysOnTop };
        onTop.Click += (_, _) => App.Modes.ToggleAlwaysOnTop();
        menu.Items.Add(onTop);
        if (App.ScreenSaver is { } saver) menu.Items.Add(saver.BuildMenu());
        var add = new MenuItem { Header = "Add Visualization…", IsEnabled = PluginLibrary.HostAvailable };
        add.Click += (_, _) => AddVisualization();
        menu.Items.Add(add);
        var fs = new MenuItem { Header = _fullScreen ? "Exit full screen" : "Full screen" };
        fs.Click += (_, _) => ToggleFullScreen();
        menu.Items.Add(fs);
        menu.IsOpen = true;
    }

    /// <summary>Adds WMP visualization plug-ins from a DLL, a zip or an .msi (other installers need unpacking first).</summary>
    void AddVisualization()
    {
        var dlg = new OpenFileDialog { Filter = "WMP visualization plug-ins (*.dll;*.zip;*.msi)|*.dll;*.zip;*.msi", Multiselect = true, Title = "Add visualizations" };
        if (dlg.ShowDialog(this) != true) return;
        Cursor = Cursors.Wait;
        var report = new List<string>();
        try { foreach (var f in dlg.FileNames) report.AddRange(PluginLibrary.Import(f, System.IO.Path.GetFileName(f))); }
        finally { Cursor = null; }
        VisualizationCatalog.Reload();
        UpdateEffectBoxes();
        MessageBox.Show(this, report.Count > 0 ? string.Join(Environment.NewLine, report) : "No WMP visualization was found in that file.",
            "Add visualizations", MessageBoxButton.OK, MessageBoxImage.Information);
    }

    // ---- full screen visualizer (WMP's Alt+Enter) ----------------------------------------

    bool _fullScreen;
    WindowState _restoreState;

    void ToggleFullScreen()
    {
        _fullScreen = !_fullScreen;
        var collapse = _fullScreen ? Visibility.Collapsed : Visibility.Visible;
        HeaderBar.Visibility = collapse;
        TransportArea.Visibility = collapse;
        VisControls.Visibility = collapse;
        if (_fullScreen)
        {
            _restoreState = WindowState;
            WindowStyle = WindowStyle.None;
            WindowState = WindowState.Normal; // re-maximize so the taskbar is covered
            WindowState = WindowState.Maximized;
            Cursor = Cursors.None;
        }
        else
        {
            WindowStyle = WindowStyle.SingleBorderWindow;
            WindowState = _restoreState;
            Cursor = null;
        }
    }

    // ---- skin chooser ------------------------------------------------------------------

    public void SelectTab(string name)
    {
        if (name.StartsWith("skin", StringComparison.OrdinalIgnoreCase)) { TabSkins.IsChecked = true; SkinsInstalled.IsChecked = true; }
        else if (name.Equals("online", StringComparison.OrdinalIgnoreCase)) { TabSkins.IsChecked = true; SkinsOnline.IsChecked = true; }
        else TabNowPlaying.IsChecked = true;
    }

    void OnTabChanged(object sender, RoutedEventArgs e)
    {
        if (NowPlayingPanel == null) return;
        bool skins = TabSkins.IsChecked == true;
        NowPlayingPanel.Visibility = skins ? Visibility.Collapsed : Visibility.Visible;
        SkinsPanel.Visibility = skins ? Visibility.Visible : Visibility.Collapsed;
        if (skins && _allSkins.Count == 0) RefreshSkins();
    }

    void RefreshSkins()
    {
        _allSkins = S.Skins.Scan();
        foreach (var s in _allSkins) S.Skins.EnsureMetadata(s);
        _allSkins = _allSkins.OrderBy(s => s.DisplayName, StringComparer.CurrentCultureIgnoreCase).ToList();
        MarkInstalled();
        ApplySkinFilter();
    }

    void OnSkinSearch(object sender, TextChangedEventArgs e) => ApplySkinFilter();

    sealed record PublisherItem(string? Name, string Label) { public override string ToString() => Label; }
    bool _fillingPublishers;

    void OnPublisherFilter(object sender, SelectionChangedEventArgs e)
    {
        if (!_fillingPublishers && SkinList != null) ApplySkinFilter();
    }

    /// <summary>Lists each publisher with its skin count, keeping the current choice when it still exists.</summary>
    void FillPublishers(IEnumerable<string> publishers)
    {
        string? current = (PublisherFilter.SelectedItem as PublisherItem)?.Name;
        var groups = publishers.GroupBy(p => p).OrderBy(g => SkinCatalog.SortKey(g.Key), StringComparer.CurrentCultureIgnoreCase).ToList();
        var items = new List<PublisherItem> { new(null, $"All publishers ({groups.Sum(g => g.Count())})") };
        items.AddRange(groups.Select(g => new PublisherItem(g.Key, $"{g.Key} ({g.Count()})")));
        _fillingPublishers = true;
        PublisherFilter.ItemsSource = items;
        PublisherFilter.SelectedItem = items.FirstOrDefault(i => i.Name == current) ?? items[0];
        _fillingPublishers = false;
    }

    /// <summary>Items under a header per publisher (alphabetical, independent artists and unknown last), then by name.</summary>
    static ListCollectionView Grouped(System.Collections.IList items)
    {
        var view = new ListCollectionView(items);
        view.GroupDescriptions.Add(new PropertyGroupDescription("Publisher"));
        view.SortDescriptions.Add(new System.ComponentModel.SortDescription("PublisherSort", System.ComponentModel.ListSortDirection.Ascending));
        view.SortDescriptions.Add(new System.ComponentModel.SortDescription("DisplayName", System.ComponentModel.ListSortDirection.Ascending));
        return view;
    }

    static bool Matches(string q, params string?[] fields) =>
        q.Length == 0 || fields.Any(f => f != null && f.Contains(q, StringComparison.CurrentCultureIgnoreCase));

    void ApplySkinFilter()
    {
        string q = SkinSearch.Text.Trim();
        if (OnlineMode)
        {
            var all = _online ?? new List<OnlineSkin>();
            FillPublishers(all.Select(s => s.Publisher));
            string? onlyFrom = (PublisherFilter.SelectedItem as PublisherItem)?.Name;
            var found = all.Where(s => (onlyFrom == null || s.Publisher == onlyFrom) && Matches(q, s.DisplayName, s.Publisher, s.Designer)).ToList();
            var current = SkinList.SelectedItem as OnlineSkin;
            SkinList.ItemsSource = Grouped(found);
            if (_online != null) SkinCount.Text = $"{found.Count} of {all.Count} skins on archive.org · {all.Count(s => !s.Installed)} not installed";
            SkinList.SelectedItem = current != null && found.Contains(current) ? current : found.FirstOrDefault(s => !s.Installed) ?? found.FirstOrDefault();
            if (SkinList.SelectedItem != null) SkinList.ScrollIntoView(SkinList.SelectedItem);
            return;
        }
        FillPublishers(_allSkins.Select(s => s.Publisher));
        string? publisher = (PublisherFilter.SelectedItem as PublisherItem)?.Name;
        var list = _allSkins.Where(s => (publisher == null || s.Publisher == publisher) && Matches(q, s.DisplayName, s.Author, s.Publisher, s.Designer)).ToList();
        var selected = SkinList.SelectedItem as SkinEntry;
        SkinList.ItemsSource = Grouped(list);
        SkinCount.Text = $"{list.Count} of {_allSkins.Count} skins";
        SkinList.SelectedItem = selected != null && list.Contains(selected) ? selected
            : list.FirstOrDefault(s => string.Equals(s.Path, S.Settings.CurrentSkin, StringComparison.OrdinalIgnoreCase)) ?? list.FirstOrDefault();
        if (SkinList.SelectedItem != null) SkinList.ScrollIntoView(SkinList.SelectedItem);
        if (_allSkins.Count == 0)
        {
            SkinName.Text = "No skins installed yet";
            SkinPreviewStatus.Text = "Open Get more skins to download the WMP Skins Archive collection,\nor use Install skin file… for a .wmz you already have.";
        }
    }

    async void OnSkinSelected(object sender, SelectionChangedEventArgs e)
    {
        if (SkinList.SelectedItem is OnlineSkin online)
        {
            ShowOnlinePreview(online);
            return;
        }
        if (SkinList.SelectedItem is not SkinEntry entry)
        {
            SkinName.Text = ""; SkinAuthor.Text = ""; SkinPreview.Source = null; SkinPreviewStatus.Text = "";
            return;
        }
        SkinName.Text = entry.DisplayName;
        SkinAuthor.Text = string.Join("  ·  ", new[] { PublisherLine(entry.Publisher, entry.Designer ?? entry.Author), entry.FileName }.Where(s => s.Length > 0));
        int version = ++_previewVersion;
        SkinPreview.Source = null;
        SkinPreviewStatus.Text = "Rendering preview…";
        await Dispatcher.Yield(DispatcherPriority.Background);
        if (version != _previewVersion) return;
        BitmapSource? preview = null;
        string? error = null;
        try { preview = SkinTheme.RenderPreview(entry.Path); }
        catch (Exception ex) { error = ex.Message; Log.Write($"Preview {entry.FileName}: {ex}"); }
        if (version != _previewVersion) return;
        SkinPreview.Source = preview;
        SkinPreviewStatus.Text = preview == null ? "No preview available" + (error != null ? $"\n{error}" : "") : "";
    }

    void OnSkinDoubleClick(object sender, MouseButtonEventArgs e)
    {
        if (OnlineMode) OnDownloadSkin(sender, e);
        else OnApplySkin(sender, e);
    }

    void OnApplySkin(object sender, RoutedEventArgs e)
    {
        if (SkinList.SelectedItem is SkinEntry entry) App.Modes.ShowSkin(entry.Path);
    }

    void OnInstallSkin(object sender, RoutedEventArgs e)
    {
        var dlg = new OpenFileDialog { Filter = "Windows Media Player skins (*.wmz;*.wms;*.zip)|*.wmz;*.wms;*.zip", Multiselect = true, Title = "Install skins" };
        if (dlg.ShowDialog(this) != true) return;
        string? last = null;
        foreach (var f in dlg.FileNames) last = S.Skins.Install(f) ?? last;
        RefreshSkins();
        if (last != null) SkinList.SelectedItem = _allSkins.FirstOrDefault(s => string.Equals(s.Path, last, StringComparison.OrdinalIgnoreCase));
    }

    void OnOpenSkinsFolder(object sender, RoutedEventArgs e) =>
        System.Diagnostics.Process.Start(new System.Diagnostics.ProcessStartInfo(SkinLibrary.InstallFolder) { UseShellExecute = true });

    // ---- get more skins (archive.org) -------------------------------------------------------

    List<OnlineSkin>? _online;
    CancellationTokenSource? _downloadCts;
    bool _loadingOnline;

    bool OnlineMode => SkinsOnline?.IsChecked == true;

    async void OnSkinSourceChanged(object sender, RoutedEventArgs e)
    {
        if (SkinList == null || InstalledButtons == null) return;
        bool online = OnlineMode;
        InstalledButtons.Visibility = online ? Visibility.Collapsed : Visibility.Visible;
        OnlineButtons.Visibility = online ? Visibility.Visible : Visibility.Collapsed;
        OnlineCredit.Visibility = online ? Visibility.Visible : Visibility.Collapsed;
        ApplySkin.IsDefault = !online;
        DownloadSkin.IsDefault = online;
        SkinList.ItemsSource = null;
        if (online && _online == null) await LoadOnlineAsync();
        else if (!online && _allSkins.Count == 0) RefreshSkins();
        else ApplySkinFilter();
    }

    async Task LoadOnlineAsync()
    {
        if (_loadingOnline) return;
        _loadingOnline = true;
        SkinCount.Text = "Loading the collection from archive.org…";
        SkinName.Text = "Get more skins";
        SkinAuthor.Text = "";
        SkinPreview.Source = null;
        SkinPreviewStatus.Text = "Loading…";
        DownloadSkin.IsEnabled = DownloadAll.IsEnabled = false;
        try
        {
            _online = await OnlineSkins.ListAsync(CancellationToken.None);
            DownloadSkin.IsEnabled = true;
            RefreshSkins(); // marks what's installed, then lists the collection
        }
        catch (Exception ex)
        {
            Log.Write("Skin collection list failed: " + ex.Message);
            SkinCount.Text = "Couldn't reach archive.org";
            SkinPreviewStatus.Text = "The skin list couldn't be loaded from archive.org.\nCheck the internet connection, then switch tabs to try again.\n\n" + ex.Message;
        }
        finally { _loadingOnline = false; }
    }

    void MarkInstalled()
    {
        if (_online == null) return;
        var have = new HashSet<string>(_allSkins.Select(s => s.FileName), StringComparer.OrdinalIgnoreCase);
        foreach (var s in _online) s.Installed = have.Contains(s.FileName);
        var missing = _online.Where(s => !s.Installed).ToList();
        DownloadAll.Content = missing.Count == 0 ? "All skins installed" : $"Download all ({missing.Count} · {FormatSize(missing.Sum(s => s.Size))})";
        DownloadAll.IsEnabled = missing.Count > 0 && _downloadCts == null;
    }

    void ShowOnlinePreview(OnlineSkin skin)
    {
        SkinName.Text = skin.DisplayName;
        SkinAuthor.Text = string.Join("  ·  ", new[] { PublisherLine(skin.Publisher, skin.Designer), FormatSize(skin.Size),
            skin.Installed ? "installed" : "not installed", "screenshot taken in Windows Media Player" }.Where(s => s.Length > 0));
        DownloadSkin.Content = skin.Installed ? "Apply Skin" : "Download and Apply";
        int version = ++_previewVersion;
        SkinPreview.Source = null;
        SkinPreviewStatus.Text = "Loading preview…";
        try
        {
            var img = new BitmapImage();
            img.BeginInit();
            img.UriSource = new Uri(skin.PreviewUrl);
            img.CacheOption = BitmapCacheOption.OnLoad;
            img.EndInit();
            void Show() { if (version != _previewVersion) return; SkinPreview.Source = img; SkinPreviewStatus.Text = ""; }
            if (!img.IsDownloading) Show();
            else
            {
                img.DownloadCompleted += (_, _) => Show();
                img.DownloadFailed += (_, _) => { if (version == _previewVersion) SkinPreviewStatus.Text = "No preview available"; };
            }
        }
        catch (Exception ex) { SkinPreviewStatus.Text = "No preview available"; Log.Write("Online preview: " + ex.Message); }
    }

    async void OnDownloadSkin(object sender, RoutedEventArgs e)
    {
        if (SkinList.SelectedItem is not OnlineSkin skin) return;
        string? path = skin.Installed ? System.IO.Path.Combine(SkinLibrary.InstallFolder, skin.FileName) : (await DownloadAsync(new[] { skin })).FirstOrDefault();
        if (path != null && System.IO.File.Exists(path)) App.Modes.ShowSkin(path);
    }

    async void OnDownloadAll(object sender, RoutedEventArgs e)
    {
        var missing = _online?.Where(s => !s.Installed).ToList();
        if (missing == null || missing.Count == 0) return;
        var answer = MessageBox.Show(this, $"Download {missing.Count} skins ({FormatSize(missing.Sum(s => s.Size))}) from archive.org?",
            "Get more skins", MessageBoxButton.OKCancel, MessageBoxImage.Question);
        if (answer == MessageBoxResult.OK) await DownloadAsync(missing);
    }

    void OnCancelDownload(object sender, RoutedEventArgs e) => _downloadCts?.Cancel();

    void OnOnlineSourceLink(object sender, RoutedEventArgs e) => WmpObjects.OpenInBrowser(OnlineSkins.PageUrl);

    /// <summary>Downloads skins one after another into the skins folder; returns the files that arrived.</summary>
    async Task<List<string>> DownloadAsync(IReadOnlyList<OnlineSkin> skins)
    {
        var done = new List<string>();
        if (_downloadCts != null) return done;
        var cts = _downloadCts = new CancellationTokenSource();
        long total = Math.Max(1, skins.Sum(s => s.Size)), received = 0;
        int failed = 0;
        bool active = true;
        var progress = new Progress<long>(n =>
        {
            if (!active) return; // a report posted after the last file finished
            received += n;
            DownloadProgress.Value = Math.Min(1, (double)received / total);
            DownloadStatus.Text = $"{FormatSize(received)} of {FormatSize(total)}";
        });
        DownloadRow.Visibility = CancelDownload.Visibility = Visibility.Visible;
        DownloadSkin.IsEnabled = DownloadAll.IsEnabled = false;
        DownloadProgress.Value = 0;
        try
        {
            foreach (var skin in skins)
            {
                cts.Token.ThrowIfCancellationRequested();
                SkinCount.Text = $"Downloading {skin.DisplayName}… ({done.Count + failed + 1} of {skins.Count})";
                try
                {
                    done.Add(await OnlineSkins.DownloadAsync(skin, SkinLibrary.InstallFolder, progress, cts.Token));
                    skin.Installed = true;
                }
                catch (Exception ex) when (ex is not OperationCanceledException)
                {
                    failed++;
                    Log.Write($"Skin download {skin.FileName} failed: {ex.Message}");
                }
            }
        }
        catch (OperationCanceledException) { }
        finally
        {
            active = false;
            bool cancelled = cts.IsCancellationRequested;
            _downloadCts = null;
            cts.Dispose();
            CancelDownload.Visibility = Visibility.Collapsed;
            DownloadSkin.IsEnabled = true;
            DownloadProgress.Value = cancelled ? DownloadProgress.Value : 1;
            DownloadStatus.Text = $"{done.Count} installed" + (failed > 0 ? $", {failed} failed" : "") + (cancelled ? ", cancelled" : "");
            if (done.Count > 0) S.Skins.NotifyInstalled(done);
            else MarkInstalled();
            if (OnlineMode) ApplySkinFilter();
        }
        return done;
    }

    /// <summary>"Disney · designed by The Skins Factory", "Microsoft", or just the artist for independent skins.</summary>
    static string PublisherLine(string publisher, string? designer)
    {
        designer = designer?.Trim();
        if (publisher is SkinCatalog.Independent or SkinCatalog.Other) return designer ?? "";
        return string.IsNullOrEmpty(designer) || designer.Contains(publisher, StringComparison.OrdinalIgnoreCase) || publisher.Contains(designer, StringComparison.OrdinalIgnoreCase)
            ? publisher : $"{publisher} · designed by {designer}";
    }

    static string FormatSize(long bytes) => bytes >= 1 << 20 ? $"{bytes / 1048576.0:0.#} MB" : $"{Math.Max(1, bytes / 1024)} KB";

    // ---- updates ------------------------------------------------------------------------

    void ShowUpdate()
    {
        if (UpdateChecker.Available is not { } update) return;
        UpdateLink.Content = $"Update available: v{update.Version}";
        UpdateLink.Visibility = Visibility.Visible;
    }

    void OnUpdateLink(object sender, RoutedEventArgs e)
    {
        if (UpdateChecker.Available is { } update) WmpObjects.OpenInBrowser(update.PageUrl);
    }

    void OnWidgetMode(object sender, RoutedEventArgs e) => App.Modes.CollapseToWidget();

    void OnAlwaysOnTop(object sender, RoutedEventArgs e) => App.Modes.ToggleAlwaysOnTop();

    /// <summary>The pin shows filled and in the accent colour while the player stays on top.</summary>
    void UpdateOnTop()
    {
        bool on = App.Modes.AlwaysOnTop;
        OnTopBtn.Content = on ? "" : "";
        OnTopBtn.Foreground = on ? (System.Windows.Media.Brush)FindResource("Accent") : (System.Windows.Media.Brush)FindResource("TextMain");
        OnTopBtn.ToolTip = on ? "Always on top: on. Click to let other windows cover the player"
                              : "Always on top: off. Click to keep the player above other windows";
    }

    void OnSkinMode(object sender, RoutedEventArgs e)
    {
        string? skin = (SkinsPanel.Visibility == Visibility.Visible ? (SkinList.SelectedItem as SkinEntry)?.Path : null) ?? S.Settings.CurrentSkin;
        if (skin == null || !System.IO.File.Exists(skin))
        {
            if (_allSkins.Count == 0) RefreshSkins();
            skin = _allSkins.FirstOrDefault()?.Path;
        }
        if (skin == null) { TabSkins.IsChecked = true; return; }
        App.Modes.ShowSkin(skin);
    }

    static void UseDarkTitleBar(IntPtr hwnd)
    {
        int on = 1;
        DwmSetWindowAttribute(hwnd, 20, ref on, 4);
    }

    [DllImport("dwmapi")] static extern int DwmSetWindowAttribute(IntPtr hwnd, int attr, ref int value, int size);
}
