# 应用后端 (启动、Windows 后端契约、请求路由)

- **后端** (单应用启动器 `js/launcher/src/backend/backend.ts` 的 `AppBackend`, 原 acde-host `backend.ts` 改写; unix): PATH 上有 `bun` 就 `bun --preload acde-listen.js server.js`, 否则 `node --import <acde-listen.js 的 file: URL> server.js`
  (Node ≥ 22.3; `ACDE_RUNTIME=node` (acdev dev 按工程的运行时设) 时先找 node, 没有再 bun; 两样都没有不起, 打一行 warn; 同一份构建产物两边都能跑, 见 acdev 的「和运行时的契约」), 随应用起, 应用退出时停, 重载 (`acdev dev` 的 `devreload` 和 `window.reload`) 时先停再起 (SIGTERM 进程组,
  3 秒后 SIGKILL); 意外退出时打 warn `[<app>#backend] stopped (exit N); starting it again`, 0.5 秒后重起, 一分钟内第 4 次崩溃后打 warn 不再起, 直到下次重载.
  这些行的目标是 `acde_host`, 后端自己的输出是 `acde::host::child` (Windows `acde::host::backends`, 日志目标是模块路径) 的 `[<app>#backend] …`, `acdev dev` 都留下. `child.rs` 起: 自己的会话和进程组, 环境从零建 (`ACDE_APP_ID` / `ACDE_APP_DATA` /
  `ACDE_APP_SOCKETS` / `PORT` / `NODE_ENV=production` + 会话的 HOME、PATH、LANG 等), 工作目录 `$XDG_DATA_HOME/acde/apps/<id>/`, 输出前缀
  `[<app>#backend]`, `PR_SET_PDEATHSIG` (子进程都由 `acde-fork` 线程 fork). preload 把 listen 换成 unix socket (`<acdev 的运行时目录>/<运行时的 pid>/<id>/`, acdev 的运行时目录是 `sys/os.ts` `ownRuntimeDir`: 有 `XDG_RUNTIME_DIR` 或在 Linux 上是那里的 `acde/`, 别处 `$TMPDIR/acde-<uid>` 本身就是 acdev 的, 不再套一层 (macOS 的 `TMPDIR` 很长, 104 字节里每个字都要省; 这样应用 id 能比 0.0.9 长 4 个字): 每个会话一份, acdev dev 和 acdev test 同时跑
  同一个应用不再互相删 socket; 应用的后端停了删它的目录, 启动器退出时删整个 pid 目录, 第一次起后端时扫掉进程已不在的 pid 目录;
  `<端口>.sock` 的路径超过 `sun_path` (macOS 104 字节, Linux 108) 时不起, 打一行 warn. Linux 上没有 `XDG_RUNTIME_DIR` 时用
  `/run/user/<uid>`; `sys.pid` 给出运行时的 pid; 只扫纯数字的目录, 同一处的 `acde/` (旧布局、`exec`) 不动), 指定端口的 UDP bind 拒绝; 原生扩展开的端口管不到.
- **后端的代理变量** (runtime `proxy::backend_variables`, Broker 经 `sys.proxyEnv()` 并进后端环境, 各平台都有): 会话设了
  `http_proxy` / `https_proxy` / `all_proxy` 中任何一个, 就原样传 `http_proxy` / `https_proxy` / `all_proxy` / `no_proxy` 这几个变量 (unix 上大小写
  两种名字各自照传, Windows 环境变量名不分大小写, 用大写名). 一个都没设时给平台的手动代理, 和页面用的同一份系统设置: Windows 是用户
  Internet 设置的手动代理和绕过列表 (`WinHttpGetIEProxyConfigForCurrentUser`), macOS 是系统配置的 HTTP / HTTPS / SOCKS 代理和例外列表
  (`CFNetworkCopySystemProxySettings`); HTTP 代理给 `http://h:p`, Windows 的 `socks=` 给 `all_proxy=socks4a://h:p`, macOS 的 SOCKS 给
  `socks5h://h:p`; unix 上大小写两种名字都设. `no_proxy` 是会话的 `no_proxy` 加上系统的例外再加 `localhost,127.0.0.1,::1` (页面那边环回地址也永远直连);
  例外换成 `no_proxy` 的写法: `*.x` → `.x`, `192.168.*` → `192.168.0.0/16`, `169.254/16` → `169.254.0.0/16`, 其他带通配符的、带协议的、
  Windows 的 `<local>` 和 macOS 的“排除简单主机名”没有对应写法, 不给. 配置脚本和自动发现 (Windows 设了配置脚本, macOS 开了自动配置或
  自动发现) 是按 URL 回答的, 变量表达不了, 这时后端不得到系统代理; Windows 只开“自动检测设置”时照样给手动代理 (和页面在 WPAD 没找到时一致).
  每次起后端时读, 改了系统设置后新起的后端生效. Linux 没有系统来源, 只有会话的变量.
