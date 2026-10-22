<div align="center">

<img src="https://cdn.jsdelivr.net/gh/vlln/xiaomi-remote-voice-coding-adapter@main/v2/public/app-icon.png" width="92" alt="Xiaomi Remote" />

# Xiaomi Remote

**把小米蓝牙语音遥控器 2 变成桌面语音输入键和可编程遥控器。**

BLE 语音 → 虚拟麦克风 · 设备级按键映射 · 持续按键测试 · 菜单栏常驻

<p>
  <strong>v2.5.11</strong> · macOS 已实机验证 · Windows/Linux CI 构建 · Rust 1.85+
</p>
<p>
  <a href="https://github.com/vlln/xiaomi-remote-voice-coding-adapter/releases/tag/v2.5.11">下载 Release</a>
  ·
  <a href="https://github.com/vlln/xiaomi-remote-voice-coding-adapter">GitHub</a>
</p>

</div>

## 它做什么

- **语音键 → 任意键**：按住遥控器说话，麦克风音频进入 BlackHole / VB-CABLE / PipeWire；语音键可以映射成右 Option、F6、Ctrl 等任意目标键。
- **固定来源，自由目标**：语音 / Home / TV / 菜单 / 返回的来源固定，目标键点击后按键盘即可重设；支持 Ctrl/Alt/Shift/Command 和小键盘。
- **持续按键测试**：点一次测试，连续按遥控器键，左侧遥控器图实时高亮并显示映射结果；测试期间拦截真实按键，不会误触发。
- **可直接接语音输入法**：Typeless、豆包语音输入法设置同一个“按住说话”键即可用遥控器输入。
- **干净的菜单栏 App**：模板图标随启停切换（空心/实心麦克风），支持深色/浅色菜单栏；音频处理（降噪 / 噪声门 / 增益）、快速设置、Star 引导。

## 界面

### 概览

