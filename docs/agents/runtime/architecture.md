# 架构: 各仓分工、两种跑法、进程与线程

| 仓库 (gitea AC/) | 是什么 | 产出 |
| --- | --- | --- |
| libacde (本仓库, 包名 `libacde`, 库名 `acde`) | 通用运行时逻辑: V8、DOM、render、存储、媒体、WebGPU / WebGL 绑定; 平台特化的接口 (`src/host/platform.rs` 的 `Platform` / `Window`, 合成器接口) | 根包 `libacde`: rlib + `libacde.so` + `include/acde.h` |
| [acdev](http://gitea.dopamine/AC/acdev) | 根上的 `src/`、`crates/hostwin` 是跨平台调试运行时 (宿主窗口、DevTools, `acdev dev` 拉起的就是它); `npm/` 是开发者命令行 `acdev`; 另有在 QEMU 里跑 Dopame OS 的模拟器 | 二进制 `acdev runtime`、命令行 `acdev` |
| [acdes](http://gitea.dopamine/AC/acdes) | Dopame OS 的系统态运行时: 合成器与窗口管理 acompositor、DRM/KMS、会话、管家、RDP、系统服务; 网页的光栅化 (`crates/paint`) 用本库的 wgpu / vello 或 detour, 没有 GPU 时在 CPU 上画 | 二进制与 Arch 包 `acdes` |

winman 是正在开发的独立窗口管理器, 还没接管; 会话里的合成与窗口管理现在都在 acdes 的 acompositor.

平台特化的部分只经接口进来: 前端 (acdev runtime / acdes 的 main) 实现 `Platform` (认领窗口 surface、文件选择器、`native` 上多挂的导出、内容类型),
交给 `acde::start` / `acde::common`; 运行时把它放在每个事件循环上 (`rt::Loop::platform`, web 线程从 Broker 那里继承), 不用全局变量.
根包 `libacde` (仓库根的 `Cargo.toml` + `src/`) 是对外唯一的包, 前端只依赖它 (acdev 和 acdes 并排检出本仓库: `libacde = { path = "../libacde" }`, 代码里 `use acde::…`). 代码都在这一个包里:
`src/host` 的公开项由 `src/lib.rs` 平铺导出, 其余是公开模块 `acde::log` / `media` / `model` / `napi` / `render` / `runtime` / `storage` / `vello` /
`webgl` / `gpu` (模块内互相引用写 `crate::render::…`); `acde::gpu` (src/gpu) 是设备登记 (`Gpu`) 和合成器接口, 平铺导出
2D 场景类型 (workspace 里的 `acde-scene`, `scene/`); blitz fork 直接画进这个场景. GPU 后端按 feature 选 (用户定): wgpu (src/vello, acdev 只开它, 管三个平台的一致) 和 detour
(还没有, acdes 开 detour 加 wgpu: Xe3 上走 detour, 别的 GPU 走 wgpu); 以前的 acgpu / detour-gpu 0.1 不再用. feature `napi` 打开 model 的 `#[napi(object)]`. 测试在根 `tests/` (`<原模块>_<名字>.rs`, 夹具 `tests/fixtures`,
runtime 的手动检查脚本 `tests/runtime/`), Wayland 协议在 `protocol/`. crate-type 是 rlib + cdylib, cdylib 即 `libacde.so`.

C 接口见 `docs/agents/runtime/ffi.md`; 发布到 cargo 源 ac 见 `docs/agents/dev/release.md`「cargo 源 ac」.

外部 crate 用的绑定工具在 lib 根上平铺导出 (`acde::{string, get, set, offload, failure, current, Msg, methods!, …}`).
会话里的合成器、窗口客户端与光栅化在 acdes; vendor (只剩 blitz 子模块) 仍在本仓库, 前端的 Cargo.toml 里不再 `[patch]` 它.

运行时和开发者命令行 `acdev` 的关系像 workerd 和 wrangler: 运行时跑桌面和应用, `acdev` 建工程、构建、装包, `acdev dev` 拉起调试运行时.
桌面是一张网页: JS 在内嵌的 V8 里 (不用 Node), Blitz (`vendor/blitz`) 排版, vello 在 GPU 上画; 不监听端口 (RDP 后端除外).
具体的桌面由桌面包提供 (例如 [dopame](http://gitea.dopamine/AC/dopame), 发行时带上, 运行时不认识它).

两种跑法, 运行时是同一套:

| 跑法 | 命令 | 平台 | 输出 | 合成器 |
| --- | --- | --- | --- | --- |
| **会话** (Dopame OS) | `acdes` (登录页起) | Linux | 屏幕 (DRM/KMS) 或 RDP | acdes 的 acompositor |
| **调试桌面** (`acdev dev`) | `acdev runtime desktop <id \| 包目录>` | Linux / Windows / macOS, 在别人的桌面里 | **一个**宿主原生窗口, 里面合成桌面和所有窗口 | acdev runtime 的 crates/hostwin |
| **调试窗口** | `acdev runtime app <id>` | 同上 | 一个宿主原生窗口, 所选应用铺满, 关闭该应用后退出 | 同上 |

选哪种看跑的是哪个二进制: `acdev runtime` 一律宿主窗口, `acdes` (只在 Linux; `/usr/bin/acdert` 是指向它的兼容链接) 一律会话, 怎么起见 acdes 的 `docs/agents/architecture.md`. 会话里的 X11 程序经 Xwayland 跑, 见同一篇「X11 程序」.

调试桌面用桌面的模块、服务和 `acde:windows` / `acde:desktop` 事件. 调试窗口用清单授权的模块和 `acde:window` 事件.
宿主窗口的 `native.Shell` 带 `hosted: true`, acde-host `Main.session` 见到它就不算「自己的会话」: 不起 fcitx5、
不开 AT-SPI、不跑 XDG autostart、不拿会话总线名字、不记登录选的桌面. 平台上没有的能力不注入, 不给假的实现.

宿主窗口的 `app` 模式从已安装的包发现应用, 按原规则选择桌面包来加载服务和资源, 并仅启动应用页面. 用 `app <id> --desktop <桌面 id 或包目录>`
指定服务来源. 应用按清单取得能力; 需要桌面页面主动发布状态的功能在此模式下没有发布者. 应用的开发重载保留宿主窗口.

## 进程与线程

```
会话: 会话进程 (acdes `crates/session`)、合成器线程和运行时子进程怎么分在 acdes 里, 见它的 docs/agents/architecture.md.

宿主窗口:
运行时 (一个进程)   主线程: 平台事件循环 (Windows win32, 别处 winit); `acdev` 线程 (栈 64 MiB): Broker;
                    每个 web 一条 JS 线程 + 一条出帧线程 `acde-host-<id>`, 它的每个 Web Worker 再一条 `acde-worker`;
                    `acde-compose` 合成到宿主窗口的 surface
```

- **会话进程**: 在 acdes 的 `crates/session`, 起停和重起运行时子进程的规则见它的 `docs/agents/architecture.md`「失败域」.
- **web** (src/host `web.rs` `spawnWeb`): runtime 里的一条 JS 线程 `acdev` (栈 8 MiB), 自己的 isolate (堆上限 `heapMb`) 和事件循环.
  和 Broker 之间是结构化克隆的字节, 投进对方事件循环 (没有 socket). `terminate` 置停止并 `terminate_execution` (退出码 137);
  线程 panic 只结束这个 web (134, `panic: …`). 句柄带 `busyMs` (当前任务跑了多久, 判卡死) 和 `cpuMs` (`/proc/self/task/<tid>/schedstat`).
  段错误、abort 带掉整个 runtime (主进程重起它), 所以共享的 Rust 状态里不写 `unsafe`, 绑定层不 `unwrap`, 每个绑定在边界上
  `catch_unwind`.
- **Web Worker** (src/host `worker.rs`, 见 `docs/agents/web/web-apps.md`「Web Worker」): web 线程起的 `acde-worker` 线程 (栈 8 MiB), 自己的 isolate
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
  `import('acde:<名字>')` 直接失败. Broker 自己只用 napi `Finder` 的 `locate` (`docs/agents/web/default-apps.md`「默认应用」), 是另一份.
  terminal 起用户的登录 shell, 在家目录: unix 是账户数据库里的 shell (`getpwuid_r` 的 `pw_shell`, 不可执行时 `$SHELL`, 再不行 `/bin/sh`), 带 `-l`, pty
  子进程 exec 前关掉 3 以上的所有 fd, 每秒从 `/proc` 报 `cwd` (只在 Linux); Windows 用 ConPTY (runtime `conpty.rs`), shell 依次取 Windows Terminal
  默认配置的 `commandline` (`settings.json`, 商店版、预览版、非打包版三处)、PATH 上的 `pwsh.exe`、Windows PowerShell、`%ComSpec%`; 子进程不继承
  任何句柄, 整棵进程树放进一个关句柄即结束的作业对象: 关终端时先关伪控制台 (shell 收到 `CTRL_CLOSE_EVENT`), 3 秒后还在就结束整个作业, 会话
  线程退出时作业句柄关闭, 后台留下的进程一并结束; 没有 `cwd` 事件, `exit` 的 `signal` 总是 null.
  终端的实现可由前端提供 (用户定, 伪终端挪到 acdes): 库里的接缝是 `acde::terminal` 的 trait `TerminalHost` (`open` / `input` /
  `resize` / `close` / `listen` / `shells` / `limits` / `close_all`), 类型都是强类型: `open` 回终端编号, `resize` 回实际的 `(cols, rows)`, `shells`
  回 `Shell {path, name, login}`, `limits` 回 `Limits {max, open}`, 事件是枚举 `TerminalEvent` (`Output` / `Cwd` / `Exit` / `Foreground`), 由绑定
  转成页面的 JSON 形状 (`acde-api` 的 `types/terminal/index.d.ts`、`events.d.ts`). 前端在 `acde::start` 之前调一次 `acde::terminal::provide(Box<dyn TerminalHost>)`
  登记 (照 `render::gpu::set_cpu_raster`): 第一次取实现 (第一个拿到 terminal 能力的 web) 时选定, 之后不再换, 晚来的 `provide` 回 false;
  `acde::exit` 调选定实现的 `close_all`, 所以实现要在那里挂断所有终端、不等待 (选定的实现在静态量里, 不会被释放; 前端不经 `acde::exit` 结束时没人收尾, acdes 的 `main` 总以 `acde::exit` 结束). trait 的方法都在页面的 JS 线程上同步执行, 实现不能阻塞 (起进程、读
  `/etc/shells` 这类快操作可以, 慢的自己放到别的线程). 绑定只解析参数形状: `cols`、`rows` 原样交给实现 (NaN、0、过大由实现检查); `open` 第三个参数
  `{command?, cwd?, env?}` 读成 `OpenOptions`, 不是对象、是函数、或无法转成 JSON (BigInt、循环引用) 回 400 `invalid_request`, 某个字段形状不对回 400,
  `code` 是该字段名 (`command` / `cwd` / `env`, 与实现方按规则拒绝时同一个 code); 规则与校验归实现方, 拒绝用 `Refusal {status, code, message}`
  表示, 页面拿到 `code` 与 `message`. 没有登记时用上面这份旧实现作缺省 (不收 `open` 的选项, 给了回 400 `unsupported`; `shells()` 回空表,
  `limits()` 回 `{max: 8, open}`); acdes 登记自己的实现并验收后, 旧实现与 ConPTY 删掉, 宿主窗口方式不再提供终端.
  network 第一次调用时连 NetworkManager; office 由启动器在会话授予 office 时 (`grants.office`, 起页面前) 调
  `sys.openOffice()` 开 (桥的套接字 `$XDG_RUNTIME_DIR/acde/office/office.sock` 一个会话只有一个, 同时有两个声明 office 的进程时后起的那个桥起不来; 文档服务另需 ONLYOFFICE Document Server; `office.state` 的 `server.problem`: `unreachable` 连不上, `missing` 是本机地址连不上而且没有 docker / podman 能起它, `unhealthy` 还没就绪, `secret` 没有共用的 JWT 密钥, `rejected` 密钥不对, `bridge` 桥没监听上, `runtime` 是运行时自己查状态失败 (例如执行器停了), 和 Document Server 无关; 起停脚本是 acdes 包里的 `/usr/lib/acdes/office-ds.sh`) (network / office / system 只在 Linux). `acde:system` (清单能力 `system`, src/host `system.rs` → runtime `system.rs`):
  这台电脑的信息 (主机名、系统、CPU、GPU、NPU、内存、开机时长、序列号)、各卷容量、时区和 NTP (经 timedated)、改时区 /
  NTP / 主机名 (经 hostnamed, 要 polkit 授权)、当前用户; 每次调用在 offload 线程上现读, 没有变化事件.
  `acde:bluetooth` (能力 `bluetooth`, src/host `bluetooth.rs` → runtime `bluetooth.rs`): BlueZ 的适配器和设备 (开关、可见、改名、
  扫描、配对 / 应答、连接、信任、屏蔽、忘记), 自己注册配对代理; `on('changed' | 'pairing')` 由 acde-host `bluetooth.ts` 包在原生 `watch` / `pairing` 上.
  登录时的开关 (`setOnLogin`: remember / on / off, 存在 `$XDG_CONFIG_HOME`) 只在自己的会话里由 Broker 调 `sys.restoreBluetooth()` 应用, 宿主窗口和登录页不碰.
  `acde:audio` (能力 `audio`, 原生名 `sound`, src/host `sound.rs` → runtime `sound.rs`): PipeWire 上的输出 / 输入设备、端口、
  声卡配置和各应用的流 (默认设备、音量、静音、平衡、端口、配置、把流挪到别的设备); `on('changed')` 由 acde-host `watched.ts` (`Watched.module`,
  只有 changed 事件的模块共用) 包在原生 `watch` 上.
  `acde:devices` (能力 `devices`, src/host `devices.rs` → runtime `devices/`): UDisks2 上的可移动驱动器 (挂载、卸载、弹出)、
  CUPS 打印机 (经 IPP: 默认打印机、暂停 / 恢复、任务和取消)、自动挂载设置 (登录时 / 插入时 / 未知设备, 每个设备单独设; 存在 `~/.config/acde/automount.json`, 默认插入时挂载、未知设备也挂, 登录时不挂, 用户定, 设置里可关; 文件里缺的键按默认); 事件 `changed` / `added`
  (新插入的驱动器) 由 acde-host `devices.ts` 包在原生 `watch(type)` / `unwatch(id)` 上. 自动挂载的监视只在自己的会话里由 Broker 调
  `sys.startDevices()` 起.
  `acde:power` (能力 `power`, src/host `power.rs` → runtime `power/`): UPower 的电池、power-profiles-daemon 的模式、
  按交流 / 电池 / 低电量分组的策略设置 (空闲动作、熄屏、合盖、电源键、亮度) 的读写, logind 挂起 / 休眠, 背光亮度, 充电上限; 事件 `changed` /
  `prompt` / `critical` 由页面胶水 `Watched.typed` 按 `type` 分发. 系统总线连接挂在 `Power` 上.
  **策略不在 libacde** (用户定, libacde 工单 08): 电源键、合盖、空闲、低电量由 acdes 的会话总管决定并执行, 它用这里公开的底层件
  (`acde::runtime::power::{backlight, bus, settings, shared}`, 机器能力用 `settings::Machine::probe`). 要问用户时 (`prompt`、`critical`)
  管家给桌面页推 `{type: 'power', event}`, 启动器原样转给页面, 页面 (`worker.ts` 的 `WatchFeed`) 把 `event` 交给 `acde:power` 的监听者,
  和原生 `watch` 来的事件一样; 页面拿不到 `acde:power` 时丢掉.
  设置存在 `$XDG_CONFIG_HOME/acde/power.json`, 由多个进程共用: 每个用到 `Power` 的页面进程都用 inotify 盯着它所在的目录
  (`power/shared.rs` 的 `watch`, acdes 也用它), 文件被写或换掉就重读, 所以桌面页改的设置在会话总管里立即生效.
  会话总管跑策略时写标记 `$XDG_RUNTIME_DIR/acde/power-policy` (`shared::PolicyMark`, 内容是 pid, 停时删); 页面的
  `PowerState.policy` 是标记里的进程还活着.
  `acde:input` (能力 `input`, src/host `input.rs`): 合成器的 xkb 布局 / 型号 / 选项、按键重复、NumLock 和鼠标 / 触控板的 libinput 设置;
  `state()` / `set(patch)` / `on('changed')` 走前端 `Platform::input()` 给的 `Input` (acdes 会话里由窗口客户端经 acompositor `acompositor_desktop_v1` v5 的输入设置提供, repo-split 第一阶段接上;
  acdev runtime 没有, 调用就抛), `catalog()` 读 xkeyboard-config 的 `rules/evdev.xml` (runtime `XkbCatalog`).
  输入法 (`ime`, runtime `fcitx.rs`, 和 `ime.rs` 同一条会话 D-Bus): `state()` 多一项 `ime {running, current, group}`, `set({ime: {current?, group?}})`、
  `catalog()` 多一项 `imes`, 都经 fcitx5 的 `org.fcitx.Fcitx5` `/controller` `org.fcitx.Fcitx.Controller1`: 当前输入法 `CurrentInputMethod` /
  `SetCurrentIM`; 分组 `CurrentInputMethodGroup` 加 `InputMethodGroupInfo` / `SetInputMethodGroupInfo` (改分组时保留原有项的布局, 不认识的 id 报错);
  目录 `AvailableInputMethods` (名字取 nativeName, 没有时取 name; `short` 是 label; `languages` 是 languageCode). `on('changed')` 在分组变时
  (`InputMethodGroupsChanged` 信号) 和当前输入法变时 (约 1 秒一次轮询, 只在页面订阅了才起) 也发. fcitx5 不在 (登录页会话不起它、没有会话总线)
  时 `running: false`、`group: []`、`imes: []`, 写 `ime` 报错; 其余设置照常.
  `acde:display` (能力 `display`, src/host `display.rs`): `state()` 是前端 `Platform::display()` 给的输出表 (acdes 会话里来自 `wl_output` / `xdg_output`),
  每个输出可带 `bounds` (在整个桌面坐标里的位置和大小) 与 `workArea` (去掉面板预留边后的可用区域), 都是 `{x, y, width, height}`, 没有时为 null 或不带 (SDK `DisplayInfo`, 多屏方案 A, 数据由 acdes 写出), 这里原样转交不改;
  内置屏的 `brightness` 由这里用 `acde:power` 的背光补上; `configure(id, {scale})` 在 acdes 会话里经 acompositor `acompositor_desktop_v1` v5 的 `set_scale` (acompositor 所有屏共用一个缩放),
  `setBrightness(id, v)` 只对内置屏 (logind 背光), `setNightLight` 还没有. 两者都是进程内模块, 不经 Broker; Linux only. `acde:office` 的根就是调用方 `acde:fs` 的根 (#96, `OfficeService::for_web` 带上这个 web 的 `Workspace`,
  runtime `Workspace::finder_roots`): 第 1 级应用是 `private`、点名的公共位置和选的文件夹, 按 `id` 认, 不给路径; 没有 `fs` 的应用没有根. 没批准或撤销了的
  公共位置和文件夹和 `acde:fs` 一样经 `FileAccess.guard` 拒绝 403 `denied`、从 `state().roots` 去掉. `state(handle)` 打开借来的文件 (web 侧 `PageCalls.lentFile` 只认
  `lent-file:` 句柄, 交名字给原生 `office.openLent(name)` → runtime `OfficeCall::Lent`, 按本应用的借出名找 `LentFile`), 文稿的 `root` 是空的, 只读借出只能查看;
  没有按路径打开的入口 (原 `office.open(path, writable)` 已删, 页面不能借它按任意路径读写). 打开的文稿按来源记会话 (`documents.rs` `Source::Root {owner, root}` /
  `Source::Lent {owner, file}`, 会话的同一性是根目录的设备号与节点号加相对路径, 或应用编号加借来文件的设备号与节点号和是否可写), 编辑器的取文件和保存回调凭签名的票据 (加密, 页面读不出内容): 票据里有会话键,
  根里的文稿另带来源 `{owner, root, path, mtimeMs}` (应用编号、根的 `id`、相对路径, 不带绝对路径). 按键找会话再经它的来源读写 (借来的文件夹有句柄没有路径, 也照样能开能存);
  运行时重启后会话没了, 根里的文稿按票据的来源重建: 在这个应用的 `Workspace` (`OfficeService::for_web` 时按应用编号记下) 里按根的 `id` 找回根, 在句柄下打开.
  借来的文件重启后句柄已不在, 回调回 404, 编辑器里没存的修改要用户另存. 保存: 根里的文件按修改时间核对, 变了另存冲突副本; 借来的文件按修改时间核对后原地写,
  变了不覆盖、报保存失败 (没有地方放冲突副本). 借来的文件每次读写前核对它仍借给这个应用 (同一个名字、同一个 `Arc`), 撤销 (`unlend`) 之后已开的会话立刻读写不了,
  保存记为失败 (410, 日志写明已被收回).
- **还没做的**: 窗口 / 应用 / 会话表还在 Broker 的 JS 里 (`acde:windows` / `acde:apps` / `acde:window` / `acde:session` 经 RPC);
  桌面包服务的调用经 Broker; 外部内存 (ArrayBuffer、文档、GPU 缓冲) 不按 isolate 记账.

## 大块内存 (macOS)

macOS 26 的系统分配器 (xzone malloc) 把释放掉的大块 (1 MiB 以上) 留在进程里, 不还给系统: `vmmap` 里是 `MALLOC_LARGE (empty)`, 脏页,
算进 footprint, 空闲多久都在, `malloc_zone_pressure_relief` 也放不掉 (实测 0 字节). 页面 ⇄ 后端的请求体、响应体每次都是新的 1–7 MiB 块,
大小一轮换, 每种大小各留一批, 开发进程跑久了多出几百 MB. 所以 macOS 上 ≥ 1 MiB 的块不经 malloc, 直接向 Mach VM 要, 释放时立刻还
(`src/memory.rs`, 只在 macOS 编译):

- **Rust 全局分配器** `acde::memory::Alloc`: libacde 只提供, 由前端设成 `#[global_allocator]` (acdev 的 `src/main.rs` 在 macOS 上设; Linux 的 acdev
  用 jemalloc, acdes 自己选). 小于 1 MiB 或对齐大于 4 KiB 的交给 `std::alloc::System`; 其余按 16 KiB 取整 `mach_vm_allocate`, 释放用
  `mach_vm_deallocate`. 两边都是大块的 `realloc`: 变小还掉尾巴; 变大先在原块后面接着要 (`VM_FLAGS_FIXED`, 那里已有映射时失败而不覆盖),
  接不上再搬. 跨过 1 MiB 的 `realloc` 是分配、拷贝、释放. 分配和释放按同一个 `Layout` 判断走哪条路, 两边一致.
- **V8 的 ArrayBuffer 分配器**: `host/rt.rs` 建 isolate 时设 `array_buffer_allocator(memory::array_buffers())` (`v8::new_rust_allocator`).
  ≥ 1 MiB 的 backing store 同样走 Mach VM (新映射的页本来就是 0, 满足 `Allocate` 要清零), 小的照 V8 默认分配器用 `calloc` / `malloc` / `free`.
  V8 释放时传回分配时的长度 (`BackingStore` 析构用 `byte_length_`; 可变长 ArrayBuffer 和 wasm 内存走 V8 自己的页分配器, 不经这里), 所以按长度判断.
- `vmmap` 里这两类分别是 `Memory Tag 241` (Rust) 和 `Memory Tag 242` (ArrayBuffer) (`VM_MAKE_TAG`; `man mmap` 建议应用用 240–255).
- 代价: 大块每次重新映射, 第一次写要缺页清零. 微基准 (Apple M4, 单线程): 1.2–7 MiB 的 `vec![x; n]` 写满再放, 每块 52 → 210 µs; 按 64 KiB
  追加到 6 MiB 的 `Vec` 因为能原地变大反而快 (510–573 → 339–458 µs); 小块 (16–2048 字节)、`Vec` 增长到 128 KiB、10 万项的
  `HashMap<String, Vec<u8>>` 在噪声内不变. 8 线程同时做大块时慢得更多 (同一张 VM 表上的锁); 运行时里这种大块只在页面 ⇄ 后端的数据通道上,
  那里一次 4 MiB 的请求要几毫秒.
- 别的平台不变: Linux、Windows 的 V8 用默认分配器.

后端请求体从 JS 只拷一次: `host/backend.rs` 的 `request` 直接把 `Uint8Array` 的内容拷进 ntex 的 `Bytes` (`Bytes::copy_from_slice`), 交给
`FetchClient::request`; 原来先拷成 `Vec<u8>`, ntex 的 `Bytes::from(Vec)` 又拷一次.
