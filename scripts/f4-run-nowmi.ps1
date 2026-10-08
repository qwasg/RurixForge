param(
    [Parameter(Mandatory = $true)][string]$Script,
    [Parameter(ValueFromRemainingArguments = $true)]$Rest
)
# 本机 WMI 服务挂住时(任何 Get-CimInstance 都无限等待)的临时替身:在调用方作用域定义同名函数 Get-CimInstance,
# 只实现被包装脚本用到的 `Win32_Process`(ProcessId / ParentProcessId / Name,-Filter 支持 ParentProcessId= / ProcessId=),
# 用 Toolhelp32 快照取数据;CommandLine 取不到,给空串(被包装脚本只拿它留痕)。PowerShell 先解析函数、后解析 cmdlet,
# 所以被包装脚本一行不改,判据与原脚本完全相同。
# 同理替身 Get-NetTCPConnection(NetTCPIP 模块同样走 CIM,WMI 挂住时一样无限等待):只支持 -State Listen,
# 数据取自 iphlpapi 的 GetExtendedTcpTable(IPv4 + IPv6 监听表),给 LocalAddress / LocalPort / OwningProcess / State。
# 用法: powershell -NoProfile -ExecutionPolicy Bypass -File scripts\f4-run-nowmi.ps1 -Script scripts\f3-desktop-presenter-smoke.ps1 -Backend godot
$ErrorActionPreference = 'Stop'
Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public static class F4Snap {
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    public struct PE32 {
        public uint dwSize; public uint cntUsage; public uint th32ProcessID; public IntPtr th32DefaultHeapID;
        public uint th32ModuleID; public uint cntThreads; public uint th32ParentProcessID; public int pcPriClassBase;
        public uint dwFlags; [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 260)] public string szExeFile;
    }
    [DllImport("kernel32.dll", SetLastError = true)] static extern IntPtr CreateToolhelp32Snapshot(uint flags, uint pid);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)] static extern bool Process32FirstW(IntPtr h, ref PE32 e);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)] static extern bool Process32NextW(IntPtr h, ref PE32 e);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
    public static List<PE32> All() {
        var list = new List<PE32>();
        IntPtr h = CreateToolhelp32Snapshot(2, 0);
        if (h == IntPtr.Zero || h == new IntPtr(-1)) return list;
        var e = new PE32(); e.dwSize = (uint)Marshal.SizeOf(typeof(PE32));
        if (Process32FirstW(h, ref e)) { do { list.Add(e); e.dwSize = (uint)Marshal.SizeOf(typeof(PE32)); } while (Process32NextW(h, ref e)); }
        CloseHandle(h);
        return list;
    }
    // TCP_TABLE_OWNER_PID_LISTENER = 3;AF_INET = 2(行 24 字节)、AF_INET6 = 23(行 56 字节);端口是网络字节序的低 16 位。
    public class Tcp { public string LocalAddress; public int LocalPort; public int OwningProcess; public string State; }
    [DllImport("iphlpapi.dll")] static extern uint GetExtendedTcpTable(IntPtr buf, ref int size, bool order, int af, int cls, uint reserved);
    static IntPtr TcpTable(int af, out uint rc) {
        rc = 0;
        for (int tries = 0; tries < 4; tries++) {
            int size = 0;
            GetExtendedTcpTable(IntPtr.Zero, ref size, false, af, 3, 0);
            size += 4096;
            IntPtr buf = Marshal.AllocHGlobal(size);
            rc = GetExtendedTcpTable(buf, ref size, false, af, 3, 0);
            if (rc == 0) return buf;
            Marshal.FreeHGlobal(buf);
            if (rc != 122) break; // 122 = ERROR_INSUFFICIENT_BUFFER:两次调用之间表变大了,重取
        }
        return IntPtr.Zero;
    }
    public static List<Tcp> Listeners() {
        var list = new List<Tcp>();
        foreach (int af in new[] { 2, 23 }) {
            uint rc;
            IntPtr buf = TcpTable(af, out rc);
            if (buf == IntPtr.Zero) {
                if (af == 2) throw new InvalidOperationException("GetExtendedTcpTable(AF_INET) rc=" + rc);
                continue; // 没有 IPv6 协议栈时 AF_INET6 取不到,只用 IPv4
            }
            try {
                int n = Marshal.ReadInt32(buf), row = af == 2 ? 24 : 56, portOff = af == 2 ? 8 : 20, pidOff = af == 2 ? 20 : 52;
                for (int i = 0; i < n; i++) {
                    IntPtr r = IntPtr.Add(buf, 4 + i * row);
                    byte[] addr = new byte[af == 2 ? 4 : 16];
                    Marshal.Copy(af == 2 ? IntPtr.Add(r, 4) : r, addr, 0, addr.Length);
                    int raw = Marshal.ReadInt32(r, portOff);
                    list.Add(new Tcp { LocalAddress = new System.Net.IPAddress(addr).ToString(), LocalPort = ((raw & 0xFF) << 8) | ((raw >> 8) & 0xFF),
                        OwningProcess = Marshal.ReadInt32(r, pidOff), State = "Listen" });
                }
            } finally { Marshal.FreeHGlobal(buf); }
        }
        return list;
    }
}
'@
function Get-CimInstance {
    param([Parameter(Position = 0)][string]$ClassName, [string[]]$Property, [string]$Filter, [int]$OperationTimeoutSec)
    if ($ClassName -ne 'Win32_Process') { throw "f4-run-nowmi: 只替身 Win32_Process,不支持 $ClassName" }
    $rows = [F4Snap]::All() | ForEach-Object {
        [pscustomobject]@{ ProcessId = [int]$_.th32ProcessID; ParentProcessId = [int]$_.th32ParentProcessID; Name = $_.szExeFile; CommandLine = '' }
    }
    if ($Filter -match 'ParentProcessId\s*=\s*(\d+)') { $v = [int]$Matches[1]; $rows = @($rows | Where-Object { $_.ParentProcessId -eq $v }) }
    elseif ($Filter -match 'ProcessId\s*=\s*(\d+)') { $v = [int]$Matches[1]; $rows = @($rows | Where-Object { $_.ProcessId -eq $v }) }
    return $rows
}
function Get-NetTCPConnection {
    [CmdletBinding()]
    param([string[]]$State, [int[]]$LocalPort, [int[]]$OwningProcess)
    if (-not $State -or @($State | Where-Object { $_ -ne 'Listen' }).Count -gt 0) { throw "f4-run-nowmi: Get-NetTCPConnection 只替身 -State Listen,不支持 '$($State -join ',')'" }
    $rows = @([F4Snap]::Listeners())
    if ($LocalPort) { $rows = @($rows | Where-Object { $_.LocalPort -in $LocalPort }) }
    if ($OwningProcess) { $rows = @($rows | Where-Object { $_.OwningProcess -in $OwningProcess }) }
    return $rows
}
$target = if ([IO.Path]::IsPathRooted($Script)) { $Script } else { Join-Path (Split-Path -Parent $PSScriptRoot) $Script }
# "-Name value" / "-Switch" 转成哈希表再展开(数组展开会把 "-Name" 当成位置参数)。
$named = @{}
$pos = @()
$argList = New-Object 'System.Collections.Generic.List[string]'
foreach ($x in @($Rest)) { if ($null -ne $x) { $argList.Add([string]$x) } }
for ($i = 0; $i -lt $argList.Count; $i++) {
    $a = $argList[$i]
    if ($a.Length -gt 1 -and $a.StartsWith('-')) {
        $key = $a.Substring(1).TrimEnd(':')
        $next = $null
        if ($i + 1 -lt $argList.Count) { $next = $argList[$i + 1] }
        if ($null -ne $next -and -not ($next.Length -gt 1 -and $next.StartsWith('-'))) { $named[$key] = $next; $i++ } else { $named[$key] = $true }
    } else { $pos += $a }
}
& $target @named @pos
exit $LASTEXITCODE
