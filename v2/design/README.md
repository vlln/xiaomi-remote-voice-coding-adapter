# Design assets

- `remote-2pro.jpg` — 用户提供的原始遥控器白底图，保留作为裁剪来源。
- `../public/remote-2.jpg` — 应用内使用的裁剪图（原图 `450,120–760,1080`）。
- `../public/app-icon.png` — 1024×1024 应用图标源图，由 `pnpm tauri icon` 生成各平台尺寸。
- `../tools/align-hotspots.html` — 拖动橙圈生成按键热区百分比。

修改图片或热区后，运行：

```bash
cd v2
pnpm tauri icon public/app-icon.png
pnpm build
```
