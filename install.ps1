# Installs thermtty to Program Files and starts it at logon, elevated, without a UAC prompt.
# Run from an elevated PowerShell next to thermtty.exe (or in the repo after building):
#   powershell -ExecutionPolicy Bypass -File install.ps1 [-User DOMAIN\name] [-Uninstall]
param(
    [string]$User = "$env:USERDOMAIN\$env:USERNAME",
    [switch]$Uninstall
)
$ErrorActionPreference = 'Stop'

$principal = [Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw 'Run this from an elevated PowerShell.'
}

$dir = Join-Path $env:ProgramFiles 'thermtty'
$exe = Join-Path $dir 'thermtty.exe'
$task = 'thermtty'
# Launching thermtty while it runs opens its settings, so the shortcut doubles as "settings".
$shortcut = Join-Path $env:ProgramData 'Microsoft\Windows\Start Menu\Programs\thermtty.lnk'

Get-Process thermtty -ErrorAction SilentlyContinue | Stop-Process -Force
Unregister-ScheduledTask -TaskName $task -Confirm:$false -ErrorAction SilentlyContinue

if ($Uninstall) {
    Start-Sleep -Milliseconds 500
    Remove-Item $dir -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item $shortcut -Force -ErrorAction SilentlyContinue
    'thermtty uninstalled.'
    return
}

$source = @("$PSScriptRoot\thermtty.exe", "$PSScriptRoot\target\release\thermtty.exe") |
    Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $source) { throw 'thermtty.exe not found next to this script or in target\release.' }

Start-Sleep -Milliseconds 500 # let a just-stopped instance release the file
New-Item -ItemType Directory -Force $dir | Out-Null
Copy-Item $source $exe -Force
$link = (New-Object -ComObject WScript.Shell).CreateShortcut($shortcut)
$link.TargetPath = $exe
$link.Description = 'Start thermtty, or open its settings if it is already running'
$link.Save()

# Program Files keeps the elevated target admin-writable only. Defaults that would stop a
# widget are overridden: the 72 h execution limit and the on-battery restrictions.
$action = New-ScheduledTaskAction -Execute $exe
$trigger = New-ScheduledTaskTrigger -AtLogOn -User $User
$run = New-ScheduledTaskPrincipal -UserId $User -LogonType Interactive -RunLevel Highest
$settings = New-ScheduledTaskSettingsSet -ExecutionTimeLimit ([TimeSpan]::Zero) -AllowStartIfOnBatteries `
    -DontStopIfGoingOnBatteries -MultipleInstances IgnoreNew
Register-ScheduledTask -TaskName $task -Action $action -Trigger $trigger -Principal $run -Settings $settings | Out-Null
Start-ScheduledTask -TaskName $task
"thermtty installed to $exe and set to start at logon for $User."
