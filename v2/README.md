# Xiaomi Remote V2

跨平台的「小米蓝牙语音遥控器 2 Pro」桌面端：BLE ATVV 语音桥接 + 虚拟麦克风 + 设备级按键映射 + 菜单栏 UI。

V2 是 Tauri 2 + Rust 的完整重写，V1 的 Python 脚本仍保留在仓库根目录作为参考实现。

![Xiaomi Remote V2](docs/screenshot.png)

## 功能

- 通过 BLE 连接遥控器，接收 ATVV 语音流
- IMA ADPCM 解码 → 高通 / 平滑 / 自适应噪声门 / 15 dB 增益 → 16 kHz→48 kHz → 虚拟麦克风
- 菜单栏常驻：状态、启停、日志、开机自启
- 设备级按键映射（macOS `hidutil`，不需要 Karabiner）
- 每次语音会话保存 `last_session.wav`，可在界面里一键回放
- 自动重连、看门狗、单实例、配置持久化

## 平台支持（V2.0.0）

| 能力 | macOS | Windows | Linux |
|---|---|---|---|
| BLE 语音桥接 | ✅ 已实机验证 | ✅ 代码已抽象，待实机验证 | ✅ 代码已抽象，待实机验证 |
| 虚拟麦克风输出 | BlackHole 2ch | VB-CABLE / Virtual Audio Cable | PipeWire / PulseAudio null sink |
| 按键映射 | ✅ `hidutil` | ⚠️ 待实现（Interception 驱动方案） | ⚠️ 待实现（evdev + uinput） |
| 菜单栏 / 托盘 | ✅ | ✅ | ✅ |
| 开机自启 | ✅ | ✅ | ✅ |
| 录音回放 | ✅ | ✅ | ✅ |

> V2.0.0 首先在 macOS 上跑通；Windows/Linux 的 BLE/音频层使用跨平台 crate（`btleplug` + `cpal`），但还需要在对应系统上做实机验证与按键后端。

## 架构

```text
v2/
├─ src/                     React + TypeScript UI
│  ├─ App.tsx               仪表盘 / 设置 / 日志 / 引导
│  └─ App.css
├─ src-tauri/
│  ├─ src/
│  │  ├─ core/
│  │  │  ├─ adpcm.rs        IMA ADPCM 解码（与 V1 逐样本一致）
│  │  │  ├─ dsp.rs          高通 / 3 点平滑 / 噪声门 / 增益 / 重采样
│  │  │  └─ atvv.rs         ATVV 协议 + 会话状态机
│  │  ├─ ble.rs             btleplug BLE 传输（含 macOS 已连接设备检索）
│  │  ├─ audio.rs           cpal 输出 + 48 kHz 立体声环形缓冲
│  │  ├─ recording.rs       每次会话写 last_session.wav
│  │  ├─ keys/              设备级按键映射（macOS hidutil 后端）
│  │  ├─ state.rs           Tauri 状态 / 命令 / 配置持久化
│  │  └─ lib.rs             托盘、插件、窗口、命令注册
│  └─ tauri.conf.json
└─ package.json
```

数据流：

```text
遥控器 --BLE ATVV--> btleplug --> Session 状态机
                                  ├─ ADPCM 解码
                                  ├─ DSP 链路
                                  └─ CpalSink --> 虚拟麦克风
                                  └─ RecordingSink --> last_session.wav
```

按键链路（macOS）：

```text
遥控器 HID --> hidutil UserKeyMapping（按 VID/PID 限定）
           └─ f5 → right Option，home → Escape，TV → Backspace，0xF1 → Backspace/Escape
```

## 开发

前置：

- Rust 1.85+
- Node.js 20+ / pnpm
- macOS：Xcode Command Line Tools
- Linux：`libwebkit2gtk-4.1-dev`、`libasound2-dev`、`libudev-dev`、`pkg-config`
- Windows：WebView2 Runtime、Visual Studio Build Tools

```bash
cd v2
pnpm install
pnpm tauri dev
```

生产构建：

```bash
pnpm tauri build
```

核心测试：

```bash
cd src-tauri
cargo test --lib
```

## macOS 使用

1. 安装 BlackHole 2ch：

   ```bash
   brew install blackhole-2ch
   ```

2. 打开 App，确认「准备检查」四项全绿。
3. 首次启动会请求蓝牙权限；允许后 App 会自动扫描/连接遥控器。
   为了让语音键变成干净的右 Option，还需要在「系统设置 → 隐私与安全性 → **辅助功能**」中允许 Xiaomi Remote。
4. 把系统输入或目标 App 的输入设为 **BlackHole 2ch**（或包含它的聚集设备）。
5. 按语音键说话；界面会显示音频帧和采样数，松开后可以点「播放上次录音」验证。

> 如果之前装过 Karabiner 并启用了遥控器规则，请先退出 Karabiner，避免它和 V2 的 `hidutil` 映射同时作用于同一个设备。

## V1 与 V2 的关系

- V1：`scripts/voice_capture.py` + Karabiner + 手动终端启动
- V2：`v2/` 下的 Tauri App，接管 BLE、音频、按键映射、托盘和自启
- V1 的协议/音频实现是 V2 Rust 移植的参考；`tests/test_voice_capture.py` 仍可用于对照 V1 行为

## 已知限制

- 遥控器松键后约 0.3–0.5 秒内重按，可能仍会被遥控器自身的 ATVV 冷却挡住；这是目前实测的下限。
- macOS 的语音键 → 右 Option 由 ATVV 会话事件驱动，并用 EventTap 吞掉会话期间的 F5；首次运行需要在「系统设置 → 隐私与安全性 → 辅助功能」里允许 Xiaomi Remote。未授权时会退回 `hidutil` 的 F5+Option 行为。
- Windows/Linux 按键映射后端尚未实现。
- 只保留最近一次录音，文件位于系统 App Data 目录下的 `last_session.wav`。
