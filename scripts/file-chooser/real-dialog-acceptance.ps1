param([string]$Run = '')

$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
Set-Location $repo
$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
if (-not $Run) { $Run = Join-Path $repo "logs\file-chooser-real.$stamp" }
$Run = [IO.Path]::GetFullPath($Run)
New-Item -ItemType Directory -Force -Path $Run | Out-Null
$logFile = Join-Path $Run 'acceptance.log'
$summaryFile = Join-Path $Run 'summary.json'
$cases = [System.Collections.Generic.List[object]]::new()
$script:typedCount = 0

function Write-Log([string]$message) {
    $line = '{0} {1}' -f (Get-Date -Format 'HH:mm:ss.fff'), $message
    Add-Content -Path $logFile -Value $line -Encoding utf8
    Write-Host $line
}

Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Windows.Forms, System.Drawing
Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public static class AcceptWin {
    public delegate bool EnumProc(IntPtr hWnd, IntPtr lParam);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc lpEnumFunc, IntPtr lParam);
    [DllImport("user32.dll")] public static extern IntPtr GetWindow(IntPtr hWnd, uint uCmd);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool IsWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
    [DllImport("user32.dll")] public static extern void keybd_event(byte bVk, byte bScan, uint dwFlags, UIntPtr dwExtraInfo);
    [DllImport("user32.dll", SetLastError = true)] public static extern bool PostMessage(IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam);
    public static List<IntPtr> TopLevel() {
        var list = new List<IntPtr>();
        EnumWindows((h, l) => { list.Add(h); return true; }, IntPtr.Zero);
        return list;
    }
}
'@

$pipeName = "acde-file-chooser-$stamp"
$prepared = & bun (Join-Path $repo 'scripts\file-chooser\manual-file-chooser.ts') $Run --prepare-only
if ($LASTEXITCODE -ne 0) { throw "could not prepare the probe: $prepared" }
$pkg = Join-Path $Run 'package'
$homeDir = Join-Path $Run 'home'
$sample = Join-Path $Run 'sample'
$saved = Join-Path $Run 'saved'
$savedFile = Join-Path $saved 'saved-from-chooser.txt'
$runtime = Join-Path $repo 'target\release\acdev.exe'

$envBefore = @{}
$envSet = @{
    ACDE_DEV = "\\.\pipe\$pipeName"; ACDE_PATH = $pkg
    ACDE_APP = $(if ($env:ACDE_APP) { $env:ACDE_APP } else { Join-Path $repo 'web\packages\acde-host\dist' })
    HOME = $homeDir; USERPROFILE = $homeDir; XDG_CONFIG_HOME = $homeDir; XDG_DATA_HOME = $homeDir; XDG_STATE_HOME = (Join-Path $Run 'state')
}
$envRemove = @('ACDE_HEADLESS', 'ACDE_STORAGE', 'ACDE_BACKEND')
foreach ($key in @($envSet.Keys + $envRemove)) { $envBefore[$key] = [Environment]::GetEnvironmentVariable($key) }
foreach ($kv in $envSet.GetEnumerator()) { [Environment]::SetEnvironmentVariable($kv.Key, $kv.Value) }
foreach ($key in $envRemove) { [Environment]::SetEnvironmentVariable($key, $null) }
Write-Log "starting $runtime app manual-file-chooser (headful, dev pipe $pipeName)"
$proc = Start-Process -FilePath $runtime -ArgumentList 'app', 'manual-file-chooser' -PassThru -RedirectStandardError (Join-Path $Run 'runtime.stderr.log')
foreach ($kv in $envBefore.GetEnumerator()) { [Environment]::SetEnvironmentVariable($kv.Key, $kv.Value) }

$pipe = $null
$reader = $null
$writer = $null
$nextId = 0
$hostWindow = [IntPtr]::Zero

function Get-HostWindow {
    $deadline = (Get-Date).AddSeconds(30)
    while ((Get-Date) -lt $deadline) {
        foreach ($h in [AcceptWin]::TopLevel()) {
            [uint32]$procId = 0
            [void][AcceptWin]::GetWindowThreadProcessId($h, [ref]$procId)
            if ($procId -eq $proc.Id -and [AcceptWin]::IsWindowVisible($h) -and [AcceptWin]::GetWindow($h, 4) -eq [IntPtr]::Zero) { return $h }
        }
        Start-Sleep -Milliseconds 250
    }
    throw 'the probe window did not appear'
}

