# 架构: 三个仓库、两种跑法、进程与线程

acdev (Agent Computer Desktop Environment) 是 Dopame 的桌面基座, 分三个仓库 (测试期间的架构, 用户定 2026-11-13):

| 仓库 (gitea AC/) | 是什么 | 产出 |
| --- | --- | --- |
| libacde (本仓库, 包名 `libacde`, 库名 `acde`) | 通用运行时逻辑: V8、DOM、render、存储、媒体、WebGPU / WebGL 绑定; 平台特化的接口 (`src/host/platform.rs` 的 `Platform` / `Window`, 合成器接口) | 根包 `libacde`: rlib + `libacde.so` + `include/acde.h` |
| acdev runtime ([acdev](http://gitea.dopamine/AC/acdev) 根上的 `src/`、`crates/hostwin`) | 跨平台调试运行时: 宿主窗口 (hostwin)、DevTools、`acdev dev` 拉起的就是它; 合成器用 OpenGL / Vulkan (wgpu) | 二进制 `acdev runtime` |
| [acdebin-system](http://gitea.dopamine/AC/acdebin-system) | Dopame OS 特化运行时: Wayland 合成器、DRM/KMS、会话、RDP、系统服务; 合成器是 smithay + GLES, 网页的光栅化 (`crates/paint`) 用本库的 wgpu / vello, 没有 GPU 时用 vello_cpu; Xe3 上的 detour 后端还没接上 | 二进制 `acdebin-system` |
| acdev 的 `npm/` | 开发者 CLI `acdev` (bun) | `acdev` |

平台特化的部分只经接口进来: 前端 (acdev runtime / acdebin-system 的 main) 实现 `Platform` (认领窗口 surface、文件选择器、`native` 上多挂的导出、内容类型),
交给 `acde::start` / `acde::common`; 运行时把它放在每个事件循环上 (`rt::Loop::platform`, web 线程从 Broker 那里继承), 不用全局变量.
根包 `libacde` (仓库根的 `Cargo.toml` + `src/`) 是对外唯一的包, 前端只依赖它 (acdev 和 acdes 并排检出本仓库: `libacde = { path = "../libacde" }`, 代码里 `use acde::…`). 代码都在这一个包里:
`src/host` 的公开项由 `src/lib.rs` 平铺导出, 其余是公开模块 `acde::log` / `media` / `model` / `napi` / `render` / `runtime` / `storage` / `vello` /
`webgl` / `gpu` (模块内互相引用写 `crate::render::…`); `acde::gpu` (src/gpu) 是设备登记 (`Gpu`) 和合成器接口, 平铺导出
2D 场景类型 (workspace 里的 `acde-scene`, `scene/`); blitz fork 直接画进这个场景. GPU 后端按 feature 选 (用户定 2026-11-19): wgpu (src/vello, acdev 只开它, 管三个平台的一致) 和 detour
(还没有, acdes 开 detour 加 wgpu: Xe3 上走 detour, 别的 GPU 走 wgpu); 以前的 acgpu / detour-gpu 0.1 不再用. feature `napi` 打开 model 的 `#[napi(object)]`. 测试在根 `tests/` (`<原模块>_<名字>.rs`, 夹具 `tests/fixtures`,
runtime 的手动检查脚本 `tests/runtime/`), Wayland 协议在 `protocol/`. crate-type 是 rlib + cdylib, cdylib 即 `libacde.so`.

C 接口见 `docs/runtime/ffi.md`; 发布到 cargo 源 ac 见 `docs/dev/release.md`「cargo 源 ac」.

外部 crate 用的绑定工具在 lib 根上平铺导出 (`acde::{string, get, set, offload, failure, current, Msg, methods!, …}`).
会话里的 shell / window / paint / supervise 已搬到 acdebin-system; vendor (只剩 blitz 子模块) 仍在本仓库, 前端的 Cargo.toml 里不再 `[patch]` 它.

运行时和开发者 CLI `acdev` (acdev runtime ([acdev](http://gitea.dopamine/AC/acdev) 根上的 `src/`、`crates/hostwin`) 的 `npm/`)
的关系像 workerd 和 wrangler: 运行时跑桌面和应用, acdev (bun) 建工程、构建、装包, `acdev dev` 拉起 acdebin-dbg. 改名成 acdert 的只有程序、Arch 包和
crate; `acde:` 模块、`@acde/api`、`acde_*` 协议、`ACDE_*` 环境变量、`$XDG_*/acde/` 这些都不改. 桌面是一张网页: JS 在内嵌的 V8 里
(不用 Node), Blitz (`vendor/blitz`) 排版, vello 在 GPU 上画; 不监听端口 (RDP 后端除外). 具体的桌面由桌面包提供 (例如
[acde-dopame](https://github.com/dopame-org/acde-dopame), 发行时带上, 运行时不认识它).

两种跑法, 运行时是同一套:

| 跑法 | 命令 | 平台 | 输出 | 合成器 |
| --- | --- | --- | --- | --- |
| **会话** (Dopame OS) | `<运行时> --desktop <id>` (登录页起) | Linux | 屏幕 (DRM/KMS) 或 RDP | acdebin-system 的 `crates/shell` |
| **调试桌面** (`acdev dev`) | `acdev runtime desktop <id \| 包目录>` | Linux / Windows / macOS, 在别人的桌面里 | **一个**宿主原生窗口, 里面合成桌面和所有窗口 | acdev runtime 的 crates/hostwin |
| **调试窗口** | `acdev runtime app <id>` | 同上 | 一个宿主原生窗口, 所选应用铺满, 关闭该应用后退出 | 同上 |

选哪种看跑的是哪个二进制: `acdev runtime` 一律宿主窗口, `acdebin-system` (只在 Linux; Arch 包里 `/usr/bin/acdert` 链到它) 一律会话, 怎么起见 acdebin-system 的 `docs/architecture.md`. 会话里只支持 Wayland: 没有 Xwayland, 只会 X11 的程序跑不了. 会话里只支持 Wayland: 没有 Xwayland, 只会 X11 的程序跑不了.

调试桌面用桌面的模块、服务和 `acde:windows` / `acde:desktop` 事件. 调试窗口用清单授权的模块和 `acde:window` 事件.
宿主窗口的 `native.Shell` 带 `hosted: true`, acde-host `Main.session` 见到它就不算「自己的会话」: 不起 fcitx5、
不开 AT-SPI、不跑 XDG autostart、不拿会话总线名字、不记登录选的桌面. 平台上没有的能力不注入, 不给假的实现.

宿主窗口的 `app` 模式从已安装的包发现应用, 按原规则选择桌面包来加载服务和资源, 并仅启动应用页面. 用 `app <id> --desktop <桌面 id 或包目录>`
指定服务来源. 应用按清单取得能力; 需要桌面页面主动发布状态的功能在此模式下没有发布者. 应用的开发重载保留宿主窗口.

## 进程与线程

```
会话: 主进程 (`supervise.rs`)、合成器线程和运行时子进程怎么分在 acdebin-system 里, 见它的 docs/architecture.md.

宿主窗口:
运行时 (一个进程)   主线程: 平台事件循环 (Windows win32, 别处 winit); `acdev` 线程 (栈 64 MiB): Broker;
                    每个 web 一条 JS 线程 + 一条出帧线程 `acde-host-<id>`, 它的每个 Web Worker 再一条 `acde-worker`;
                    `acde-compose` 合成到宿主窗口的 surface
```

- **主进程** (会话): 在 acdebin-system 的 `src/supervise.rs`, 起停和重起运行时子进程的规则见它的 `docs/architecture.md`.
- **web** (src/host `web.rs` `spawnWeb`): runtime 里的一条 JS 线程 `acdev` (栈 8 MiB), 自己的 isolate (堆上限 `heapMb`) 和事件循环.
  和 Broker 之间是结构化克隆的字节, 投进对方事件循环 (没有 socket). `terminate` 置停止并 `terminate_execution` (退出码 137);
  线程 panic 只结束这个 web (134, `panic: …`). 句柄带 `busyMs` (当前任务跑了多久, 判卡死) 和 `cpuMs` (`/proc/self/task/<tid>/schedstat`).
  段错误、abort 带掉整个 runtime (主进程重起它), 所以共享的 Rust 状态里不写 `unsafe`, 绑定层不 `unwrap`, 每个绑定在边界上
  `catch_unwind`.
- **Web Worker** (src/host `worker.rs`, 见 `docs/web/web-apps.md`「Web Worker」): web 线程起的 `acde-worker` 线程 (栈 8 MiB), 自己的 isolate
  (它的 web 的所有 worker 合用一份 `heapMb` 大的堆预算, CPU 算进 web 的 `cpuMs`) 和事件循环; 和页面之间交换 `Envelope` (序列化的字节加转移的 backing store, 不复制), 投进对方事件循环.
  唯一的 `unsafe` 是 `envelope.rs` 的 `Moved` 实现 `Send`: v8 的 `SharedRef<BackingStore>` 不是 `Send` (`BackingStore` 不是 `Sync`), 但转移后
  发送方已把 ArrayBuffer 分离、不再碰这块内存, 引用计数是 `std::shared_ptr` 的原子计数, 内存由 isolate 共享持有的分配器释放 (rusty_v8 总是
  设 `array_buffer_allocator_shared`), 所以交给别的线程安全; `envelope.rs` 的测试覆盖发送方 isolate 先没了的情况.
- **一个 V8, 每个窗口一个 isolate**: 内存 (各自的堆上限)、生死 (一个死了只掉它的窗口)、CPU (每个 isolate 一个线程, 同一优先级) 各管各的.
- **注入的系统能力** (`spawnWeb` 第 7 个参数 `{app, fs, terminal, network, system, office}`, Broker `#grants`: 桌面全给, 应用按清单): fs、terminal、
  network、office 和选文件夹在各 web 里绑定同一份 Rust 对象 (src/host `fs.rs` / `terminal.rs` / `network.rs` / `office.rs` /
  `picker.rs`; 阻塞的活经 `offload.rs` 交给别的线程, promise 在这个 web 的事件循环上落定; `acde:fs` 的改动 (mkdir、copy、rename、delete、
  writeFile、revoke) 在每个 web 一条的有序队列 `acde-fs-changes` 上按调用顺序跑, 读在全进程 4 条线程的 `acde-fs-<n>` 池里跑, 这个 web 还有改动
  没做完时读也排进有序队列, 所以没等的写之后的读看得到写的结果). 限制 (fs 的 roots) 在注入时绑进去, 没给的
  `import('acde:<名字>')` 直接失败. Broker 自己只用 napi `Finder` 的 `locate` (`docs/web/default-apps.md`「默认应用」), 是另一份.
  terminal 起用户的登录 shell, 在家目录: unix 是账户数据库里的 shell (`getpwuid_r` 的 `pw_shell`, 不可执行时 `$SHELL`, 再不行 `/bin/sh`), 带 `-l`, pty
  子进程 exec 前关掉 3 以上的所有 fd, 每秒从 `/proc` 报 `cwd` (只在 Linux); Windows 用 ConPTY (runtime `conpty.rs`), shell 依次取 Windows Terminal
  默认配置的 `commandline` (`settings.json`, 商店版、预览版、非打包版三处)、PATH 上的 `pwsh.exe`、Windows PowerShell、`%ComSpec%`; 子进程不继承
  任何句柄, 整棵进程树放进一个关句柄即结束的作业对象: 关终端时先关伪控制台 (shell 收到 `CTRL_CLOSE_EVENT`), 3 秒后还在就结束整个作业, 会话
  线程退出时作业句柄关闭, 后台留下的进程一并结束; 没有 `cwd` 事件, `exit` 的 `signal` 总是 null.
  network 第一次调用时连 NetworkManager; office 由启动器在会话授予 office 时 (`grants.office`, 起页面前) 调
  `sys.openOffice()` 开 (桥的套接字 `$XDG_RUNTIME_DIR/acde/office/office.sock` 一个会话只有一个, 同时有两个声明 office 的进程时后起的那个桥起不来; 文档服务另需 ONLYOFFICE Document Server) (network / office / system 只在 Linux). `acde:system` (清单能力 `system`, src/host `system.rs` → runtime `system.rs`):
  这台电脑的信息 (主机名、系统、CPU、GPU、NPU、内存、开机时长、序列号)、各卷容量、时区和 NTP (经 timedated)、改时区 /
  NTP / 主机名 (经 hostnamed, 要 polkit 授权)、当前用户; 每次调用在 offload 线程上现读, 没有变化事件.
  `acde:bluetooth` (能力 `bluetooth`, src/host `bluetooth.rs` → runtime `bluetooth.rs`): BlueZ 的适配器和设备 (开关、可见、改名、
  扫描、配对 / 应答、连接、信任、屏蔽、忘记), 自己注册配对代理; `on('changed' | 'pairing')` 由 acde-host `bluetooth.ts` 包在原生 `watch` / `pairing` 上.
  登录时的开关 (`setOnLogin`: remember / on / off, 存在 `$XDG_CONFIG_HOME`) 只在自己的会话里由 Broker 调 `sys.restoreBluetooth()` 应用, 宿主窗口和登录页不碰.
  `acde:audio` (能力 `audio`, 原生名 `sound`, src/host `sound.rs` → runtime `sound.rs`): PipeWire 上的输出 / 输入设备、端口、
  声卡配置和各应用的流 (默认设备、音量、静音、平衡、端口、配置、把流挪到别的设备); `on('changed')` 由 acde-host `watched.ts` (`Watched.module`,
  只有 changed 事件的模块共用) 包在原生 `watch` 上.
  `acde:devices` (能力 `devices`, src/host `devices.rs` → runtime `devices/`): UDisks2 上的可移动驱动器 (挂载、卸载、弹出)、
  CUPS 打印机 (经 IPP: 默认打印机、暂停 / 恢复、任务和取消)、自动挂载设置 (登录时 / 插入时 / 未知设备, 每个设备单独设; 存在 `~/.config/acde/automount.json`, 默认插入时挂载、未知设备也挂, 登录时不挂, 用户 2026-11-20 定, 设置里可关; 文件里缺的键按默认); 事件 `changed` / `added`
  (新插入的驱动器) 由 acde-host `devices.ts` 包在原生 `watch(type)` / `unwatch(id)` 上. 自动挂载的监视只在自己的会话里由 Broker 调
  `sys.startDevices()` 起.
  `acde:power` (能力 `power`, src/host `power.rs` → runtime `power/`): UPower 的电池、power-profiles-daemon 的模式、
  按交流 / 电池 / 低电量分组的策略设置 (空闲动作、熄屏、合盖、电源键、亮度) 的读写, logind 挂起 / 休眠, 背光亮度, 充电上限; 事件 `changed` /
  `prompt` / `critical` 由页面胶水 `Watched.typed` 按 `type` 分发. 系统总线连接挂在 `Power` 上.
  **策略不在 libacde** (用户 2026-11-18 定, libacde 工单 08): 电源键、合盖、空闲、低电量由 acdes 的会话总管决定并执行, 它用这里公开的底层件
  (`acde::runtime::power::{backlight, bus, settings, shared}`, 机器能力用 `settings::Machine::probe`). 要问用户时 (`prompt`、`critical`)
  管家给桌面页推 `{type: 'power', event}`, 启动器原样转给页面, 页面 (`worker.ts` 的 `WatchFeed`) 把 `event` 交给 `acde:power` 的监听者,
  和原生 `watch` 来的事件一样; 页面拿不到 `acde:power` 时丢掉.
  设置存在 `$XDG_CONFIG_HOME/acde/power.json`, 由多个进程共用: 每个用到 `Power` 的页面进程都用 inotify 盯着它所在的目录
  (`power/shared.rs` 的 `watch`, acdes 也用它), 文件被写或换掉就重读, 所以桌面页改的设置在会话总管里立即生效.
  会话总管跑策略时写标记 `$XDG_RUNTIME_DIR/acde/power-policy` (`shared::PolicyMark`, 内容是 pid, 停时删); 页面的
  `PowerState.policy` 是标记里的进程还活着.
  `acde:input` (能力 `input`, src/host `input.rs`): 合成器的 xkb 布局 / 型号 / 选项、按键重复、NumLock 和鼠标 / 触控板的 libinput 设置;
  `state()` / `set(patch)` / `on('changed')` 走前端 `Platform::input()` 给的 `Input` (acdes 会话里由窗口客户端经 winman `winman_desktop_v1` v5 的输入设置提供, repo-split 第一阶段接上;
  acdev runtime 没有, 调用就抛), `catalog()` 读 xkeyboard-config 的 `rules/evdev.xml` (runtime `XkbCatalog`).
  输入法 (`ime`, runtime `fcitx.rs`, 和 `ime.rs` 同一条会话 D-Bus): `state()` 多一项 `ime {running, current, group}`, `set({ime: {current?, group?}})`、
  `catalog()` 多一项 `imes`, 都经 fcitx5 的 `org.fcitx.Fcitx5` `/controller` `org.fcitx.Fcitx.Controller1`: 当前输入法 `CurrentInputMethod` /
  `SetCurrentIM`; 分组 `CurrentInputMethodGroup` 加 `InputMethodGroupInfo` / `SetInputMethodGroupInfo` (改分组时保留原有项的布局, 不认识的 id 报错);
  目录 `AvailableInputMethods` (名字取 nativeName, 没有时取 name; `short` 是 label; `languages` 是 languageCode). `on('changed')` 在分组变时
  (`InputMethodGroupsChanged` 信号) 和当前输入法变时 (约 1 秒一次轮询, 只在页面订阅了才起) 也发. fcitx5 不在 (登录页会话不起它、没有会话总线)
  时 `running: false`、`group: []`、`imes: []`, 写 `ime` 报错; 其余设置照常.
  `acde:display` (能力 `display`, src/host `display.rs`): `state()` 是前端 `Platform::display()` 给的输出表 (acdes 会话里来自 `wl_output` / `xdg_output`),
  每个输出可带 `bounds` (在整个桌面坐标里的位置和大小) 与 `workArea` (去掉面板预留边后的可用区域), 都是 `{x, y, width, height}`, 没有时为 null 或不带 (SDK `DisplayInfo`, 多屏方案 A, 数据由 acdes 写出), 这里原样转交不改;
  内置屏的 `brightness` 由这里用 `acde:power` 的背光补上; `configure(id, {scale})` 在 acdes 会话里经 winman `winman_desktop_v1` v5 的 `set_scale` (winman 所有屏共用一个缩放),
  `setBrightness(id, v)` 只对内置屏 (logind 背光), `setNightLight` 还没有. 两者都是进程内模块, 不经 Broker; Linux only. `acde:office` 的根就是调用方 `acde:fs` 的根 (#96, `OfficeService::for_web` 带上这个 web 的 `Workspace`,
  runtime `Workspace::finder_roots`): 第 1 级应用是 `private`、点名的公共位置和选的文件夹, 按 `id` 认, 不给路径; 没有 `fs` 的应用没有根. 没批准或撤销了的
  公共位置和文件夹和 `acde:fs` 一样经 `FileAccess.guard` 拒绝 403 `denied`、从 `state().roots` 去掉. `state(handle)` 按交来的文件句柄打开 (web 侧认
  `PageCalls` 登记过的句柄, 换成路径交给 runtime `OfficeCall::Open`), 文稿的 `root` 是空的, 只读句柄只能查看. 编辑器的取文件和保存回调凭签名的票据里的
  文件夹路径找文件 (`OfficeFiles::file` / `save` 不再查共享根).
- **还没做的**: 窗口 / 应用 / 会话表还在 Broker 的 JS 里 (`acde:windows` / `acde:apps` / `acde:window` / `acde:session` 经 RPC);
  桌面包服务的调用经 Broker; 外部内存 (ArrayBuffer、文档、GPU 缓冲) 不按 isolate 记账.
