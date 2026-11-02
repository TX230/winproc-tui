using System.Runtime.InteropServices;

internal static class Native
{
    [DllImport("user32")] internal static extern uint GetGuiResources(nint process, uint flags);
}
