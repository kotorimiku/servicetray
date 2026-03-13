# Service Tray

[English](README.md) | [简体中文](README_zh_CN.md)

系统托盘应用，用于管理和控制多个服务/程序。

## 功能特性

- 从系统托盘管理多个程序
- 直接从托盘菜单打开服务网址
- 配置文件热重载

## 安装

### 从源码构建

```bash
git clone https://github.com/kotorimiku/servicetray.git
cd servicetray
cargo build --release
```

编译后的程序位于 `target/release/servicetray`。

## 配置

配置文件按以下顺序查找：
1. 可执行文件旁的 `config.json`（便携模式）
2. `~/.config/servicetray.json`

### 配置示例

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

### 字段说明

| 字段 | 说明 |
|------|------|
| `programs` | 要管理的程序列表 |
| `programs[].name` | 托盘菜单中显示的名称 |
| `programs[].path` | 可执行文件路径或命令 |
| `programs[].args` | 可选的命令行参数 |
| `programs[].service_url` | 可选的菜单打开网址 |
| `autostart` | 是否开机自启 |
| `log_level` | 日志级别（trace, debug, info, warn, error） |
| `save_log_file` | 是否保存日志到文件 |

## 命令行参数

```
servicetray [OPTIONS]

Options:
  -l, --log-level <LEVEL>   设置日志级别
      --save-log-file <BOOL> 保存日志到文件
  -h, --help                显示帮助
  -V, --version             显示版本
```

## 许可证

MIT
