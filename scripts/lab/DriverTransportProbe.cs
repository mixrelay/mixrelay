using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;

// Native IOCTL/lifecycle smoke only; does not create WASAPI audio streams.
public static class MixRelayDriverTransportProbe
{
    [StructLayout(LayoutKind.Sequential)]
    private struct Request { public uint Version, Size, Render, Capture, Rate, Capacity; }
    [StructLayout(LayoutKind.Sequential)]
    private struct Descriptor { public uint Version, Size; public ulong Bytes, Address, RenderEvent, CaptureEvent; }
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern SafeFileHandle CreateFile(string name, uint access, uint share,
        IntPtr security, uint disposition, uint flags, IntPtr template);
    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool DeviceIoControl(SafeFileHandle file, uint code, ref Request request,
        uint inputSize, out Descriptor result, uint outputSize, out uint returned, IntPtr overlapped);
    [DllImport("kernel32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool CloseHandle(IntPtr handle);

    public static int Run()
    {
        int cycles = 0;
        if (Marshal.SizeOf(typeof(Request)) != 24 || Marshal.SizeOf(typeof(Descriptor)) != 40)
            throw new InvalidOperationException("Transport ABI mismatch");
        for (uint channels = 1; channels <= 64; ++channels)
        foreach (uint rate in new uint[] {44100, 48000, 96000})
        {
            using (var file = CreateFile(@"\\.\MixRelayAudio", 0xC0000000, 3, IntPtr.Zero, 3, 0, IntPtr.Zero))
            {
                if (file.IsInvalid) throw new Win32Exception(Marshal.GetLastWin32Error(), "Open MixRelay control");
                var request = new Request {Version=1, Size=24, Render=channels, Capture=channels, Rate=rate, Capacity=256};
                Descriptor descriptor; uint returned;
                if (DeviceIoControl(file, 0x001DE000, ref request, 24, out descriptor, 40, out returned, IntPtr.Zero))
                    throw new InvalidOperationException("Old ABI unexpectedly accepted");
                int error = Marshal.GetLastWin32Error();
                if (error != 87) throw new Win32Exception(error, "Expected invalid-version rejection");
                request.Version = 2;
                if (!DeviceIoControl(file, 0x001DE000, ref request, 24, out descriptor, 40, out returned, IntPtr.Zero))
                    throw new Win32Exception(Marshal.GetLastWin32Error(), "Map transport");
                try
                {
                    if (returned != 40 || descriptor.Version != 2 || descriptor.Size != 40 ||
                        descriptor.Address == 0 || descriptor.RenderEvent == 0 || descriptor.CaptureEvent == 0)
                        throw new InvalidOperationException("Invalid descriptor");
                    var address = new IntPtr((long)descriptor.Address);
                    if (Marshal.ReadInt32(address, 0) != 0x4F49524D || Marshal.ReadInt32(address, 4) != 2 ||
                        Marshal.ReadInt32(address, 8) != 112 || Marshal.ReadInt32(address, 16) != channels ||
                        Marshal.ReadInt32(address, 24) != rate || Marshal.ReadInt32(address, 28) != 1)
                        throw new InvalidOperationException("Mapped header mismatch/offline");
                    Descriptor duplicate;
                    if (DeviceIoControl(file, 0x001DE000, ref request, 24, out duplicate, 40, out returned, IntPtr.Zero))
                        throw new InvalidOperationException("Duplicate controller unexpectedly accepted");
                    if (Marshal.GetLastWin32Error() != 170)
                        throw new Win32Exception(Marshal.GetLastWin32Error(), "Expected busy controller");
                    ++cycles;
                }
                finally
                {
                    if (descriptor.RenderEvent != 0) CloseHandle(new IntPtr((long)descriptor.RenderEvent));
                    if (descriptor.CaptureEvent != 0) CloseHandle(new IntPtr((long)descriptor.CaptureEvent));
                }
                // Close file last: CLEANUP invalidates the user mapping.
            }
        }
        return cycles;
    }
}
