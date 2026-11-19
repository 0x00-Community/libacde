# 文件 (`acde:fs`)

页面的 `acde:fs` 是 src/host `fs.rs` 绑的 `natives.fs`, 实现在 runtime `fs/` (`Workspace` → `WorkspaceFs`), 类型 acde-api `AcdeFs`.
第一个参数是 root (`roots()` 里某项的 `id`; 下面按路径的根 `id` 就是 `path`), 第二个是相对路径: 绝对路径、`..` 出 root、经符号链接出 root 都拒绝 (`WorkspaceFs::resolve`),
root 本身不能替换或删. Finder 的根: 桌面文件夹、下载 (`XDG_DOWNLOAD_DIR` 或 `~/Downloads`, 存在时)、`~/agentos-work` (存在时), 加上用户授给
这个应用的文件夹. 根列表每个 web 缓存 (#143, runtime `fs/access/cache.rs` `RootCache`, 存在 `Workspace` 上): `roots()` 每次重建并刷新缓存; 别的调用用缓存,
授权变了 (任何应用的授予或撤销, `Grants::changes`) 或超过 2 秒就重建; 调用点名的根 (`FsCall::targets`) 在缓存里找不到时, 先重建再执行;
只读调用 (`FsCall::reads`) 失败后, 若缓存里的根不见了或重建结果不同, 用新列表重试一次; 写操作进了文件系统后失败不重试 (避免 copy / rename / delete 执行两次), 所以根文件夹刚被删时的写操作在缓存过期 (2 秒) 前直接报错.
列表内容和顺序与不缓存时一致, 只省掉每次调用都查环境变量、建桌面 / 私有文件夹、对每个根 `is_dir`. Windows 上隐藏和系统属性的文件按隐藏文件算. 运行时不记 Finder 看到哪里 (`state()` / `place()` 随 #69 移除, 由文件管理器自己记).

**应用的根** (#66; runtime `fs/access/files.rs` `Files`, Broker `broker/access/files.ts` `FileAccess`): 所有应用 (系统应用也是) 的 root 是不透明的标识, 不是路径:
`private` (本应用的私有数据文件夹 `$XDG_DATA_HOME/acde/files/<应用 id>`, 没设时 `$HOME/.local/share/...`, 第一次用时建), 公共位置名 (SDK `SHARED_PLACES`:
`desktop`、`documents`、`downloads`、`music`、`pictures`、`videos`, 按 XDG 用户目录 `XDG_<名>_DIR` / `user-dirs.dirs`; Windows 没设时用对应的已知文件夹
`SHGetKnownFolderPath`, `path.rs` `known_folder`), 选择的文件夹是 `folder-<路径 SHA-256 前 16 位十六进制>` 的令牌. `fs: true` 只给 `private` 和用户选给它的文件夹;
公共位置要在 `fs.roots` 里点名 (校验器: 不是这六个名字就是错误, `agentos-work` 不算;
末段是位置名的路径如 `~/Downloads` 提示改写位置名, 别的提示用户选的文件夹) 并经批准 (见 `docs/runtime/webs.md`「批准」). `roots()` 回
`{id, label, source: 'private' | 'place' | 'granted', mode, reason, exists, place?}`, 只有点名了公共位置的系统应用另带 `path`; 别的应用拿不到绝对路径,
`locate` / `openPath` 报 403 `EACCES`. 没批准或撤销了的位置: web 侧 (`FileAccess.guard`, 经 `Permits`)
把它从 `roots()` 里去掉, 以它为 root 的调用 reject 403 `denied` (`AcdePermissionError`). `fs.revoke(令牌)` 收回选择的文件夹, `acde:permissions` 也列出和收回它 (见 `docs/runtime/webs.md`「批准」). Broker 交给 web 的授权是
`fs: {places, paths}` (src/host `web.rs` → `Workspace::files`). 下面按路径的根只给桌面页面.

**临时借出的根** (daily-desktop 12, 用户 2026-11-18 定方案 A, 名字 `lent`): 管家可给应用的某一次拉起借出文件夹.
- 拉起时: welcome 的 `grants.fs.roots` (`[{name, dir, label?}]`, 只认对象形式的 `fs`) 在起页面线程时登记.
- 运行中: 目标应用不多开且已在运行时, 管家把请求推给它并附 `lend: {name, dir, label?}`; 启动器先经 `sys.lendRoot(应用编号, lend)` 登记,
  再把请求交给页面 (`window.request`), 请求里补 `root: <name>`.
- 登记的根放在进程内 (`fs/mod.rs` 的 `Shared.lent`, 按应用编号), 同一应用同名的后来的替换先前的, 进程结束就没了, 不记进批准.
  `roots()` 里排在私有文件夹和公共位置之后: `{id: name, label: label 或目录名, source: 'lent'}`, 可读写, 按 `name` 当 root 用.
  `name` 不能是 `private`、公共位置名或 `folder-` 开头, `dir` 要是绝对路径, 不合的拒绝 (`LentRoot::checked`).
- 收回: 桌面调 `acde:apps` 的 `unlend(dir)` (管家的 `apps.unlend`, 只有桌面能调, 应答 `{revoked: 收回几个}`), 管家对借过这个目录的每个进程推 `{type: 'unlend', name}`;
  启动器经 `sys.unlendRoot(应用编号, name)` 从 `Shared.lent` 去掉 (`Workspace::unlend`), 之后 `roots()` 不再列它, 拿它当 root 的调用按不是自己的根拒绝;
  去掉了就给页面发 `acde:fs` 的 `roots` 事件 `{removed: [name]}`, 页面据此重新取 `roots()`. 宿主上没有管家, 没有这条路.
- 桌面经 `acde:apps` 的 `open(app, request, {root: {name, dir, label?}})` 请管家借出 (SDK `OpenWindowOptions.root`, 只给桌面用), 管家决定给不给.

**拖放和打开请求的句柄** (#68): 用户交给应用的文件和文件夹是标准的 `FileSystemHandle`, 页面拿不到路径.
- 系统拖放: `dnddrop` 的 `files` 里每个路径 (acde-host `dnd.ts` `DragDrop`, 链接和别的类型跳过): 不超过 64 MiB 的普通文件进 `dataTransfer.files` (内容在 drop 时读进来),
  每个文件和文件夹在 `items` 里是 `kind: 'file'` 的一项, `getAsFileSystemHandle()` 给只读句柄 (`createWritable` / 新建 / 删除是 `NotAllowedError`,
  `queryPermission({mode: 'readwrite'})` 是 `prompt`). 和 Chromium 不同: 文件夹和超过 64 MiB 的文件不在 `files` 里, 那一项的 `getAsFile()` 是 null, 只能用句柄.
  页面自己的 `DataTransfer` 和页面发起的拖动 (没有 `files`) 的项没有句柄, `text/uri-list` 里的 `file:` 也不再变成 `File`.
  `dataTransfer.items` 同浏览器按下标取 (`items[i]`, 活的, 越界是 `undefined`, 同一项每次是同一个对象), 也能迭代; 下标经 acde-dom `events.ts` 的代理.
  `items.clear()` 同规范移除全部项 (字符串、文件和句柄项), `clearData()` 只清字符串. 没有拖放数据存储的只读 / 受保护模式, 页面拿到的列表一律可写.
- `File.type`: 拖放的 `File`、句柄的 `getFile()`、`<input type=file>` 和 `showOpenFilePicker` 的文件都按名字取类型, 和选应用用的同一套
  (`mimedb.ts` `FileTypes.byName`: Windows 是扩展名登记的 `Content Type`, 别处是 shared-mime-info 的 globs; 见 `docs/web/default-apps.md`「默认应用」的「文件的类型」),
  acde-host `PageCalls` 把它设成 acde-dom 的 `env.fileType`. 系统不知道的是空字符串, 同浏览器. 不按内容猜 (浏览器也只看名字).
- 打开请求: `Opener` 给接收方的 `request` 带内部的 `entry: {path, kind, writable}` (要选中文件时是所在文件夹加 `file` 名字), 接收方的 web (`remote.ts`
  `WindowSelf`) 把它换成 `handle` 再交给页面, `entry` 不进页面. 来源可写 (`mode` 有属主写位; Windows 只读属性) 时句柄可写. `FileAccess.view` 是 `handle` 的
  应用 (第三方应用, 和只有 `fs: true` 的系统应用) 只拿 `{kind, handle, file?}`; 点名了公共位置的系统应用和没有 `fs` 的系统应用拿 `url` 加 `handle`;
  按路径的 `{root, path, file}` 加 `handle` 只给桌面页面.
- 选择器: `showOpenFilePicker` 的文件只读 (只读属性) 时句柄是只读的, 别的可写; `showSaveFilePicker` 可写; `showDirectoryPicker` 照 `mode`.
- 句柄怎么登记: 文件记在这个页面的 `PageCalls` (和打开选择器选中的文件一起; 整读整写走 acde-host `sys/fs`, 同时经 `natives.picker.allowFile` 记进
  这个 web 的 `PickedFolders`, 只认这一个路径, 大文件的按需读和分块写走它), 文件夹经 `natives.picker.adopt` (src/host
  `picker.rs` `adopt`) 进这个 web 的 `PickedFolders`, 读写和选的文件夹一样不出这个文件夹、不跟随链接. acde-dom 经 `WebInternals.handles` (`of` / `picked` /
  `transfer`) 造句柄、认句柄、往 `DataTransfer` 里放; 页面自己 `new FileSystemFileHandle(...)` 造不出能读的句柄 (`PageCalls` 只认登记过的路径).
- 交出去: 应用的 `acde.apps.open({kind: 'open-file', handle})` (或 `open(appId, …)`) 在 web 里把 `handle` 换成 `held: {path, kind, writable}`, 只认这个页面登记过的句柄
  (否则 `TypeError`), 页面自己给的 `held` / `entry` 丢掉. Broker (`Opener.request`) 按 `held` 的路径打开, 交给系统默认程序时调用方的根加上这个文件所在的文件夹; 交出的是只读句柄时接收方也只拿到只读句柄.
  这样第 1 级应用能把拖进来的、选中的、别人交给它的文件交给别的应用打开; 私有文件夹和公共位置里的文件还没有句柄 (要先经选择器).

工作区内的相对路径用 `/` 分隔. Windows 上拒绝反斜杠、数据流分隔符、保留设备名和以空格或点结尾的名称;
判断解析后的路径是否在 root 内时按系统路径组件比较.

Windows 的 `XDG_DESKTOP_DIR` 和 `XDG_DOWNLOAD_DIR` 支持盘符绝对路径及普通 UNC 文件共享路径, 可用 `/` 或 `\` 分隔.
`$HOME` 可用两种分隔符展开. 设备路径、逐字路径、IPC 共享、相对路径和指向用户主目录的覆盖项使用默认目录 `%USERPROFILE%\<名>`.
没设 (或为空) 时按路径的根和第 1 级公共位置一样用系统已知文件夹 (`known_folder`), 桌面挪进 OneDrive 时不再新建 `%USERPROFILE%\Desktop`.

**失败**: reject 一个 `Error` (`AcdeFsError`), 字段来自 runtime `failure.rs` 的 `Failure`: `message` 人读的英文, `status`, node 风格的 `code`:

| `code` | `status` | 什么时候 |
| --- | --- | --- |
| `EINVAL` | 400 | 参数不对、出 root、类型不对、符号链接 |
| `EACCES` | 403 | 清单没给这个 root、系统不让 |
| `ENOENT` | 404 | 不存在 |
| `EEXIST` | 409 | 已经有了 |
| `ECONFLICT` | 409 | 打开之后被改了 / 没了 (`expectedModifiedMs` / `ifModifiedMs` / `ifVersion` 对不上) |
| `ENOTEMPTY` | 409 | 删非空的文件夹 |
| `EFBIG` | 413 | 超过上限 |
| `EILSEQ` | 400 | `readText` 读到的不是 UTF-8 |
| `ENOSPC` | 507 | 磁盘满 |
| `EIO` | 500 | 别的 (细节进日志) |

`code` 由造 `Failure` 的地方定 (`exists` / `conflict` / `with`), 否则按 status; io 错误按 `io::ErrorKind` 映射 (unix 上另认 ELOOP).
系统拒绝 (Windows 的拒绝 ACE、只读属性) 在写入、新建、mkdir、copy、rename、删除和解析路径 (`WorkspaceFs::resolve` 取 realpath) 时都是 403 `EACCES`;
拒绝 `W` 也拒绝了 `SYNCHRONIZE`, 连打开文件夹都失败, 所以那个文件夹的列出也是 403. 父目录不存在仍是 400 `EINVAL`. 只有
`acde:fs` 是这个形状; napi `Finder.locate` 的 message 照旧是失败的 JSON.

调用: `roots` / `list` / `preview` (文字最多 256 KiB) / `download` / `mkdir` / `rename` / `delete` / `locate` / `openPath` / `revoke`, 和文件级的
(#69 移除了 `write` (用 `writeFile`)、`desktop()` (用 `list`)、`state()`、`place()` 和 `copyIn` (用 `copy` 和句柄)):

- `list(root, path, hidden)` → `{root, path, entries, truncated, skipped?}`: `hidden` 默认 false, 不列隐藏项 (点开头的名字, Windows 另加隐藏或系统属性; #149, 之前默认列出), 给 true 才列; 先排好 (文件夹在前, 区域排序规则) 再截到 5000 条 (`truncated`);
  名字不是合法 UTF-8 (Windows UTF-16) 的项跳过, `skipped` 是个数 (没有时不出现); 目录句柄的列出同样跳过这些名字.
- `download(root, path)` → `{name, contentType, data: Uint8Array}`: runtime 只回名字和内容 (`FsReply::File`), `contentType` 由 web 侧 `fsmodule.ts` `FsModule.typed`
  (`worker.ts` 把 `natives.fs` 包上它再交出去) 按名字从 `FileTypes.system().byName` 取, 和 `File.type` 同一个库 (见 `docs/web/default-apps.md`「默认应用」的「文件的类型」:
  `.md` 是 `text/markdown`); 系统不认识的名字是 `application/octet-stream`.
- `stat(root, path)` → `FinderEntry & {path}` (lstat; 普通文件多带 `version`).
- `readText` → `{text, modifiedMs, size, version}` (最多 64 MiB, BOM 原样留), `readBytes` → `{data: Uint8Array, modifiedMs, size, version}`.
  `version` 是不透明字符串 (`fs/save.rs`: 大小、完整修改时间、文件编号 (unix dev + inode, Windows 卷序号 + 文件索引)), 只用来比相等,
  不保证跨机器、跨平台可比; 同一毫秒里的两次写、FAT 的 2 秒精度、改系统时间都分得开.
- `writeFile(root, path, data, {create: 'always' | 'new' | 'never', ifModifiedMs, ifVersion, mkdirs})` → `FinderEntry` (带写后的 `version`):
  替换已有文件是先在同一文件夹写临时文件 (`.<名字>.<uuid>.acde-save`), fsync, 再原子替换 (unix `rename` 后 fsync 文件夹, 保留权限、尽量保留属主;
  Windows `ReplaceFileW`, 保留属性和 ACL), 中途出错原文件不动、临时文件删掉; 文件夹不让新建文件 (建临时文件 `EACCES`) 时退回在原文件上截断写入,
  结果带 `inPlace: true`, 这时没有崩溃安全. 新文件直接写 (0644). `ifModifiedMs` (毫秒) 和 `ifVersion` 任一对不上都是 409 `ECONFLICT`, 不能和
  `create: 'new'` 一起用 (`EINVAL`). 目录句柄的 `createWritable().close()` 同样原子替换. 选项对象经 JSON 交给 runtime, 类型不对 `EINVAL`.
- `mkdir(root, path, {recursive})`: recursive 从 root 起逐级建, 已是文件夹算成功.
- `copy(fromRoot, fromPath, toRoot, toDir)` → `FinderEntry & {path}` (#67, runtime `fs/copy.rs`): 在本应用可用的两个 root 之间复制, 两边各是 root 加相对路径,
  不传绝对路径. 原生复制文件和文件夹 (流式, 不限大小), 名字被占用时照 `numbered` 编号 (`a 2.txt`、`project 2`), `path` 是在目标 root 里的相对路径.
  两个 root 都要在本应用的 root 里 (按路径的根两边都过清单检查), 否则 403 `EACCES`; 没批准的公共位置 403 `denied`; 源是 root 本身、`..` 或经链接 /
  联接出 root (源或目标)、源本身是链接、文件夹里有链接 (先整棵查一遍, 复制中途变成链接也停, 删掉已复制的部分)、把文件夹复制进它自己, 都是 `EINVAL`.

**大文件** (#75, runtime `fs/ranged.rs`): `readText` / `readBytes` / `writeFile` 和整读整写仍限 64 MiB. 更大的文件主路径是标准接口:
句柄的 `getFile()` 超过 64 MiB 时给按需读的 `File` (acde-dom `core/blob.ts` `RangedSource`), `slice()` 不读, `stream()` 每次读 4 MiB,
`text()` / `arrayBuffer()` 才整读; 打开时记下 `version`, 之后文件变了读是 `NotReadableError` (同 Chromium). 按需读的 `Blob` 在同步用到字节的地方
(`structuredClone` / `postMessage`、`fetch` / XHR 的请求体、表单的 multipart、`blob:` URL、IndexedDB、WebSocket 发送) 是 `NotReadableError`, `FileReader` 照常.
`createWritable()` 开一个写入 (`keepExistingData` 先把原内容复制进去), `write` 按位置写进同一文件夹的临时文件 (`.<名字>.<uuid>.acde-part`,
每块最多 16 MiB, 大的数据和按需读的 `Blob` 分块写), `seek` 只移位置, `truncate` 截断或补零, `close()` 提交: fsync 后和 `writeFile` 一样原子替换
(unix 保留权限和属主, Windows `ReplaceFileW`; 原来没有这个文件时不覆盖地改名), `abort()` 删掉临时文件. 写的位置超过当前长度时中间补零, 同
`FileSystemWritableFileStream.write({position})`. 文件夹不让建临时文件时是 `EACCES`, 不退回原地写. 同一个流的操作按调用顺序排队, 一次失败后流的操作都 reject.

`acde:fs` 的底层调用 (根和路径的规则同上): `readRange(root, path, offset, length, {ifVersion}?)` → `{data, offset, size, modifiedMs, version}`
(`size` 是整个文件的; 一次最多 16 MiB, 超过 413 `EFBIG`; 超出文件末尾的部分不返回; `ifVersion` 对不上 409 `ECONFLICT`);
`openWrite(root, path, {keepExistingData, create, ifVersion, mkdirs}?)` → `{id, size}`; `writeChunk(id, offset, data)`; `truncateWrite(id, size)`;
`commitWrite(id)` → `FinderEntry` (带 `version`); `abortWrite(id)`. `create` 同 `writeFile`, 在提交时再查一次 (`new` 时文件已有是 `EEXIST`, `never` 时没了是 `ENOENT`);
给了 `ifVersion` 时打开和提交时都比, 中途别人改了是 `ECONFLICT`, 临时文件删掉、原文件不动. 偏移、长度和大小是非负整数, 否则 `EINVAL`.
限制 (`Writes`, 每个 web 一份): 同时最多 16 个写入 (再开是 `EINVAL`); 加长文件前查剩余空间, 写完剩不到 256 MiB 时 507 `ENOSPC`;
10 分钟没有操作的写入自动放弃 (`acde-fs-writes` 线程每 30 秒查一次; 之后用这个 `id` 是 `EINVAL`); web 结束 (关页面、崩溃) 时放弃它全部的写入;
进程被杀留下的 `.acde-part` 在下次在同一文件夹开写入时删掉超过 20 分钟没改的. 写入的调用和 `writeFile` 一样进这个 web 的改动队列, `readRange` 走读的线程池.

打开文件一律不跟随符号链接 (unix `O_NOFOLLOW | O_NONBLOCK`, 不会卡在 FIFO 上; Windows `FILE_FLAG_OPEN_REPARSE_POINT` 再查是不是链接).
读写、建、删、改名都先打开父文件夹 (`fs/beneath.rs` `Dir`) 再对其中的名字操作, 检查和使用之间换掉中间文件夹也出不了 root: Linux `openat2`
(`RESOLVE_BENEATH`, root 内的链接照常可走, 出 root 是 `EXDEV` → `EINVAL`), 之后经 `/proc/self/fd/<fd>/<名字>` 和 `renameat2(RENAME_NOREPLACE)`;
Windows 打开父文件夹 (不给 `FILE_SHARE_DELETE`, 开着时改不了名、删不掉), 用 `GetFinalPathNameByHandleW` 查最终路径在 root 里 (不分大小写), 改名用不带
`MOVEFILE_REPLACE_EXISTING` 的 `MoveFileExW`; 其他 unix (macOS) 还是先 `canonicalize` 再按路径用 (没验证). 改名不会覆盖同时出现的同名项 (`EEXIST`).

**拖放不再授权** (#69): 系统拖放只交给页面 `File` 和只读句柄, 不记一次性授权, 也不往 `text/uri-list` / `text/plain` 里放绝对路径;
Linux 会话 (acdebin-system 的 `crates/window` `drop_performed`) 有本地文件时同样去掉这两项, 路径只经 `Event.files` 交给页面宿主.

**选文件夹和授权**: 标准的 `showDirectoryPicker(options?)` → `FileSystemDirectoryHandle` (acde-dom `dom/picker.ts`, 参数校验和错误照 Chromium,
取消或没有选择器是 `AbortError`, `requestPermission` 不弹窗; handle 只活到页面结束). 链路: acde-dom 的
`env.files` → src/host `picker.rs` 绑的 `natives.picker` (不是 `acde:` 模块). 选择器在 `acde-picker` 线程上开, 先取 `choose` 事先给的答复
(acdev runtime 的 `src/chooser.rs`, 三种选择器都在这里定去哪): Linux 是 portal
FileChooser (`directory: true`), macOS 是 hostwin 主线程上的 `NSOpenPanel` sheet, Windows 是 `SHBrowseForFolderW`. handle 的读写在 runtime
`fs/picked.rs` `PickedFolders` (每个 web 一份, 只在内存, 选中的文件夹当 root; 新建 `O_EXCL | O_NOFOLLOW`; 整读整写一个文件最多 64 MiB, 更大的见「大文件」).
Windows 选择器返回盘符或 UNC 绝对路径, 内存句柄和持久授权使用同一路径规则. `bash scripts/check-directory-picker.sh` 用预设选择结果验收.
用户选中的文件夹同时授给调用的应用 (`acde:fs` 多一个 root `{source: 'granted', label}`, 文件夹没了是 `mode: 'unavailable'`):
默认的只读选择给只读网页句柄和只读的 root (`mode: 'read_only'`, 能列出和读, 写入、新建、mkdir、rename、删除、复制进去都是 403 `EACCES`,
检查在 `WorkspaceFs` 的各写入函数和 `copy` 开头的 `FinderRoot::writable`); `mode: 'readwrite'` 的选择提示明确读写授权, root 可写.
同一个文件夹再被读写地选中就升成可写, 只读选择不会把可写的降级. 记在 `$XDG_STATE_HOME/acde/grants/<应用 id>.json`
(`{roots, readOnly?}`, `readOnly` 是 `roots` 里只读的那些; runtime `fs/access/grants.rs`; `ACDE_STORAGE` 为空时只在内存); 桌面选的不授.
`fs.revoke(rootPath)` 收回, `acde:permissions` 同样列出和收回只读的. Broker 起 web 时 grants 带应用 id, `web.rs` 给 `Workspace::scoped(roots, app)` 和 `natives.picker`.

**打开和保存文件**: `showOpenFilePicker` / `<input type=file>` / `showSaveFilePicker` (acde-dom `dom/picker.ts`、`dom/forms.ts`) → `env.files.open` /
`save` → acde-host `pagecalls.ts` 的页面调用 `open` / `save` → Broker `#fileChooser()` 的 `native.FileChooser` 的 `openFile({title, multiple,
directory, filters})` / `saveFile({title, name, filters})`, filter 是 `{name, globs, mimes}` (页面的 `types` / `accept` 换来的), 回路径列表 / 路径
或 null (取消). 选中的文件只给这个页面读写 (`pagecalls.ts` 记着). 没有 `FileChooser` 时打一行 warn, 页面得到 `AbortError` / `cancel`.
- 会话: runtime `portal.rs` 的 napi `FileChooser` (portal 的 `OpenFile` / `SaveFile`).
- 宿主窗口: acdev runtime 的 `src/hostnative.rs` 换上自己的 `FileChooser`, 在 `acde-chooser` 线程上走 `chooser.rs`: 先取 `choose` 的答复; Linux 有窗口时
  是 portal (runtime `open_files` / `save_file`, 和 napi 同一段); 别的交给 hostwin 主线程: macOS 是挂在宿主窗口上的 `NSOpenPanel` /
  `NSSavePanel` sheet; Windows 是 `platform/win32/dialog.rs` 的 `IFileOpenDialog` / `IFileSaveDialog`, 以宿主窗口为 owner (`multiple` 是
  `FOS_ALLOWMULTISELECT`, `directory` 是 `FOS_PICKFOLDERS`, 总是 `FOS_FORCEFILESYSTEM`, 返回盘符或 UNC 绝对路径, 取消回 `None`); headless 回错.
  COM 的 vtable 在 `dialog.rs` 手写 (windows-sys 0.61 没有这些接口), 次序对照 `windows` crate 的定义, 测试覆盖选项、文件名、IShellItem 和数组.
- macOS 的类型过滤 (`chooser.rs` `content_types`): `*.ext` 按扩展名、MIME 按 MIME、`image/*` / `text/*` / `audio/*` / `video/*` 按大类换成
  UTType; 有一项换不了 (`*`、别的通配、`application/*`、不认识的 MIME) 就不限类型 (宁可多给不能少给). UTType 按类型的继承匹配: `text/plain`
  也放行 Markdown, 和 Linux 上 shared-mime-info 的子类型一样. 保存面板不按 filters 限扩展名 (GTK 的保存对话框也只拿它筛显示的文件),
  建议的名字原样填进去.
- Windows 的类型过滤 (`dialog.rs` `specs`), 同 Chromium 的 `accept`: 过滤器有 `*.ext` 扩展名时只按扩展名映射成 `SetFileTypes`, 同一过滤器里的 MIME 忽略
  (`types: [{accept: {'image/png': ['.png']}}]` 只显示 .png). 只有没有扩展名的过滤器 (只有 MIME) 退回 All files, 因为 Windows 没有完整的 MIME 到扩展名表;
  一项扩展名换不了 (`*`、`report-*`、含 `;`) 也整体退回 All files, 不藏起页面接受的文件 (宁可多给不能少给, 和 macOS 同一规则).
- 验收: 无头 `bash scripts/file-chooser/check-file-chooser.sh` 用 `choose` 的预设答复 (`answerOpen` / `answerSave`) 检查打开多选、取消、无答复时失败、
  保存写入句柄、保存取消. 真对话框: `pwsh -NoProfile -ExecutionPolicy Bypass -File D:\Projects\AgentComputer\libacde\scripts\file-chooser\real-dialog-acceptance.ps1`
  (有界面的桌面, 会打开真的打开/保存对话框). 它用 dev 管道 (`ACDE_DEV`) 点探针页面的按钮、读结果行, 用 UI Automation 找到宿主窗口拥有的对话框
  (文件名框的值不对外暴露, 所以路径经剪贴板 Ctrl+V 贴进去, 确认 Enter, 取消 Esc; 每次按键前核对前台窗口是对话框, 否则不输入), 检查列表项和保存结果, 截图和
  `summary.json` 写到 `logs/file-chooser-real.<时间戳>`. 人工探针 (`scripts/file-chooser/manual-file-chooser.ts`) 仍可用, 不带参数时新建
  `logs/manual-file-chooser.<时间戳>`; 它的 `--prepare-only` 只生成环境.