- **Windows 后端契约** (2026-11-09 定, 运行时实现在 src/host `backends/` 和 runtime `channel/`; acdev 的「和运行时的契约」写同一份, 两边一起发版). 不变量: 后端运行期间没有 TCP 监听、没有命名管道,
  套接字文件只有 SYSTEM 和当前用户能开; macOS / Linux 不变. argv、`ACDE_RUNTIME` 选 bun / node、环境从零建、一分钟 4 次崩溃上限、首窗起末窗停都同上.
  - **两种模式**: node 走**通道模式** (Node 在 Windows 上不能用文件系统 AF_UNIX, 命名管道的默认安全描述符给 Everyone / 匿名读、也接受远端打开);
    bun 走**套接字模式** (和 unix 一样, `Bun.serve` 语义完整). 环境: 通道模式 `ACDE_APP_CHANNEL=fd`, 不设 `ACDE_APP_SOCKETS`; 套接字模式
    `ACDE_APP_SOCKETS=<本代目录>` 加 `ACDE_APP_CONTROL=fd`, 不设 `ACDE_APP_CHANNEL`. 其余变量同 unix.
  - **进程**: `CreateProcessW`, 每个后端一个自己的作业对象 (关闭即终止, 后端派生的子进程一起在里面), 只显式继承列出的句柄
    (`PROC_THREAD_ATTRIBUTE_HANDLE_LIST`). 两种模式都另建两根匿名管道, 经 CRT 的 `lpReserved2` 交成子进程的 fd 3 (子进程读) 和 fd 4 (子进程写);
    单向匿名管道两根, 因为双工管道要有名字. 通道模式上面跑全部帧; 套接字模式只跑控制帧 (SHUTDOWN, 帧格式同下), 数据走 AF_UNIX. 运行时一侧一条线程阻塞写、一条阻塞读, 和 ntex 的 `IoStream` 之间只经互斥锁和 waker.
    Broker 用的原生接口多两个只在 Windows 有的: `sys.spawnBackend(argv, {cwd, env, prefix, app, mode, preload}, onExit)` (两种模式都用它, mode 是
    `'channel'` / `'sockets'`; 传输变量和 `SystemRoot`、`TEMP`、`LOCALAPPDATA` 等 Windows 程序要的变量由运行时补) 和 `sys.stopBackend(pid, ms)`.
    `AppBackend` 有它们就走这条路, node 是通道模式, bun 是套接字模式. 运行时退出时 `child::stop_all` 终止还在的后端并删本代目录;
    等待线程正在收尾的后端 (末窗关闭先发了 SHUTDOWN, 进程刚退出, 目录可能还要重试删除) 也等它收尾完 (`FINISHING` 锁) 再退出, 不留目录.
  - **通道帧** (大端): `type u8 | stream u32 | length u32 | payload`, 头 9 字节, payload 最多 64 KiB. 类型: OPEN 1 (运行时开一条流, payload 是
    u16 逻辑端口, 即 `listen(P)` 的 P)、DATA 2 (字节, 运行时按 16 KiB 分块)、FIN 3 (这一方向写完, 半关闭)、RST 4 (中止这条流, payload 是 UTF-8
    原因)、WINDOW 5 (payload u32, 给对方追加的信用字节数)、SHUTDOWN 6 (流号 0, 无 payload, 运行时请后端优雅退出). 流号由运行时分配, 不复用;
    每条流每个方向初始信用 256 KiB, 发 DATA 扣信用, 收方把数据交出去后发 WINDOW 补回. 不认识的类型忽略; 不存在的流上的 DATA / FIN 回 RST;
    P 上没有登记 server 时预加载回 RST. 两端都在 FIN 排空后才算关闭, RST 只用于中止. 流量控制在运行时一侧按 ntex 的读写高低水位.
  - **预加载 (acdev 的 `acde-listen.js`)**: 通道模式把每个 OPEN 交给 `listen(P)` 登记的 server 当一条连接 (HTTP 和 WebSocket 升级都走它), 不真正监听;
    读 fd 3 用专用循环, 不长期占公共线程池. 套接字模式和 unix 一样把 `listen(P)` / `Bun.serve` 换成 `<ACDE_APP_SOCKETS>\<P>.sock`
    (Bun 1.4.4 在 Windows 上 `Bun.serve({unix})` 和 `node:http` 的 `listen(路径)` 都建 AF_UNIX 套接字, 不是命名管道), 有 `ACDE_APP_CONTROL=fd`
    时读 fd 3 上的控制帧. 预加载不改目录的 ACL.
    支持通道的 `acde-listen.js` 含字符串 `acde-listen-channel/1`; 运行时起 node 前查这个标记, 没有就不起, 打 warn
    `not started: acde-listen.js does not support the Windows channel (built by an older acde: acdev build it again)`.
  - **套接字目录**: 每一代 (每次起, 含崩溃后重起) 新建一个短目录 `<base>\<运行时 pid>.<6 位随机十六进制>`, base 是 `$XDG_RUNTIME_DIR\acdev\b`,
    没设时用户临时目录下的 `acdev\b` (`std::env::temp_dir()`, 即 `GetTempPath2W`: `%TMP%` / `%TEMP%`, 一般是 `%LOCALAPPDATA%\Temp`). 建目录时就带上受保护的 DACL (不先建后改): 只有 SYSTEM 和当前用户, 完全控制, 对象和容器继承, 关闭从上级继承
    (SDDL `D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;<用户 SID>)`); 套接字文件继承它 (实测 `D:AI(A;ID;FA;;;<用户>)(A;ID;FA;;;SY)`). AF_UNIX 路径最多
    108 字节 (`sun_path`, UTF-8, 含结尾 NUL): 运行时用 `65535.sock` 算最长路径, 超了不起并打 warn. 停止后删掉本代目录; 运行时第一次起后端前删掉
    base 下 pid 已不在的目录 (崩溃留下的). 被杀的 Bun 会留下套接字文件, 同一路径再 `Bun.serve` 是 `EADDRINUSE`, 所以每代用新目录.
    不用 `%LOCALAPPDATA%` 本身: 从 MSIX 打包应用 (例如 Claude 桌面版) 派生的进程 (含经 `Start-Process` / `cmd /c start` 再起的) 写
    `%APPDATA%` / `%LOCALAPPDATA%` 时被重定向到 `Packages\<包>\LocalCache`, 在那里建的 AF_UNIX 套接字连不上 (Bun 自己的客户端也一样);
    临时目录不被重定向. 实测 : 普通进程 (不在包环境里) 两处都能连, 包环境里只有临时目录能连.
  - **运行时连接**: 套接字模式和 unix 同一段: runtime `fetch/` 的 `ntex::rt::unix_connect` (Windows 上 ntex 的 IOCP 反应器用 socket2 建 AF_UNIX
    再 `ConnectEx`), 不用线程桥接; `Dial::check` 接受 Windows 的绝对路径. 通道模式经通道开流. web 初始化参数: 套接字模式照旧给 `sockets`
    (只表示有后端, 值是 base), 通道模式不给 `sockets`, 给 `channel: true` (`BackendPlace.init`). Windows 上两种 web 都按 `init.app.id` 拨
    `Dial::App`, 运行时的登记表 (runtime `channel::register`, 每代一个 `Route::Channel` / `Route::Sockets(本代目录)`) 给出当前一代, 所以重起后换了
    目录的 Bun 也能接上. 两种模式没就绪或拒绝连接时都重试 10 秒
    (通道: 还没注册; 套接字: `NotFound` / `ConnectionRefused`, 后者也是残留套接字的错误 10061), 新一代起来后接上.
  - **停止**: 两种模式都在 fd 3 上发 SHUTDOWN (预加载停止接受新连接、让在途请求做完、关 server 后退出), 等 3 秒 (`STOP_MS`), 然后终止作业对象
    (旧预加载不读控制帧, 就是等满 3 秒再终止). 后端退出时注销这一代的通道 / 目录, 上面还没完的流失败.
