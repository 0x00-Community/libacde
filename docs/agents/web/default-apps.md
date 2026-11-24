# 默认应用

一个入口 `Opener.open(路径或 URI, {app?, select?})` (acde-host `open.ts`, runtime `bus.rs`), 四条路进来: `acde.openPath` (桌面和有 `openFiles`
的应用)、会话总线的 `org.freedesktop.FileManager1` (`ShowFolders` / `ShowItems` / `ShowItemProperties`)、`org.dopame.Acde1.Open`
(`<运行时> open [--app <id>] <路径|URI>...`, 没有会话时退出码 3)、原生程序的 xdg-open (经装的 desktop entry 回到 `<运行时> open`).

- **选应用**: 类型 (文件夹 `inode/directory`; 链接 `x-scheme-handler/<scheme>`; 文件见下) → 按 XDG
  MIME Applications 规范读 mimeapps.list (每个目录先读 `acde-mimeapps.list`), `acde-<app>.desktop` 指 acde 应用; 没设过的用清单 `mimeTypes`
  写了它的第一个应用 (全名先于 `type/*`), 再没有用原生程序.
- **文件的类型** (`MimeTypes.of`, 按名字的部分在 `mimedb.ts` `FileTypes`): 用各平台自己登记的类型, 运行时不带扩展名表, 也不用 mime-db
  这类面向浏览器的库 (决定). 系统不知道的类型就是未知 (`''`), 不猜, 由用户选应用.
  - Windows (有 `sys.contentType`): 按最后一个扩展名 (小写, `a.tar.gz` 是 `.gz`) 取系统登记的 `Content Type` (`HKEY_CLASSES_ROOT\.<ext>`,
    hostwin `system/assoc.rs` 用 `AssocQueryStringW(ASSOCF_INIT_IGNOREUNKNOWN, ASSOCSTR_CONTENTTYPE)`, 原生调用, 不起子进程), 原样用它:
    `.csv` 可能是 `application/vnd.ms-excel`, `.zip` 是 `application/x-zip-compressed`; `.md` `.py` `.rs` `.yaml` 一般没有, 就是未知.
    不问 gio、`file`, 也不读 shared-mime-info.
  - Linux / macOS: 先问 `gio info` (Linux). 它给不出 MIME 时 (macOS 的 gio 回 UTI, 或没有 gio) 按 shared-mime-info 的
    `mime/globs2` 匹配文件名 (`mimedb.ts`; XDG 数据目录, macOS 另加 Homebrew / MacPorts 的 `share/mime`; 每个进程读一次; `__NOGLOBS__` 照规范):
    完整文件名先于通配, 再比权重、模式长度、区分大小写的优先. 打平时用 `file --mime-type`: 它是候选之一就用它, 否则挑和它一样是不是文本
    (`text/*` 或经 `subclasses` 到 `text/plain`) 的那个, 和 gio 用 magic 消歧义一样. 一个也没匹配上用 `file`, 再不行是
    `application/octet-stream`. 这样 macOS 的宿主窗口和 Dopame OS 得到同样的类型 (`.md` 是 `text/markdown`, 不是 `file` 说的 `text/plain`).
    数据库不进包 (shared-mime-info 是 GPL), 没有时打一行 warn (macOS 提示 `brew install shared-mime-info`), 类型只来自 `file`.
  - 测试 `acde-host/test/mimedb.test.ts` (注入的注册表读取, 不碰真的注册表).
- **这个平台上跑不了的应用不选** (Broker `#runsHere`): 宿主窗口里没有 slots (嵌原生窗口只在 Linux 会话里), 要 `slots` 的应用 (如
  Dopame 的浏览器) 不当默认应用、不进 `handlers`, 页面的外部链接也不交给它 (打一行 warn, 不开只会报错的空窗口). 它们也不在
  `acde.apps.list()` 和 `appschanged` 里 (Dock、搜索、设置看不到); `launch` / `acde.open(<id>)` 和指定 `app` 的 `openPath` 回
  `{status: 404, code: 'unsupported_app'}` (页面的 `navigator.platform` 总是 Linux, 看不出宿主和有没有 slots, 所以靠这个 code). Linux 会话不变.
