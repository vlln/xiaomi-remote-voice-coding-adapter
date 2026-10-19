# Changelog

## 2.0.0

V2 是 Tauri 2 + Rust 的跨平台重写；V1 Python 脚本保持原样。

### Added

- Tauri 2 + React 菜单栏 App：状态、启停、设置、引导、日志、单实例
- Rust BLE ATVV 语音桥接：设备发现、已连接设备检索、自动重连、看门狗
- IMA ADPCM 解码 + 高通 / 平滑 / 自适应噪声门 / 15 dB 增益 / 16→48 kHz 重采样
- `cpal` 音频输出到虚拟麦克风（BlackHole / VB-CABLE / PipeWire）
- 每次会话保存 `last_session.wav`，界面内一键回放
- macOS 设备级按键映射：`hidutil` + EventTap，语音键由 ATVV 会话驱动右 Option
- Linux 设备级按键映射：evdev grab + uinput 全键透传
- Windows 语音键 `SendInput` 修饰键注入
- 可选 Windows `interception` feature：Interception 驱动的设备级 Home/TV/返回映射
- 开机自启（LaunchAgent / 登录项 / systemd 用户级，经 Tauri plugin）
- 关闭窗口驻留菜单栏、单实例唤起
- 本地打包：macOS `.app` / `.dmg`、Linux `.deb`
- 核心测试 18 个（macOS），Linux 14 个；V1 14 个
- CI：三平台构建、核心测试、rustfmt；V1 测试；tag 发布 workflow

### Known limitations

- macOS 干净右 Option 需要 `Privacy & Security → Device Control and Data Access` 授权（密码解锁）；未授权时回退到 `hidutil` 的 F5+Option。
- Windows 默认仅实现语音键；Home/TV/返回需要可选的 `interception` feature、Interception 驱动和 `interception.dll`。
- Linux/Windows 的 BLE/音频链路只做了编译/链接验证，尚未在真实设备上验证。
- 松键后约 0.3–0.5 秒的遥控器 ATVV 冷却仍在。
- 只保留最近一次录音。
- Windows 安装包由 CI 构建；macOS/Linux 本地包未签名/未公证。
