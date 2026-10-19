# 按键映射

V2.1 的按键映射是「录制式」的：点击某个动作右侧的按键，然后按下遥控器上的对应键。App 会等待下一次按键，录制成功后立即保存并应用。

## 默认绑定

| 动作 | 默认按键 | HID usage | 目标 |
|---|---|---|---|
| 语音键 | F5 | `0x3E` | 右 Option（由 ATVV 会话驱动） |
| Home 键 | Home | `0x4A` | Escape |
| TV 键 | `~` | `0x35` | Backspace |
| 返回键 | AC Back | `0xF1` | Backspace / Escape / 不映射 |

## 数据模型

绑定以 USB HID keyboard usage 存储，而不是 macOS keycode。这样同一份配置可以翻译到：

- macOS：`hidutil UserKeyMapping`
- Linux：evdev key code + uinput
- Windows：SendInput / 可选 Interception scancode

旧版配置文件没有 `voice_key` / `home_key` / `tv_key` / `back_key` 字段，加载时自动填充默认值。

## 录制流程（macOS）

1. 前端调用 `capture_key`。
2. 后端确保 Accessibility EventTap 已启用，进入 capture 模式。
3. EventTap 吞掉所有按键，并把下一次 KeyDown 的 macOS keycode 发回。
4. 后端通过 `macos_keycode_to_usage` 转成 HID usage。
5. 前端检查是否与其他动作冲突，然后自动调用 `update_settings` 保存并应用。
6. Esc 或界面上的取消按钮会终止录制。

录制复用已有的键盘控制权限，不额外要求 Input Monitoring。

## 冲突与回退

- 同一个按键不能绑定到两个动作；前端会提示冲突并保留原绑定。
- 非标准键盘按键（没有对应 HID usage）无法录制，会提示换一个标准按键。
- Windows 默认只有语音键；Home/TV/返回需要 `interception` feature 和驱动。
- Linux 需要 `input` 组读 `/dev/input/event*`、`/dev/uinput` 写权限。