- **Windows 的打开方式** (宿主 `Shell.fileHandlers` / `Shell.openWith`, hostwin `system/assoc.rs`; Opener 经 `FileAssociations`): Windows 上不读 `.desktop`,
  原生程序是系统给这个扩展名登记的处理程序 (`SHAssocEnumHandlers(ASSOC_FILTER_RECOMMENDED)`, 即资源管理器「打开方式」里的那些), 每项 `{id: IAssocHandler::GetName
  (程序全路径或打包应用的 id), label: GetUIName, icon: data:image/png (GetIconLocation, 可执行文件里的图标经 SHDefExtractIconW 画成 32 px,
  打包应用的 `@{…}` 经 SHLoadIndirectString 取 logo 的 PNG), default}`; 系统默认 (名字等于 `ASSOCSTR_EXECUTABLE` 或 `ASSOCSTR_APPID`) 在 `handlers()` 里标
  `system: true`, 只读不写 (不调 `MakeDefault`). `handlers(路径)` 回 `{mime, association, handlers}`: acde 应用照旧按清单 `mimeTypes` 匹配, 类型未知时
  列出所有声明了文件类型的 acde 应用 (只收 `inode/directory` 的除外) 和这个扩展名的原生程序; `association` 是记住选择用的键, Windows 上是扩展名
  (小写 `.md`), 没有扩展名时是类型. 用户的选择由运行时记: `setDefault('.md', 应用 id 或程序 id)` 写 `$XDG_CONFIG_HOME/acde-extensions.list`
  (格式同 mimeapps.list, 键是扩展名), 原生程序只能按扩展名记且必须是登记过的; 选 acde 应用时按扩展名或 MIME 都行. 选应用的顺序: 扩展名的选择 →
  MIME 的选择 → 清单 `mimeTypes` 的第一个 → 交给系统默认 (下一条) → `no_app` (由桌面弹自己的打开方式). 用原生程序打开: 先过 `Finder.admit`
  (同下), Rust 再做和交给系统同样的路径检查 (绝对、非设备路径、无 `..`、非命名流、普通文件), 但不限扩展名 (用户为这个文件选的这个程序),
  重新枚举确认它是这个扩展名登记的处理程序, 经 `IAssocHandler::Invoke` (文件的 `IDataObject` 来自 `IShellItem::BindToHandler(BHID_DataObject)`)
  在 `acde-open` 线程上打开 (先 `CoInitializeEx`). `openPath({app: 程序 id})` 点名原生程序只给桌面 (和 `<运行时> open`) 以及点名了公共位置的系统应用 (能 `locate` 的那些, `OpenCaller.paths`, #115;
  文件仍要在它的根里), 别的应用点名回 403 `refused`;
  记住的选择对所有调用方都生效. headless 照样检查, 只打一行 info 不打开. 测试 `acde-host/test/open/windows.test.ts` (注入的关联读取).
