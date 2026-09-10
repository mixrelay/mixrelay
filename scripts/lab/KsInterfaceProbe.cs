using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;

// Read-only inventory: distinguish an INF registry entry from a live KS interface.
public static class MixRelayKsInterfaceProbe
{
    [StructLayout(LayoutKind.Sequential)] struct InterfaceData
    { public uint Size; public Guid Class; public uint Flags; public UIntPtr Reserved; }
    [DllImport("setupapi.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    static extern IntPtr SetupDiGetClassDevs(ref Guid cls, string name, IntPtr hwnd, uint flags);
    [DllImport("setupapi.dll", SetLastError=true)]
    static extern bool SetupDiEnumDeviceInterfaces(IntPtr set, IntPtr device, ref Guid cls, uint index, ref InterfaceData data);
    [DllImport("setupapi.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    static extern bool SetupDiGetDeviceInterfaceDetail(IntPtr set, ref InterfaceData data, IntPtr detail, uint size, out uint needed, IntPtr device);
    [DllImport("setupapi.dll")] static extern bool SetupDiDestroyDeviceInfoList(IntPtr set);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    static extern SafeFileHandle CreateFile(string path, uint access, uint share, IntPtr security, uint disposition, uint flags, IntPtr template);
    [DllImport("kernel32.dll", SetLastError=true)]
    static extern bool DeviceIoControl(SafeFileHandle file, uint code, byte[] input, uint inputSize, byte[] output, uint outputSize, out uint returned, IntPtr overlapped);
    public static void Run()
    {
        Guid cls = new Guid("6994ad04-93ef-11d0-a3cc-00a0c9223196");
        IntPtr set = SetupDiGetClassDevs(ref cls, null, IntPtr.Zero, 0x12);
        if (set == new IntPtr(-1)) throw new Win32Exception();
        try {
            for (uint i=0;;i++) {
                InterfaceData data = new InterfaceData { Size=(uint)Marshal.SizeOf(typeof(InterfaceData)) };
                if (!SetupDiEnumDeviceInterfaces(set, IntPtr.Zero, ref cls, i, ref data)) {
                    if (Marshal.GetLastWin32Error()==259) break;
                    throw new Win32Exception();
                }
                uint needed;
                SetupDiGetDeviceInterfaceDetail(set, ref data, IntPtr.Zero, 0, out needed, IntPtr.Zero);
                IntPtr detail=Marshal.AllocHGlobal((int)needed);
                try {
                    Marshal.WriteInt32(detail, IntPtr.Size==8 ? 8 : 6);
                    if (!SetupDiGetDeviceInterfaceDetail(set, ref data, detail, needed, out needed, IntPtr.Zero)) throw new Win32Exception();
                    string path=Marshal.PtrToStringUni(IntPtr.Add(detail,4));
                    Console.WriteLine("Interface flags="+data.Flags+" "+path);
                    if (path.IndexOf("root#media#0000",StringComparison.OrdinalIgnoreCase)<0) continue;
                    using (SafeFileHandle file=CreateFile(path,0xC0000000,3,IntPtr.Zero,3,0,IntPtr.Zero)) {
                        if (file.IsInvalid) { Console.WriteLine("OPEN ERROR="+Marshal.GetLastWin32Error()); continue; }
                        byte[] property=new byte[24];
                        new Guid("8c134960-51ad-11cf-878a-94f801c10000").ToByteArray().CopyTo(property,0);
                        BitConverter.GetBytes(1u).CopyTo(property,16); // KSPROPERTY_PIN_CTYPES
                        BitConverter.GetBytes(1u).CopyTo(property,20); // GET
                        byte[] output=new byte[4]; uint returned;
                        bool ok=DeviceIoControl(file,0x002F0003,property,24,output,4,out returned,IntPtr.Zero);
                        Console.WriteLine(ok ? "PIN COUNT="+BitConverter.ToUInt32(output,0) : "PROPERTY ERROR="+Marshal.GetLastWin32Error());
                        for (uint pin=0; pin<2; pin++) {
                            foreach (uint id in new uint[] {2, 7, 9, 10}) {
                                byte[] request=new byte[32];
                                property.CopyTo(request,0);
                                BitConverter.GetBytes(id).CopyTo(request,16);
                                BitConverter.GetBytes(pin).CopyTo(request,24);
                                byte[] value=new byte[4096];
                                ok=DeviceIoControl(file,0x002F0003,request,32,value,4096,out returned,IntPtr.Zero);
                                string result=ok ? BitConverter.ToString(value,0,(int)Math.Min(returned,128u)) : "ERROR="+Marshal.GetLastWin32Error();
                                if (ok && id==10 && returned>8) result="pin="+BitConverter.ToUInt32(value,4)+" path="+System.Text.Encoding.Unicode.GetString(value,8,(int)returned-8).TrimEnd('\0');
                                Console.WriteLine("pin="+pin+" property="+id+" "+result);
                            }
                        }
                    }
                } finally { Marshal.FreeHGlobal(detail); }
            }
        } finally { SetupDiDestroyDeviceInfoList(set); }
    }
}
