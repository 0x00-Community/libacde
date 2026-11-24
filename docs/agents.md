# 给 Agent 的文档目录

`docs/agents/` 是权威版本, 随代码一起改. 每篇什么时候读:

| 什么时候读 | 文件 |
| --- | --- |
| 改说明或新增说明文档 | [agents/writing-docs.md](agents/writing-docs.md) |
| 第一次接手运行时, 要弄清各仓的分工、两种跑法 (会话 / 宿主窗口) 或进程与线程 | [agents/runtime/architecture.md](agents/runtime/architecture.md) |
| 找运行时的某个模块或目录归谁, 新加文件前确认放哪 | [agents/runtime/layout.md](agents/runtime/layout.md) |
| 改 C 接口 (`src/ffi.rs`、`include/acde.h`)、`Platform` 回调或 so 的导出表 | [agents/runtime/ffi.md](agents/runtime/ffi.md) |
| 改窗口表、窗口模式与状态、焦点 / 模态 / grab、全局快捷键, 或窗口协议的去向 (会话里归 acdes 的 acompositor) | [agents/runtime/window-model.md](agents/runtime/window-model.md) |
| 改包清单的 `acde` 字段、桌面服务、契约级别 `apiLevel`、选哪个桌面、交出会话或登录页 | [agents/runtime/packages.md](agents/runtime/packages.md) |
| 写一个桌面或换桌面: 桌面包声明什么、能用哪些 `acde:` 模块、窗口 / 虚拟桌面 / 面板 / 窗框 / 快捷键 / 锁屏页 / 拖放 / 截图 / 服务的规则 | [agents/runtime/desktop.md](agents/runtime/desktop.md) |
| 改 SDK `@acde/api` (`acde-api/`, 只有类型声明) 或清单规则 (`manifest/`), 或在别的工程里引用它们 | [agents/acde-api.md](agents/acde-api.md) |
| 改 web 的出帧、窗框、钉住的面板、`acde:` 模块的注入 (层级、能力、批准)、沙箱或 web 的生命周期 | [agents/runtime/webs.md](agents/runtime/webs.md) |
| 改应用入口和脚本加载, 或某个浏览器接口 (表单控件、XML、通知、声音、视频、设备、摄像头和麦克风) | [agents/web/web-apps.md](agents/web/web-apps.md) |
| 改网页存储 (localStorage、IndexedDB、cookie、Cache API、`navigator.storage`) | [agents/web/storage.md](agents/web/storage.md) |
| 改应用后端的起停、代理变量、Windows 后端契约, 或 fetch / XHR / WebSocket 的路由 | [agents/web/backend.md](agents/web/backend.md) |
| 改 `acde:fs`、应用的根、拖放和打开请求的句柄、文件夹和文件选择器 | [agents/web/fs.md](agents/web/fs.md) |
| 改默认应用、文件类型判断、打开方式或 Windows 的文件关联 | [agents/web/default-apps.md](agents/web/default-apps.md) |
| 改 `acde:inspect` 的语义树、ref 和操作, 或原生窗口的 AT-SPI | [agents/web/inspect.md](agents/web/inspect.md) |
| 改 `src/render` 的接口、canvas 2D、窗框绘制、模糊、GPU 设备或性能预算 | [agents/graphics/render-gpu.md](agents/graphics/render-gpu.md) |
| 改 WebGL (`src/webgl`、`js/dom/src/webgl/`) 或 GLSL 前端 | [agents/graphics/webgl.md](agents/graphics/webgl.md) |
| 改运行时自带的 JS (`js/`: 网页接口层、页面胶水、单应用启动器) 或它的打包 | [agents/runtime/layout.md](agents/runtime/layout.md)「js」 |
| 改 `vendor/blitz`, 或升 stylo / parley / html5ever / naga 这类跟着 blitz 走的依赖 | [agents/graphics/vendor.md](agents/graphics/vendor.md) |
| 构建运行时、在 Windows 上编、升 v8 或改依赖镜像、设环境变量、跑运行时的命令和 doctor、看日志和 `perf::` 计时 | [agents/dev/build-run.md](agents/dev/build-run.md) |
| 改 dev 的 IPC、调试请求 (eval / shot / route / clock / input …)、dev 日志或热重载 (运行时一侧) | [agents/dev/dev-ipc.md](agents/dev/dev-ipc.md) |
| 改 DevTools 端点、页面一侧的 CDP 域 (DOM / CSS / Network / Overlay)、断点或重载不断开 | [agents/dev/devtools.md](agents/dev/devtools.md) |
| 写运行时的回归测试、跑或更新 WPT 期望 (`scripts/wpt/`; CI 在 acdev 的 `.github/workflows/test.yaml`) | [agents/dev/testing.md](agents/dev/testing.md) |
| 把 `libacde` 发到 cargo 源 ac, 或了解运行时怎么随 acdev 发版 | [agents/dev/release.md](agents/dev/release.md) |
