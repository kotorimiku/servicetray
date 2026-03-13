# Service Tray

[English](README.md) | [简体中文](README_zh_CN.md)

A system tray application for managing and controlling multiple services/programs.

## Features

- Manage multiple programs from system tray
- Open service URLs directly from tray menu
- Hot-reload configuration file

## Installation

### From Source

```bash
git clone https://github.com/kotorimiku/servicetray.git
cd servicetray
cargo build --release
```

The binary will be at `target/release/servicetray`.

## Configuration

Service Tray looks for configuration in this order:
1. `config.json` next to the executable (portable mode)
2. `~/.config/servicetray.json`

### Example Configuration

```json
{
    "programs": [
        {
            "name": "syncthing",
            "path": "syncthing",
            "args": ["--no-browser"],
            "service_url": "http://localhost:8384"
        }
    ],
    "autostart": false,
    "log_level": "info",
    "save_log_file": true
}
```

### Fields

| Field | Description |
|-------|-------------|
| `programs` | List of programs to manage |
| `programs[].name` | Display name in tray menu |
| `programs[].path` | Executable path or command |
| `programs[].args` | Optional command-line arguments |
| `programs[].service_url` | Optional URL to open from menu |
| `autostart` | Enable auto-start on boot |
| `log_level` | Log level (trace, debug, info, warn, error) |
| `save_log_file` | Save logs to file |

## CLI Options

```
servicetray [OPTIONS]

Options:
  -l, --log-level <LEVEL>   Set the log level
      --save-log-file <BOOL> Save log to file
  -h, --help                Show help
  -V, --version             Show version
```

## License

MIT
