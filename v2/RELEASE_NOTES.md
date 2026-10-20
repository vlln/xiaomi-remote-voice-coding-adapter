# Xiaomi Remote V2.5.7 — Release Notes

发布日期：2026-10-20

## 验证范围

| 平台 | 状态 | 说明 |
|---|---|---|
| macOS arm64 | ✅ 已验证 | Apple Silicon / macOS 27，真实遥控器、BlackHole 2ch、语音会话、按键映射、持续测试、音频处理 |
| Windows x86_64 | ⚠️ 仅 CI 验证 | windows-latest 上 23/23 单元测试通过并构建 NSIS；**未在 Windows 真机验证** BLE / 音频 / 按键 |
| Linux amd64 | ⚠️ 仅 CI 验证 | ubuntu-22.04 上 25/25 单元测试通过并构建 `.deb`；**未在 Linux 真机验证** BLE / 音频 / 按键 |

> Windows/Linux 的“通过”只代表 CI 编译、单元测试和打包通过，**不代表真机可用**。三平台中只有 macOS 完成了实机验证。

## 产物

Release: https://github.com/vlln/xiaomi-remote-voice-coding-adapter/releases/tag/v2.5.7

```text
Xiaomi.Remote_2.5.7_aarch64.dmg
SHA-256 f7cc01fbc4625cac6559b49aedf8a3b123eb587289a8f6747de855fa01d42abb

Xiaomi.Remote_2.5.7_x64-setup.exe
SHA-256 5aa4158c144048b6d6dff78fcc860064939193f961a4f5d0c968ec1747c6cd6d

Xiaomi.Remote_2.5.7_amd64.deb
SHA-256 2978476a772472307912ff167b238212ad590c44ce4370eadc26c9db175fb010
```

- macOS 签名：ad-hoc（`-`），**未做 Apple 公证**
- 首次打开可能需要在「系统设置 → 隐私与安全性」允许
- 重装/换构建后，需在「设备控制与数据访问」重新授权键盘控制

## 本版变化

- 概览页删除「回放上次录音」按钮；音频页仍保留「上次录音 → 播放」
- 持续按键测试：测试期间临时拦截真实按键，页面顶部显示测试状态与停止按钮
- 页面/分区标题下的重复描述已删除
- 固定菜单键（keycode 110 / HID 0x65）
- 语音键目标可改，不再绑死右 Option/Alt
- 按键圈按用户实测坐标对齐，并加入按压效果
- 目标键录制支持 Ctrl/Alt/Shift/Command、Caps Lock 和小键盘
- 新应用图标；GitHub 区域改为 Star 引导

## 安装（macOS）

1. 挂载 DMG，把 **Xiaomi Remote.app** 拖入 `/Applications`
2. 打开 App，按「概览 → 快速设置」完成：
   - BlackHole 2ch
   - 键盘控制权限（「设备控制与数据访问」）
   - 遥控器连接
3. 在会议 App 里把麦克风设为 **BlackHole 2ch**

### Windows

运行 `Xiaomi.Remote_2.5.7_x64-setup.exe`；需要 WebView2 Runtime 和 VB-CABLE（未真机验证）。

### Linux

```bash
sudo apt-get install -y libwebkit2gtk-4.1-0 libasound2 libudev1
sudo dpkg -i Xiaomi.Remote_2.5.7_amd64.deb
sudo apt-get install -f
```

需要 PipeWire/PulseAudio null sink（未真机验证）。

## 界面截图

- [概览](https://cdn.jsdelivr.net/gh/vlln/xiaomi-remote-voice-coding-adapter@main/v2/docs/screenshot.png)
- [按键映射](https://cdn.jsdelivr.net/gh/vlln/xiaomi-remote-voice-coding-adapter@main/v2/docs/keymap.png)
- [持续按键测试](https://cdn.jsdelivr.net/gh/vlln/xiaomi-remote-voice-coding-adapter@main/v2/docs/capture.png)

## 已知限制

- Windows/Linux 已完成 CI 单元测试和安装包构建，但**未做真机验证**；BLE/音频/按键需要自行测试。
- macOS 的 ad-hoc 签名在重装后可能丢失 TCC 授权，需要重新允许。
- 测试模式会拦截所有真实按键（鼠标停止不受影响）。
- 只缓存最近一次录音；概览不再提供回放入口，音频页可播放。
- 特殊厂商按键可能无法被 EventTap 识别。

## 相关文档

- [README](README.md)
- [按键映射](docs/KEY_MAPPING.md)
- [CHANGELOG](CHANGELOG.md)
- [橙圈对齐工具](tools/align-hotspots.html)
- [配合语音输入法（Typeless / 豆包）](docs/VOICE_INPUT.md)