![概览](https://cdn.jsdelivr.net/gh/vlln/xiaomi-remote-voice-coding-adapter@main/v2/docs/screenshot.png)

### 按键映射

![按键映射](https://cdn.jsdelivr.net/gh/vlln/xiaomi-remote-voice-coding-adapter@main/v2/docs/keymap.png)

### 持续按键测试

![持续按键测试](https://cdn.jsdelivr.net/gh/vlln/xiaomi-remote-voice-coding-adapter@main/v2/docs/capture.png)

## 配合语音输入法

Typeless、豆包语音输入法都支持“按一个键开始拾音，然后转成文字”。把它们的语音输入快捷键，设置成与遥控器语音键相同的目标键，就能直接用遥控器说话输入。

1. 在 Xiaomi Remote 把**语音键目标**设为 `F6`（或一个不冲突的普通键）。
2. Xiaomi Remote 音频输出选 **BlackHole 2ch**；输入法的麦克风也选 **BlackHole 2ch**。
3. Typeless / 豆包语音输入法里把**按住说话**快捷键设为同一个键，例如 `F6`。
4. 光标放到输入框，按住遥控器语音键说话，松开后等文字上屏。

> 两个输入法同时装时，不要把同一个快捷键同时启用；只保留一个，避免冲突。
>
> 不建议用右 Option 这类纯修饰键作为输入法快捷键；F6 这类普通键更稳。

详细设置与排错：[配合语音输入法](v2/docs/VOICE_INPUT.md)。

## 下载

Release: **[v2.5.11](https://github.com/vlln/xiaomi-remote-voice-coding-adapter/releases/tag/v2.5.11)**

| 平台 | 安装包 | 验证状态 |
|---|---|---|
| macOS arm64 | [Xiaomi.Remote_2.5.11_aarch64.dmg](https://github.com/vlln/xiaomi-remote-voice-coding-adapter/releases/download/v2.5.11/Xiaomi.Remote_2.5.11_aarch64.dmg) | ✅ 已实机验证 |
| Windows x64 | [Xiaomi.Remote_2.5.11_x64-setup.exe](https://github.com/vlln/xiaomi-remote-voice-coding-adapter/releases/download/v2.5.11/Xiaomi.Remote_2.5.11_x64-setup.exe) | ⚠️ CI 构建，未真机验证 |
| Linux amd64 | [Xiaomi.Remote_2.5.11_amd64.deb](https://github.com/vlln/xiaomi-remote-voice-coding-adapter/releases/download/v2.5.11/Xiaomi.Remote_2.5.11_amd64.deb) | ⚠️ CI 构建，未真机验证 |

<details>
<summary>SHA-256</summary>

```text
1621ca6e84a31ec03ae1860453d7893b40f5322342845216590e86fc9bea50c1  Xiaomi.Remote_2.5.11_aarch64.dmg
6dda83316ffe4f65d8d350f846be462a36be1c7c2bed228c982abf711aa5753a  Xiaomi.Remote_2.5.11_amd64.deb
e0630022e9dadf08710fdfe879424c3b0d4c066e1594480e07d45cf9e87956f3  Xiaomi.Remote_2.5.11_x64-setup.exe
```

</details>

> macOS 是唯一完成**真机 BLE / 音频 / 按键**验证的平台。Windows/Linux 通过了 CI 单元测试和安装包构建，但还没有在真机上验证，发布包请按实验性版本对待。

## 更新

App 启动后会自动检查版本，也可以在「高级 → 更新 → 检查更新」手动触发。检查源优先使用 jsDelivr 上的 `update.json`，失败时回退 GitHub API；发现新版本会在页面顶部提示，按钮会下载对应平台的安装包（DMG / NSIS / `.deb`），不会静默安装。

## macOS 快速开始

```bash
brew install blackhole-2ch
# 安装 DMG 后打开 Xiaomi Remote
```

1. 在「概览 → 快速设置」完成：BlackHole 2ch、键盘控制权限、遥控器连接。
2. 在「系统设置 → 隐私与安全性 → 设备控制与数据访问」允许 Xiaomi Remote（需密码解锁）。
3. 在会议 App 里把麦克风设为 **BlackHole 2ch**。
4. 到「按键映射」点击目标键，按键盘上的键完成重设；点「测试按键」可连续验证。

## 平台状态

| 平台 | 单元测试 | 安装包 | 真机验证 |
|---|---|---|---|
| macOS arm64 | 24/24 | DMG | ✅ |
| Windows x64 | 23/23 | NSIS | ❌ |
| Linux amd64 | 25/25 | `.deb` | ❌ |

更细的平台差异、构建方式和已知限制见 **[v2/README.md](v2/README.md)**。

## 项目结构

```text
v2/            Tauri 2 + React + Rust 主应用
  src-tauri/   BLE / ATVV / DSP / 录音 / 按键后端
  docs/        截图与按键映射文档
  tools/       橙圈对齐工具
v1/            已归档的 Python + Karabiner 版本
```

## 开发

```bash
cd v2
pnpm install
pnpm tauri dev
```

构建 macOS DMG：

```bash
pnpm tauri build
```

核心测试：

```bash
cd src-tauri && cargo test --lib
```

## 文档

- [V2 README](v2/README.md)
- [按键映射](v2/docs/KEY_MAPPING.md)
- [配合语音输入法](v2/docs/VOICE_INPUT.md)
- [发布说明](v2/RELEASE_NOTES.md)
- [CHANGELOG](v2/CHANGELOG.md)
- [V1 归档](v1/README.md)

## License

GPL-3.0-only. 早期协议/音频实现参考了 GPL-3.0 的 [remote-bridge-hub](https://github.com/xxb26553663-star/remote-bridge-hub)，详见 [LICENSE](LICENSE) 与 [v1/README.md](v1/README.md)。

<div align="center">

如果它让你的遥控器更好用，去 GitHub 点个 Star 支持一下 ⭐

</div>
