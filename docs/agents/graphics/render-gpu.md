# 组件契约: render、shell、GPU 与性能

渲染相关的契约分四篇, 改接口先改对应那篇:

| 文件 | 内容 |
| --- | --- |
| [render.md](render.md) | `src/render` 的 Rust 接口: 文档、DOM、出帧、canvas 2D、窗框、动画曲线 |
| [glass.md](glass.md) | 模糊链与系统玻璃 (`--acde-glass`): 预设、材质表、玻璃组、透镜 |
| [shell.md](shell.md) | napi 类 `Shell`: 窗口系统对象、surface 与事件 |
| [gpu.md](gpu.md) | GPU 与性能的硬约束, `src/render` 的已知问题和测试数据 |
