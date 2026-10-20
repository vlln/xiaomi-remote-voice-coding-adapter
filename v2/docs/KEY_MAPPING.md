# 按键映射

V2.5.1 的映射方向是固定的：**遥控器按键是来源，目标键可自定义**。

## 默认映射

| 遥控器按键（固定） | 默认目标 | 说明 |
|---|---|---|
| 语音键 | 右 Option / 右 Alt | push-to-talk，目标可改，由 ATVV 会话注入 |
| Home 键 | Esc | |
| TV 键 | 退格 | |
| 菜单键 | 菜单 | 固定来源 keycode 110 / HID 0x65 |
| 返回键 | 退格 | |

- macOS 目标显示为「右 Option」，Windows / Linux 显示为「右 Alt」。
- 遥控器按键本身不能改；点击右侧目标键，然后按键盘上的按键即可重设目标。
- 目标键可以是 Esc、退格、回车、功能键、字母等标准键盘键。
- 语音键目标不再绑死右 Option/Alt，可改成任意可录制键；macOS 上由 ATVV 会话直接发送目标键。

## 录制流程（macOS）

1. 点击语音 / Home / TV / 菜单 / 返回右侧的目标键。
2. 后端进入 EventTap 捕获模式，吞掉下一次按键。
3. 按下键盘上的目标键；Esc 也可以作为目标（取消请点弹窗里的「取消」）。
4. 目标以 HID usage 保存，并立即应用。

## 测试

点击「测试按键」，按下遥控器上的按键：

- 不吞键，实际映射会发送到系统；
- 页面显示「检测到 Home 键 → Esc」或「未绑定」；
- 可以在目标 App 里直接验证效果。

## 平台实现

- macOS：固定来源 usage → 目标 usage，通过 `hidutil` 按 VID/PID 映射；语音键由 EventTap 在会话期间吞掉，ATVV 注入当前语音目标键。
- Linux：evdev grab 固定来源 key code → 目标 evdev code，通过 uinput 透传。
- Windows：默认语音键 SendInput；Home/TV/菜单/返回需要可选 Interception 构建。
- 按键录制目前只在 macOS 提供；Linux/Windows 使用默认目标或配置文件。

## 目标键支持范围

目标通过 HID usage 存储，常见字母、数字、F1–F12、方向键、Esc/退格/回车、修饰键都有跨平台转换。极特殊的厂商键可能只能识别来源、不能作为目标。