- **路由**: 页面的 `fetch` / `XMLHttpRequest` / `WebSocket` / `EventSource` 都由这个 web 自己的线程发 (src/host `backend.rs` 的 `natives.backend`,
  每个 web 都有 → runtime `fetch/` `FetchClient`: ntex 的 HTTP / WebSocket 客户端, I/O 在 `acde-rt` 线程; acde-dom `core/net/index.ts` 的 `route`). 去哪:
  - 同源资源 (GET / HEAD) → 按最长前缀在包目录和借出的目录里找 (src/host `dom.rs` `Assets::file`, 模块和 source map 也走它): 和渲染器的资源一样
    解开符号链接后核对仍在那个目录里 (`render/net.rs` `within`; 原来借出目录里指向外面的链接照读), 不在就当没有.
  - 同源非资源路径、有后端时的 `localhost:<port>` (和 127.0.0.1、[::1]、0.0.0.0) → 自己应用的 unix socket (`Dial::Socket`, `init.sockets`; 没出现或
    拒绝连接时重试 10 秒). 没有后端的 web 同源非资源路径失败, `localhost` 走下面的网络.
  - 别的主机 (`Dial::Url`, http / https / ws / wss) → 宿主的网络: 系统 DNS, TCP, https / wss 用 `http.rs` 的 `tls()` (只给 http/1.1 的 ALPN,
    验证证书和主机名). 不自动解压 (不发 `Accept-Encoding`).
    rustls 的加密实现: 本库自己建的 TLS 配置 (`tls()`, 代理隧道, 豆包语音的 wss) 都显式用 ring (`builder_with_provider`), 不看进程默认.
    但 ntex 4.9 的 `Client::builder()` 不论用不用都先按进程默认建一份 rustls 配置 (`ClientBuilder::new`, 之后才被 `.rustls(..)` 换掉),
    而前端 (如 acdes) 的依赖同时开了 ring 和 aws-lc-rs 时 rustls 不会自己选默认, 这一步就 panic.
    所以建 ntex 客户端之前 (`http::client()`、fetch 拨到后端用的客户端) 先调 `http::default_provider()`: 进程还没有默认时装上 ring,
    已有 (前端自己装了) 就不动. `WsClient::new` 的默认连接器只有 TCP, 不建 rustls 配置; wss 照旧显式 `.rustls(tls())`.
  - 代理 (runtime `proxy/`): 契约照 Linux, 设了 `http_proxy` / `https_proxy` / `all_proxy` (大小写都认, 小写优先) 中任何一个就只看环境变量,
    各平台都一样; 没设时问平台: Windows 是用户的 Internet 设置 (WinHTTP: 自动检测 WPAD 和配置脚本按 URL 问
    `WinHttpGetProxyForUrl`, WPAD 没找到后 5 分钟内不再问; 没有答案时用手动代理和它的绕过列表, `<local>`、通配符), macOS 是系统配置
    (CFNetwork, 例外列表由它处理, PAC 在 `acde-proxy` 线程的 run loop 上跑, 最多 10 秒), Linux 没有别的来源. `no_proxy` (名字含子域、
    IP、CIDR、`:端口`、`*`) 对系统设置也生效; localhost 和环回地址、应用后端永远直连. 求代理可能阻塞 (WPAD、PAC), 在 `acde-proxy` 线程上做.
    经代理时自己建隧道: HTTP 代理一律 `CONNECT host:port` (http 也是, 代理看不到请求本身; 主机名由代理解析), 或 SOCKS4a / SOCKS5
    (主机名交给代理); https / wss 再在隧道上和目标做 TLS (SNI 和校验用目标主机). 经代理的连接不进连接池 (每个请求一条). 按列表顺序试
    (PAC 的 `PROXY a; DIRECT`), 都失败时错误里写出每个代理和原因. 不支持: 代理认证 (407 原样报错)、`https://` 代理、Linux 桌面环境的代理设置.
  - 和浏览器一样的规则 (页面的源是 `http://acde.local`): 跨源请求带 `Origin`、`Referer: http://acde.local/`、`User-Agent`、`Accept-Language`;
    CORS (acde-dom `core/cors.ts`): 简单请求之外先发 `OPTIONS` 预检 (不缓存), 回答要 2xx 且允许这个源、方法和头 (`*` 只在不带凭据时算,
    `Authorization` 不归 `*`); 响应没有允许这个源 (带凭据时还要 `Access-Control-Allow-Credentials: true`) 就是网络错误 (`TypeError`, `cause`
    以 `CORS:` 开头); 页面只看得到安全名单里的响应头和 `Access-Control-Expose-Headers` 列的. `Request.mode`: `no-cors` 只许 GET / HEAD / POST
    和安全名单里的头, 回不透明响应 (status 0, type `opaque`, 没有头和 body); `same-origin` 不许跨源. 重定向照常跟 (每跳都查 CORS, 换到第三个源后
    `Origin: null`). 跨源请求不带也不存 cookie (所有主机对 `acde.local` 都是第三方, 和 Safari 拦第三方 cookie 一样); 同源 (后端) 的 cookie 照旧.
  - 被禁的请求头 (Host、Cookie、Origin、Referer、Accept-Encoding、`Sec-*`、`Proxy-*` …) 页面设了也不发. 同步 XHR 只给本地资源.
  - body (acde-dom `core/net/index.ts` `Body`): 读 body (`text()` 等) 用流的 reader 读完且不放锁 (规范的 fully read), 之后 `body.locked` 是 true;
    null body 读了不算 `bodyUsed`. `Response.clone()` 保留 `error` / `opaque` 的 status 0 和 type. URL 带用户名或密码的 `Request` 抛 TypeError.
  - 经典 `<script src>` 从别的主机取时不查 CORS (和浏览器一样, 不带 `crossorigin` 的脚本照样执行): `page.ts` 经 `core/net/` 的内部 `scriptSource`, 不是页面能用的接口.
