using System.Runtime.InteropServices;
using System.Text;

internal static class Native
{
    [DllImport("kernel32", SetLastError = true)] internal static extern nint VirtualAlloc(nint address, nuint size, uint type, uint protection);
    [DllImport("kernel32", SetLastError = true)] [return: MarshalAs(UnmanagedType.Bool)] internal static extern bool VirtualFree(nint address, nuint size, uint type);
    [DllImport("user32", CharSet = CharSet.Unicode, SetLastError = true)] internal static extern nint CreateWindowExW(uint exStyle, string cls, string title, uint style,
        int x, int y, int width, int height, nint parent, nint menu, nint instance, nint param);
    [DllImport("user32", SetLastError = true)] [return: MarshalAs(UnmanagedType.Bool)] internal static extern bool DestroyWindow(nint hwnd);
    [DllImport("gdi32", SetLastError = true)] internal static extern nint CreateSolidBrush(uint color);
    [DllImport("gdi32", SetLastError = true)] [return: MarshalAs(UnmanagedType.Bool)] internal static extern bool DeleteObject(nint handle);
    [DllImport("user32")] internal static extern uint GetGuiResources(nint process, uint flags);
    [DllImport("workload-gpu", CallingConvention = CallingConvention.Cdecl, CharSet = CharSet.Unicode)] internal static extern int gpu_create(uint index, out nint gpu, StringBuilder name, uint capacity, out ulong luid);
    [DllImport("workload-gpu", CallingConvention = CallingConvention.Cdecl)] internal static extern int gpu_resize(nint gpu, uint mib);
    [DllImport("workload-gpu", CallingConvention = CallingConvention.Cdecl)] internal static extern int gpu_tick(nint gpu, uint passes);
    [DllImport("workload-gpu", CallingConvention = CallingConvention.Cdecl)] internal static extern int gpu_stats(nint gpu, out ulong local, out ulong nonlocal, out ulong budget, out uint mib);
    [DllImport("workload-gpu", CallingConvention = CallingConvention.Cdecl)] internal static extern void gpu_destroy(nint gpu);
    internal static nint Require(nint value) => value != 0 ? value : throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
    internal static void Require(bool value) { if (!value) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error()); }
    internal static void Hr(int hr) => Marshal.ThrowExceptionForHR(hr);
}
