# Changelog

## 2.4.0

### Changed

- 概览删除日志，按键改为箭头映射，新增音频摘要和跳转
- 按键测试改为实时模式：按下遥控器按键时左图高亮，并显示映射结果
- 支持添加其他遥控器按键的自定义映射（来源/目标/删除/启用）
- 保存提示只在有未保存更改时出现，toast 2 秒后自动消失
- 高级页新增作者和 GitHub 跳转
- 新应用图标：橙色底 + 白色遥控器
- 修复音频处理切换导致 tokio oneshot 重复 poll 崩溃

## 2.3.0

### Changed

- 按键映射改为「遥控器按键固定 → 任意目标键」：Home/TV/返回可点击目标键后按键盘按键重设
- 新增「测试按键」：按下遥控器按键，实际发送映射并显示结果
- 新增音频处理：降噪、噪声门、增益（0–18 dB），修改后自动重启音频桥接
- 音频页明确虚拟音频设备需要单独安装，并区分已安装/缺失状态
- 缩短页面说明文本，减少无效提示
- 统一中英文：系统级映射、设备控制与数据访问、CABLE 输入/输出、退格等

## 2.2.0

### Changed

- 切换到小米风格浅色 UI：白底、橙色强调、圆角卡片
- 使用小米遥控器 2 白底产品图作为按键映射可视化，可在图上点击语音 / 返回 / Home / TV 按钮录制
- 按键方向明确为「实体按键 → 映射目标」，例如「语音键 → 右 Option」
- 按平台显示目标键名：macOS 右 Option，Windows/Linux 右 Alt
- 录音不再展示路径或保存开关，只缓存最近一次并提供回放按钮
- 音频页移除手动刷新，设备列表自动轮询
- 快速设置增加平台化指引和跳转（BlackHole / VB-CABLE / PipeWire / 权限 / 蓝牙配对）
- 产品命名统一为「小米遥控器 2」

## 2.1.0

### Changed

- 全新的侧边栏布局：概览 / 按键映射 / 音频 / 高级，减少仪表盘式的拥挤感
- 按键映射改为「点击后按遥控器按键」录制，支持自定义语音 / Home / TV / 返回键
- 按键修改即时保存并应用，不再需要手动点「应用映射」
- 快速设置改为三步清单，直接提供安装 / 授权 / 启动入口
- 开发服务器固定绑定 127.0.0.1，避免 macOS WebView 解析 localhost 失败

### Internal

- 按键绑定以 HID usage 存储，macOS / Linux / Windows 后端共享
- 新增 `macos_keycode_to_usage` 反向表
- 修复 Windows 可选 Interception 后端的按键状态保留

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
