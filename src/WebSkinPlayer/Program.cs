namespace WebSkinPlayer;

public static class Program
{
    /// <summary>
    /// Runs the WPF UI thread with a large stack: skin scripts execute on it through an
    /// interpreting JS engine, and a deeply recursive skin should hit the engine's recursion
    /// limit (a script error) rather than overflow the 1 MB default stack and crash the app.
    /// </summary>
    [STAThread]
    public static void Main()
    {
        var ui = new Thread(() =>
        {
            var app = new App();
            app.InitializeComponent();
            app.Run();
        }, 64 * 1024 * 1024);
        ui.SetApartmentState(ApartmentState.STA);
        ui.Start();
        ui.Join();
    }
}
