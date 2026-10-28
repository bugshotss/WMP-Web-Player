namespace WebSkinPlayer;

/// <summary>
/// Test mode switch: while enabled, skins can run (scripts, timers, animations) but nothing
/// leaves the process — no media commands or volume changes to the real player, no URLs,
/// sounds, dialogs, saved preferences/settings, and no visible windows.
/// </summary>
public static class Sandbox
{
    public static bool Enabled { get; set; }
}
