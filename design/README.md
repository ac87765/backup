# 界面稿

客户端界面的设计稿以静态 HTML/CSS 保存在仓库中，是实现与验收的依据（取代此前的 Figma 文件）。

## 结构

```
design/
  capture.mjs            生成截图：node design/capture.mjs [desktop]
  desktop/               Windows / macOS 桌面客户端（openspec change: add-native-clients）
    tokens.css           设计令牌与基础组件（颜色、字号、间距、圆角、阴影、动效）
    shared.js            平台切换、状态切换、Lucide 图标、标题栏 / 侧边栏 / 状态栏
    *.html               每个界面一个文件，顶部注释列出全部状态
    screenshots/         每个界面、每个状态、两个平台（-win / -mac）的 2x 截图
```

在浏览器中直接打开 HTML 即可查看，用查询参数切换：`files.html?os=mac&state=transfers`。

## 实现约定（给实现方）

- **以 HTML 源码为准**：颜色、尺寸、间距、字号、文案都从源码中读取，截图只用于整体对照。
- **设计令牌**：`tokens.css` 的 `:root` 与 Web 端 `web/src/index.css` 一致，Flutter 中集中定义为一个令牌类，组件不得出现零散色值。
- **字体**：Windows 用 Segoe UI Variable + 微软雅黑；macOS 用系统字体（SF Pro + 苹方）。截图在 Windows 上生成，mac 版截图中的字体只作示意。
- **图标**：Lucide，线宽 2，名称见 `shared.js` 的 `ICONS`。
- **平台差异**：只在窗口外框（Windows 11 标题栏按钮 / macOS 红绿灯与居中标题）、托盘与菜单栏的说法、本地路径写法、系统钥匙串名称上不同，其余完全一致。
- **动效**：对话框 180ms 缩放淡入、开关 180ms，曲线 `cubic-bezier(0.2, 0, 0, 1)`；系统开启“减少动态效果”时关闭。
- **验收**：Flutter golden 截图与 `screenshots/` 中对应文件对照；布局、层级、文案一致，像素级差异（字体渲染、抗锯齿）不计。

## 修改流程

设计稿由审查方维护。修改后运行 `node design/capture.mjs` 重新生成截图并一起提交；实现方如果认为设计稿有问题，在汇报中提出，不直接修改。
