using System;
using System.Runtime.InteropServices;

// Read-only runtime verification: BCD describes the next boot, not necessarily
// the currently running kernel. Keep this separate from boot-policy mutation.
public static class MixRelayCodeIntegrityState
{
    [StructLayout(LayoutKind.Sequential)]
    private struct Information { public uint Length; public uint Options; }

    [DllImport("ntdll.dll", ExactSpelling = true)]
    private static extern int NtQuerySystemInformation(
        int informationClass, ref Information information, uint length, out uint returned);

    public static uint Read()
    {
        var information = new Information { Length = 8 };
        uint returned;
        int status = NtQuerySystemInformation(103, ref information, 8, out returned);
        if (status != 0 || returned != 8)
            throw new InvalidOperationException("Code Integrity query failed: " + status);
        return information.Options;
    }
}
