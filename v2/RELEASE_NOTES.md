# Xiaomi Remote V2.5.6 — Release Notes

发布日期：2026-10-20

## 验证范围

| 平台 | 状态 | 说明 |
|---|---|---|
| macOS arm64 | ✅ 已验证 | Apple Silicon / macOS 27，真实遥控器、BlackHole 2ch、语音会话、按键映射、持续测试、音频处理 |
| Windows x86_64 | ⚠️ 未测试 | 本轮没有在 Windows 真机验证 BLE / 音频 / 按键，也没有构建发布包 |
| Linux | ⚠️ 未测试 | 本轮没有在 Linux 真机验证 BLE / 音频 / 按键，也没有构建发布包 |

> Windows/Linux 的代码路径存在，但**不是本次发布验证过的能力**。发布说明中的“可用”仅指 macOS。

## macOS 产物

```text
Xiaomi Remote_2.5.6_aarch64.dmg
SHA-256 0d365322332bbc0098c0f8e9250d0152a04be9d5d0dc4235b4f069372b50f10e
```

- 构建方式：`pnpm tauri build`
- 签名：ad-hoc（`-`），**未做 Apple 公证**
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

## 已知限制

- Windows/Linux 未做真机验证；需要时请从源码构建并自行测试。
- macOS 的 ad-hoc 签名在重装后可能丢失 TCC 授权，需要重新允许。
- 测试模式会拦截所有真实按键（鼠标停止不受影响）。
- 只缓存最近一次录音；概览不再提供回放入口，音频页可播放。
- 特殊厂商按键可能无法被 EventTap 识别。

## 相关文档

- [README](README.md)
- [按键映射](docs/KEY_MAPPING.md)
- [CHANGELOG](CHANGELOG.md)
- [橙圈对齐工具](tools/align-hotspots.html)
