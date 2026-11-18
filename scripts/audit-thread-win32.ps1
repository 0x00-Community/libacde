param(
    [Parameter(Mandatory)] [int] $ProcessId,
    [Parameter(Mandatory)] [ValidateSet('Trace', 'Resize')] [string] $Mode
)

$ErrorActionPreference = 'Stop'
$runtime = Get-Process -Id $ProcessId
if ($runtime.ProcessName -ne 'acdev') { throw 'Expected an acdev process' }
Add-Type @'
using System;
using System.Runtime.InteropServices;

public static class AuditWin32 {
    [DllImport("kernel32.dll")]
    private static extern IntPtr OpenThread(uint access, bool inheritHandle, uint id);
    [DllImport("kernel32.dll")]
    private static extern int GetThreadDescription(IntPtr thread, out IntPtr description);
    [DllImport("kernel32.dll")]
    private static extern bool CloseHandle(IntPtr handle);
    [DllImport("kernel32.dll")]
    private static extern IntPtr LocalFree(IntPtr memory);
    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool SetWindowPos(IntPtr window, IntPtr after, int x, int y, int w, int h, uint flags);

    public static string Describe(uint id) {
        var thread = OpenThread(0x0800, false, id);
        if (thread == IntPtr.Zero) return "";
        try {
            if (GetThreadDescription(thread, out var text) != 0 || text == IntPtr.Zero) return "";
            try { return Marshal.PtrToStringUni(text); }
            finally { LocalFree(text); }
        }
        finally { CloseHandle(thread); }
    }
}
'@

if ($Mode -eq 'Resize') {
    $window = $runtime.MainWindowHandle
    $at = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
    if ($window -eq [IntPtr]::Zero -or -not [AuditWin32]::SetWindowPos($window, [IntPtr]::Zero, 0, 0, 980, 650, 22)) {
        throw 'Could not resize the test host window'
    }
    $at
    exit 0
}

while (-not $runtime.HasExited) {
    $runtime.Refresh()
    $stamp = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
    $threads = foreach ($thread in $runtime.Threads) {
        $name = [AuditWin32]::Describe($thread.Id)
        try {
            $state = [string] $thread.ThreadState
            [pscustomobject]@{
                id = $thread.Id
                name = $name
                cpuMs = $thread.TotalProcessorTime.TotalMilliseconds
                state = $state
                reason = if ($state -eq 'Wait') { [string] $thread.WaitReason } else { $null }
            }
        } catch [System.ComponentModel.Win32Exception] {}
        catch [System.InvalidOperationException] {}
    }
    @{ at = $stamp; threads = @($threads) } | ConvertTo-Json -Depth 3 -Compress
    Start-Sleep -Milliseconds 150
}