function Send-Rpc([string]$op, [hashtable]$fields) {
    $script:nextId++
    $id = $script:nextId
    $message = @{ type = 'request'; id = $id; op = $op } + $fields
    $writer.WriteLine((ConvertTo-Json $message -Compress -Depth 5))
    while ($true) {
        $line = $reader.ReadLine()
        if ($null -eq $line) { throw "the dev pipe closed while waiting for $op" }
        $reply = $line | ConvertFrom-Json
        if ($reply.type -eq 'response' -and $reply.id -eq $id) {
            if (-not $reply.ok) { throw "$op failed: $($reply.error)" }
            return $reply.result
        }
    }
}

function Get-PageResult { (Send-Rpc 'eval' @{ web = 'manual-file-chooser'; code = "document.getElementById('result').textContent" }).value }

function Click-Page([string]$id) {
    [void](Send-Rpc 'eval' @{ web = 'manual-file-chooser'; code = "document.getElementById('$id').click(), 'clicked'" })
}

function Save-Page([string]$name) {
    $shot = Send-Rpc 'shot' @{ web = 'manual-file-chooser' }
    [IO.File]::WriteAllBytes((Join-Path $Run "$name.png"), [Convert]::FromBase64String($shot.png))
}

function Wait-Dialog {
    $deadline = (Get-Date).AddSeconds(25)
    while ((Get-Date) -lt $deadline) {
        foreach ($h in [AcceptWin]::TopLevel()) {
            if ([AcceptWin]::IsWindowVisible($h) -and [AcceptWin]::GetWindow($h, 4) -eq $script:hostWindow) { return $h }
        }
        Start-Sleep -Milliseconds 200
    }
    throw 'no file dialog owned by the probe window appeared'
}

function Wait-Closed($dialog) {
    $deadline = (Get-Date).AddSeconds(25)
    while ((Get-Date) -lt $deadline) {
        if (-not [AcceptWin]::IsWindow($dialog)) { return }
        Start-Sleep -Milliseconds 200
    }
    throw 'the file dialog stayed open'
}

function Get-Root($dialog) { [System.Windows.Automation.AutomationElement]::FromHandle($dialog) }

function Focus-Dialog($dialog) {
    [void][AcceptWin]::ShowWindow($dialog, 9)
    for ($attempt = 0; $attempt -lt 3; $attempt++) {
        if ([AcceptWin]::GetForegroundWindow() -eq $dialog) { return }
        [AcceptWin]::keybd_event(0x12, 0, 0, [UIntPtr]::Zero)
        [AcceptWin]::keybd_event(0x12, 0, 2, [UIntPtr]::Zero)
        [void][AcceptWin]::SetForegroundWindow($dialog)
        Start-Sleep -Milliseconds 250
    }
    if ([AcceptWin]::GetForegroundWindow() -ne $dialog) { throw 'the file dialog could not be brought to the front; nothing was typed' }
}

function Escape-Keys([string]$text) {
    -join ($text.ToCharArray() | ForEach-Object { if ('+^%~(){}[]'.Contains([string]$_)) { "{$_}" } else { [string]$_ } })
}

function Send-ToDialog($dialog, [string]$keys) {
    Focus-Dialog $dialog
    if ([AcceptWin]::GetForegroundWindow() -ne $dialog) { throw 'the foreground changed; nothing was typed' }
    [System.Windows.Forms.SendKeys]::SendWait($keys)
    Start-Sleep -Milliseconds 250
}

function Describe-Focus {
    $focused = [System.Windows.Automation.AutomationElement]::FocusedElement
    if (-not $focused) { return 'no focused element' }
    $value = $null
    $text = ''
    if ($focused.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref]$value)) { $text = ([System.Windows.Automation.ValuePattern]$value).Current.Value }
    return ('focus type={0} id={1} name={2} value={3}' -f $focused.Current.ControlType.ProgrammaticName, $focused.Current.AutomationId, $focused.Current.Name, $text)
}

