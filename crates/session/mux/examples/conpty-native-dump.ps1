param([uint32]$SnapshotPid, [string]$Out)
$ErrorActionPreference = 'Stop'
Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Text;
public static class NativeDump {
  [StructLayout(LayoutKind.Sequential)] public struct Coord { public short X, Y; public Coord(short x, short y) {X=x;Y=y;} }
  [StructLayout(LayoutKind.Sequential)] struct Rect { public short Left, Top, Right, Bottom; }
  [StructLayout(LayoutKind.Sequential)] struct Info { public Coord Size, Cursor; public ushort Attributes; public Rect Window; public Coord Maximum; }
  [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
  [DllImport("kernel32.dll")] static extern bool FreeConsole();
  [DllImport("kernel32.dll",SetLastError=true)] static extern bool AttachConsole(uint pid);
  [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern IntPtr CreateFileW(string name,uint access,uint share,IntPtr security,uint disposition,uint flags,IntPtr template);
  [DllImport("kernel32.dll",SetLastError=true)] static extern bool GetConsoleScreenBufferInfo(IntPtr h,out Info info);
  [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool ReadConsoleOutputCharacterW(IntPtr h,[Out] char[] text,uint count,Coord start,out uint read);
  static void Check(bool ok) { if(!ok) throw new Win32Exception(Marshal.GetLastWin32Error()); }
  public static string Snapshot(uint pid) {
    FreeConsole(); Check(AttachConsole(pid));
    IntPtr h=CreateFileW("CONOUT$",0x80000000,3,IntPtr.Zero,3,0,IntPtr.Zero);
    try {
      if(h==new IntPtr(-1)) throw new Win32Exception(Marshal.GetLastWin32Error());
      Info info; Check(GetConsoleScreenBufferInfo(h,out info));
      var r=new StringBuilder(); r.AppendFormat("native cursor=({0},{1}) window={2}..{3} buffer={4}x{5}",info.Cursor.X,info.Cursor.Y,info.Window.Top,info.Window.Bottom,info.Size.X,info.Size.Y);
      for(short y=0;y<info.Size.Y;y++){var row=new char[info.Size.X]; uint n; Check(ReadConsoleOutputCharacterW(h,row,(uint)info.Size.X,new Coord(0,y),out n)); var s=new string(row,0,(int)n).TrimEnd(); if(s.Length>0||y==info.Cursor.Y) r.AppendFormat("\n  row {0,2}: \"{1}\"",y,s);}
      return r.ToString();
    } finally {if(h!=new IntPtr(-1))CloseHandle(h); FreeConsole();}
  }
}
'@
[NativeDump]::Snapshot($SnapshotPid) | Out-File -Encoding utf8 $Out
