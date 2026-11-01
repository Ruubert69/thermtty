# thermtty

Tiny terminal-style desktop widget for Windows, four rows:

```
cpu   52°   19%  ▂▃▅▆▅▃▂▂▁▁▂▃
gpu   41°   39%  ▁▂▂▃▅▆▅▃▂▂▁▁
ram 15.6G   24%  ━━━─────────
net  ↓98K ↑224K  ▁▁▂▅▂▁▁▁▃▁▁▁
```

Temperature, load and a 60-second temperature sparkline for CPU and GPU; RAM use; network
down/up with a traffic sparkline. Native Win32 + GDI, single ~290 KB exe with no runtime
or redistributable needed.

## Sources

| Reading | Source |
|---|---|
| CPU temperature | [PawnIO](https://pawnio.eu) driver + `AMDFamily17` module (AMD family 17h–1Ah only) |
| CPU load | `GetSystemTimes` |
| GPU temperature, load | NVAPI (`nvapi64.dll`, NVIDIA only; NVML would cost ~20 MiB more) |
| RAM | `GlobalMemoryStatusEx` |
| Network | `GetIfTable2`, physical adapters only |

## Requirements

- PawnIO installed (it ships with recent LibreHardwareMonitor / FanControl, or from pawnio.eu).
- Runs elevated (the manifest requests admin) because PawnIO needs it.
  Without PawnIO the CPU temperature shows `--` and everything else still works.

## Build

```
cargo build --release
```

`target/release/thermtty.exe` is self-contained.

## Use

- Drag anywhere to move; click the `×` in the top-right corner to close.
- Right-click opens the settings panel (right-click again, Esc, or click elsewhere to close).
  Launching thermtty again while it runs (Start → thermtty after `install.ps1`) also opens
  it, for when the widget is hidden behind windows:
  - **layer**: always on top, normal window, or keep on desktop (stays behind all windows)
  - **widget**: show the close button, lock position (no dragging), temperatures in °F
  - **rows**: show or hide cpu, gpu, ram and net
  - **opacity**, **text size**, **refresh** (1, 2 or 5 s; graphs span 60 samples)
  - **quit**
- Settings and position are saved to `%APPDATA%\thermtty\config.ini`.
- `thermtty.exe --dump out.txt` writes one rendered frame as text, for checking sensors.

### Start with Windows

`install.ps1` copies the exe to `C:\Program Files\thermtty` and registers a logon task that
runs it elevated (no UAC prompt), with no execution time limit. From an elevated PowerShell,
next to `thermtty.exe` or in the repo after building:

```
powershell -ExecutionPolicy Bypass -File install.ps1
powershell -ExecutionPolicy Bypass -File install.ps1 -Uninstall
```

Re-run it after rebuilding to update the installed copy.

## License note

`assets/AMDFamily17.bin` is the compiled `AMDFamily17` module from
[namazso/PawnIO.Modules](https://github.com/namazso/PawnIO.Modules) 0.2.11, LGPL-2.1.