function Type-Name($dialog, [string]$text) {
    Send-ToDialog $dialog '%n'
    Write-Log ('  after alt+n: ' + (Describe-Focus))
    Send-ToDialog $dialog '^a'
    Set-Clipboard -Value $text
    Send-ToDialog $dialog '^v'
    Start-Sleep -Milliseconds 400
    Write-Log ('  after paste: ' + (Describe-Focus))
    $script:typedCount++
    Save-Dialog $dialog ('typed-{0:00}' -f $script:typedCount)
}

function Accept($dialog) { Send-ToDialog $dialog '{ENTER}' }

function Cancel($dialog) { Send-ToDialog $dialog '{ESC}' }

function List-Names($dialog) {
    $root = Get-Root $dialog
    $condition = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ControlTypeProperty, [System.Windows.Automation.ControlType]::ListItem)
    $deadline = (Get-Date).AddSeconds(15)
    while ((Get-Date) -lt $deadline) {
        $items = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants, $condition)
        if ($items.Count -gt 0) { return @($items | ForEach-Object { $_.Current.Name }) }
        Start-Sleep -Milliseconds 300
    }
    return @()
}

function Save-Dialog($dialog, [string]$name) {
    $rect = (Get-Root $dialog).Current.BoundingRectangle
    if ($rect.Width -lt 1 -or $rect.Height -lt 1) { return }
    $bitmap = New-Object System.Drawing.Bitmap ([int]$rect.Width), ([int]$rect.Height)
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    $graphics.CopyFromScreen([int]$rect.X, [int]$rect.Y, 0, 0, $bitmap.Size)
    $bitmap.Save((Join-Path $Run "$name.png"), [System.Drawing.Imaging.ImageFormat]::Png)
    $graphics.Dispose()
    $bitmap.Dispose()
}

function Close-Dialogs {
    foreach ($h in [AcceptWin]::TopLevel()) {
        if ([AcceptWin]::IsWindowVisible($h) -and [AcceptWin]::GetWindow($h, 4) -eq $script:hostWindow) {
            [void][AcceptWin]::PostMessage($h, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero)
        }
    }
}

function Dump-Tree($element, [int]$depth, [System.Collections.Generic.List[string]]$lines) {
    $patterns = ($element.GetSupportedPatterns() | ForEach-Object { $_.ProgrammaticName }) -join ','
    $lines.Add(('{0}{1} id={2} name={3} patterns=[{4}]' -f ('  ' * $depth), $element.Current.ControlType.ProgrammaticName, $element.Current.AutomationId, $element.Current.Name, $patterns))
    if ($depth -gt 9) { return }
    $child = [System.Windows.Automation.TreeWalker]::RawViewWalker.GetFirstChild($element)
    while ($child) {
        Dump-Tree $child ($depth + 1) $lines
        $child = [System.Windows.Automation.TreeWalker]::RawViewWalker.GetNextSibling($child)
    }
}

function Dump-Dialogs([string]$label) {
    $lines = [System.Collections.Generic.List[string]]::new()
    foreach ($h in [AcceptWin]::TopLevel()) {
        if ([AcceptWin]::IsWindowVisible($h) -and [AcceptWin]::GetWindow($h, 4) -eq $script:hostWindow) {
            Dump-Tree (Get-Root $h) 0 $lines
        }
    }
    if ($lines.Count -gt 0) { Set-Content -Path (Join-Path $Run "tree-$label.txt") -Value $lines -Encoding utf8 }
}

function Run-Case([string]$name, [scriptblock]$body) {
    Write-Log "case $name"
    try {
        $detail = & $body
        $cases.Add([ordered]@{ name = $name; status = 'pass'; detail = [string]$detail })
        Write-Log "  pass: $detail"
    } catch {
        try { Dump-Dialogs $name } catch {}
        $cases.Add([ordered]@{ name = $name; status = 'fail'; detail = $_.Exception.Message })
        Write-Log "  FAIL: $($_.Exception.Message)"
        throw
    }
}