- **macOS / Windows 的宿主窗口交给系统默认程序** (没有 XDG 的 `.desktop`, `natives()` 是空的): 没有记住的选择、没有能用的 acde 应用、也没有指定 `app`
  时, `Opener.open` 经宿主 `Shell.openDefault(目标)` (只在这两处注入; hostwin `system/handoff.rs`, 在 `acde-open` 线程上做, promise 落定)
  交给系统: Windows `ShellExecuteW` (动词 `open`, 线程上先 `CoInitializeEx`), macOS `/usr/bin/open <目标>` (不经 shell). 回
  `{kind: 'native', id: 'system'}`. 只有两种目标:
  - http / https 链接: `Opener` 先用 `URL` 规范成 `href`; Rust 再要求小于 8 KiB、只有可见 ASCII、没有 `"` `\` `<` `>` `` ` `` 和空白、有主机
    (挡掉往浏览器命令行里塞参数). 别的 scheme (`file:`、`javascript:`、`ms-settings:`、`mailto:` …) 不交.
  - 文件: 不是文件夹、不是 `select`, 先过 napi `Finder.admit(path, roots, owner)` (runtime `fs/access/admit.rs`): 把路径和调用方能用的根都按真实路径
    解析 (`canonicalize`, 跟随联接和链接) 后, 文件必须落在其中一个根里, 交给系统的是解析后的路径 (去掉 `\\?\`); 不在的回 `outside_roots`.
    调用方能用的根: 桌面、`<运行时> open` 和系统总线 (`openFromBus`) 是全部 Finder 根; 应用是清单 `fs` 的 roots (`fs: true` 是全部 Finder 根,
    没有 `fs` 是空的) 加上授予它的文件夹, 和它的 `acde:fs` 一样 (每次重读授权文件); 第 1 级的应用是私有文件夹、批准了的点名位置 (`OpenCaller.places`,
    Broker 按当前的批准滤过) 和选的文件夹 (napi `Finder.admit` 第 4 个参数, runtime `Workspace::admit_with`), 交来的文件所在的文件夹也算 (#110).
    所以根里的联接指向根外时也被拒绝.
    Rust 再要求绝对路径、不是设备 / 逐字路径
    (`\\?\`、`\\.\`)、不带 `..`、Windows 上不是命名流 (组件里有 `:`)、`symlink_metadata` 是普通文件 (链接不交), 扩展名在
    `handoff.rs` `EXTENSIONS` 里 (PDF、图片、音视频、纯文本 / Markdown / CSV / JSON、新版 Office (`.docx` `.xlsx` `.pptx`) 和 OpenDocument).
    可执行文件、脚本、快捷方式 (`.exe` `.bat` `.ps1` `.js` `.lnk` `.url` `.hta` `.docm` …) 一律不交. 也不交 (决定): 旧版 Office
    (`.doc` `.xls` `.ppt`, 能带宏)、`.html` `.htm` `.svg` (在浏览器里跑本地脚本)、`.rtf` (能嵌 OLE 对象). 选的是收窄类型, 不是给下载补
    Mark of the Web: acdev 的下载和应用自己写的文件都没有 `Zone.Identifier`, 交给 Office 时不进受保护视图, 收窄对所有来源都成立.
    Windows 不关区域检查 (文件本来带的 Mark of the Web 照旧提示).
  - Rust 拒绝 (`code: 'refused'`) 或系统打不开 (`failed`) 都回 `no_app`, 文字带原因. 系统找不到默认程序时 Windows 弹它自己的「打开方式」.
  - 安全边界: 只有 `Opener.open` 这条路 (桌面、有 `openFiles` 的应用、`<运行时> open`), 靠能力、调用方的根和上面的过滤限范围.
    **不要求用户手势**: acde-dom 还没有用户激活 (user activation) 模型, 有 `openFiles` 的应用不经点击也能交出去; acde-dom 有了激活模型后,
    应用发起的交出要求一次有效的激活 (桌面和 `<运行时> open` 不要求). 页面自己的导航 (`<a>`、`location`、`window.open` 到 http(s)) 是页面可控的,
    不交给系统. headless (`ACDE_HEADLESS`) 照样检查, 只打一行 info 不打开. Linux (会话和宿主窗口) 不注入, 行为不变.
- **交给应用**: 应用收 `request` (和 `acde.open` 同一个事件), 窗口提前 (桌面收 `windowshow`); 请求带 `handle` (见 `docs/agents/web/fs.md`「拖放和打开请求的句柄」),
  按路径的根的拿 `{root, path, file}` (`fs.locate`), 能看路径的拿 `{url}`, 第 1 级的第三方应用只有 `handle`. 不在任何根里的文件交给原生程序 (`gio launch`), 没有就报 `outside_roots` (宿主窗口也不交给系统).
- **改默认**: `acde.setDefault(mime, 应用)` 写 `$XDG_CONFIG_HOME/acde-mimeapps.list` (只在 `XDG_CURRENT_DESKTOP=acdev` 下生效; Windows 见上,
  扩展名的选择写 `acde-extensions.list`). `acde.handlers(路径)` 列能打开它的应用, `association` 是给 `setDefault` 的键.
- `<运行时> applications <目录> [--package <包目录>]` 给有 `mimeTypes` 的应用写 `acde-<app>.desktop` (`Exec=<运行时> open --app <app> %U`,
  `NoDisplay`, `OnlyShowIn=acdev;`) 和 `acde-mimeapps.list`.
- 总线名字只在自己的会话里拿 (`ACDE_BUS_NAMES=1/0` 强制), 允许被替换; 不装 D-Bus activation 文件.
