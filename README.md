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

- Drag anywhere to move; position is saved to `%APPDATA%\thermtty\config.ini`.
- Click the `×` in the top-right corner to close (it turns red on hover).
- Right-click: always on top, opacity, text size, exit.
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