$passed = $false
try {
    $script:hostWindow = Get-HostWindow
    Write-Log ('probe window handle {0}' -f $script:hostWindow)
    $pipe = New-Object System.IO.Pipes.NamedPipeClientStream('.', $pipeName, [System.IO.Pipes.PipeDirection]::InOut)
    $pipe.Connect(20000)
    $utf8 = New-Object System.Text.UTF8Encoding($false)
    $writer = New-Object System.IO.StreamWriter($pipe, $utf8)
    $writer.AutoFlush = $true
    $reader = New-Object System.IO.StreamReader($pipe, $utf8)
    $writer.WriteLine('{"type":"hello","role":"dev"}')

    $deadline = (Get-Date).AddSeconds(30)
    while ((Get-Date) -lt $deadline) {
        $title = (Send-Rpc 'eval' @{ web = 'manual-file-chooser'; code = 'document.title' }).value
        if ($title -eq 'Manual File Chooser') { break }
        Start-Sleep -Milliseconds 300
    }
    Write-Log 'page loaded'

    Run-Case 'open-one-text' {
        Click-Page 'one'
        $dialog = Wait-Dialog
        Start-Sleep -Milliseconds 800
        Save-Dialog $dialog 'open-one-dialog'
        Type-Name $dialog (Join-Path $sample 'notes.txt')
        Accept $dialog
        Wait-Closed $dialog
        Start-Sleep -Milliseconds 400
        $result = Get-PageResult
        Save-Page 'open-one-result'
        if ($result -ne 'Open one: notes.txt: "plain text sample"') { throw "unexpected result: $result" }
        $result
    }

    Run-Case 'open-several' {
        Click-Page 'many'
        $dialog = Wait-Dialog
        Start-Sleep -Milliseconds 800
        Type-Name $dialog $sample
        Accept $dialog
        Start-Sleep -Milliseconds 1000
        if (-not [AcceptWin]::IsWindow($dialog)) { throw 'the dialog closed when navigating to the sample folder' }
        Save-Dialog $dialog 'open-several-dialog'
        Type-Name $dialog '"notes.txt" "data.csv"'
        Accept $dialog
        Wait-Closed $dialog
        Start-Sleep -Milliseconds 400
        $result = Get-PageResult
        Save-Page 'open-several-result'
        if (-not ($result.StartsWith('Open several: ') -and $result.Contains('notes.txt: "plain text sample"') -and $result.Contains('data.csv: '))) {
            throw "unexpected result: $result"
        }
        $result
    }

    Run-Case 'accept-filter-png-jpg' {
        Click-Page 'png'
        $dialog = Wait-Dialog
        Start-Sleep -Milliseconds 800
        Type-Name $dialog $sample
        Accept $dialog
        Start-Sleep -Milliseconds 1000
        $names = List-Names $dialog
        Save-Dialog $dialog 'accept-filter-dialog'
        if ($names.Count -eq 0) { throw 'the dialog listed no items' }
        if (-not ($names -contains '流萤合照.png' -and $names -contains 'picture.png')) { throw "image files missing from the list: $($names -join ', ')" }
        if ($names -contains 'notes.txt' -or $names -contains 'data.csv') { throw "text files shown under the .png/.jpg filter: $($names -join ', ')" }
        Type-Name $dialog (Join-Path $sample '流萤合照.png')
        Accept $dialog
        Wait-Closed $dialog
        Start-Sleep -Milliseconds 400
        $result = Get-PageResult
        if ($result -ne 'Input accept: 流萤合照.png') { throw "unexpected result: $result" }
        "listed: $($names -join ', '); result: $result"
    }

    Run-Case 'accept-type-png-mime' {
        Click-Page 'pngtype'
        $dialog = Wait-Dialog
        Start-Sleep -Milliseconds 800
        Type-Name $dialog $sample
        Accept $dialog
        Start-Sleep -Milliseconds 1000
        $names = List-Names $dialog
        Save-Dialog $dialog 'accept-type-png-dialog'
        if ($names.Count -eq 0) { throw 'the dialog listed no items' }
        if (-not ($names -contains 'picture.png' -and $names -contains '流萤合照.png')) { throw "png files missing from the list: $($names -join ', ')" }
        if ($names -contains 'notes.txt' -or $names -contains 'data.csv') { throw "text files shown under the png type filter: $($names -join ', ')" }
        Cancel $dialog
        Wait-Closed $dialog
        Start-Sleep -Milliseconds 400
        $result = Get-PageResult
        if ($result -ne 'Open png type ended: AbortError') { throw "unexpected result: $result" }
        "listed: $($names -join ', '); result: $result"
    }

    Run-Case 'open-one-chinese-binary' {
        Click-Page 'one'
        $dialog = Wait-Dialog
        Start-Sleep -Milliseconds 800
        Type-Name $dialog (Join-Path $sample '屏幕截图 1.png')
        Accept $dialog
        Wait-Closed $dialog
        Start-Sleep -Milliseconds 400
        $result = Get-PageResult
        if (-not $result.StartsWith('Open one: 屏幕截图 1.png: 12 bytes')) { throw "unexpected result: $result" }
        $result
    }

    Run-Case 'save-write' {
        Click-Page 'save'
        $dialog = Wait-Dialog
        Start-Sleep -Milliseconds 800
        Save-Dialog $dialog 'save-dialog'
        Type-Name $dialog $savedFile
        Accept $dialog
        Wait-Closed $dialog
        Start-Sleep -Milliseconds 600
        $result = Get-PageResult
        if ($result -ne 'Saved as saved-from-chooser.txt') { throw "unexpected result: $result" }
        $content = [IO.File]::ReadAllText($savedFile)
        if ($content -ne 'written by the manual check') { throw "unexpected file content: $content" }
        "$result; file content checked"
    }

    Run-Case 'cancel-open-one' {
        Click-Page 'one'
        $dialog = Wait-Dialog
        Start-Sleep -Milliseconds 800
        Cancel $dialog
        Wait-Closed $dialog
        Start-Sleep -Milliseconds 400
        $result = Get-PageResult
        if ($result -ne 'Open one ended: AbortError') { throw "unexpected result: $result" }
        $result
    }

    Run-Case 'cancel-open-several' {
        Click-Page 'many'
        $dialog = Wait-Dialog
        Start-Sleep -Milliseconds 800
        Cancel $dialog
        Wait-Closed $dialog
        Start-Sleep -Milliseconds 400
        $result = Get-PageResult
        if ($result -ne 'Open several ended: AbortError') { throw "unexpected result: $result" }
        $result
    }

    Run-Case 'cancel-save' {
        Click-Page 'save'
        $dialog = Wait-Dialog
        Start-Sleep -Milliseconds 800
        Cancel $dialog
        Wait-Closed $dialog
        Start-Sleep -Milliseconds 400
        $result = Get-PageResult
        if ($result -ne 'Save ended: AbortError') { throw "unexpected result: $result" }
        $count = @(Get-ChildItem -Path $saved -File).Count
        if ($count -ne 1) { throw "a cancelled save wrote files: $count in saved" }
        $result
    }

    Run-Case 'cancel-input' {
        $before = Get-PageResult
        Click-Page 'png'
        $dialog = Wait-Dialog
        Start-Sleep -Milliseconds 800
        Cancel $dialog
        Wait-Closed $dialog
        Start-Sleep -Milliseconds 400
        $after = Get-PageResult
        if ($after -ne $before) { throw "the input result changed on cancel: $after" }
        Save-Page 'cancel-input-result'
        "result unchanged: $after"
    }

    $passed = $true
} catch {
    Write-Log "stopped: $($_.Exception.Message)"
} finally {
    try { Close-Dialogs } catch {}
    try { if ($writer) { $writer.WriteLine('{"type":"quit"}') } } catch {}
    try { if ($pipe) { $pipe.Dispose() } } catch {}
    Start-Sleep -Milliseconds 800
    if (-not $proc.HasExited) { try { [void]$proc.CloseMainWindow() } catch {} }
    if (-not $proc.WaitForExit(8000)) {
        Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
        Write-Log 'probe process stopped by force'
    }
    $summary = [ordered]@{ passed = $passed; run = $Run; cases = $cases }
    $summary | ConvertTo-Json -Depth 6 | Set-Content -Path $summaryFile -Encoding utf8
    Write-Log "summary: $summaryFile"
}
if (-not $passed) { exit 1 }
