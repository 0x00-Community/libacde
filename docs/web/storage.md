# 网页存储

- **网页存储** (src/storage `acde-storage`, src/host `storage.rs`, acde-dom `src/storage/`): 所有 web 都是同一个进程里的线程, 存储按
  **分区**放在进程里共享: 每个应用一个 (`app-<id>`), 每个桌面一个 (`desktop-<id>`), 登录页 `greeter` (只在内存). Broker 在 `#spawn` 时把
  `{key, dir, legacy}` 作为 grant 交给 web (acde-host `storage.ts` `StoragePlaces`), Rust 用 `Lease` 引用分区, 最后一个 web 退出时写盘并放掉;
  进程退出前 `acde_storage::flush_all()`. 目录 `$ACDE_STORAGE/<key>/` (默认 `$XDG_DATA_HOME/acde/storage`), 文件是 SQLite (rusqlite 自带的,
  WAL, 坏了挪成 `.bad-<毫秒>` 再从空的开始, 打不开就只在内存里, 都打 warn).
  - `localStorage`: 同一应用的所有窗口共用一份 (`LocalArea`, 内存里的表是准的, 改动由 `acde-storage` 线程按顺序批量写进
    `localstorage.sqlite3`); 键和值按 UTF-16 存 (单独的代理项原样保留); 配额 5 MiB, 按键和值的 UTF-16 码元数算, 超了 `setItem` 抛
    `QuotaExceededError` 且不改; 别的窗口改了发 `storage` 事件 (`StorageEvent`, `url` 是改的那个页面当时的地址, `storageArea` 是
    `localStorage`), 改的窗口自己不收. 命名属性照 WebIDL: 原型上有的名字 (`key`、`length` …) 不当成条目, 赋值一律存. 老版本的
    `$XDG_CONFIG_HOME/acde/storage/app-<id>.json` / `<desktop>.json` 在第一次建库时导入, 改名成 `.json.imported` (只在没设 `ACDE_STORAGE` 时).
  - `sessionStorage`: 每个 web 自己的, 只在内存, 配额同上; `location.reload()` 重起 web, 不保留 (标准里同一个浏览上下文要保留, 是缺口).
  - **IndexedDB** (src/storage `idb/`, src/host `idb.rs`, acde-dom `src/storage/idb/`): 分工是 Rust 管「谁先谁后」和数据, JS 管
    标准的语义.
    - Rust: 每个分区一条 `acde-idb` 线程 (`mod.rs` `serve`), 上面每个数据库一个 `Worker` (`database.rs`), 一个 SQLite 文件
      `indexeddb/<n>.sqlite3` (没有连接、事务和排着的请求时关掉文件, 内存分区的不关), 名字和版本在
      `indexeddb/catalog.sqlite3` (内存分区在 `:memory:` 和表里). 线程数和打开的文件数不随页面开的数据库个数涨. Worker 里排: 打开 / 删除的队列 (版本变了先给别的连接发 `versionchange`,
      它们都回 `acknowledge` 后还有没关的就给请求发 `blocked`, 都关了才升级或删)、事务调度 (作用域重叠且有一方要写时按创建顺序等;
      SQLite 同一时刻只有一个写事务, 所以不重叠的写事务也排队, 标准允许)、执行操作 (写操作各包一个 SAVEPOINT, 失败只撤这一条)、
      提交 / 回滚. 只读事务不开 SQLite 事务, 和正在写的事务作用域不重叠, 读不到它没提交的东西. 键是 JS 编好的字节, BLOB 按 memcmp
      比较就是 IndexedDB 的键序 (`records (store, key)`、`index_records (idx, key, pk)`, 都 WITHOUT ROWID); 值是结构化克隆的字节;
      store / index 的 id 由 JS 定 (升级里新建的 store 马上就要被后面的请求引用). 连接、事务、请求的 id 是 `(web, 本地号)`, web 退出时
      `owner_gone` 关掉它的连接、回滚它的事务.
    - JS (`idb.ts`, `keys.ts`): 键 → 字节 (数字 / 日期是保序的 f64, 字符串按 UTF-16 码元、二进制按字节, 0 转义, 数组以 0 结尾; 类型
      标签 数字 < 日期 < 字符串 < 二进制 < 数组; 数组的代理对象不是键, 用 `host.isProxy` 认), 键路径、生成键注入、多值索引、
      `IDBKeyRange`、全部参数检查和异常 (顺序照规范, 文字照 Chromium). 公开的 `IDB*` 类只有 IDL 里的成员 (`test/storage.test.ts` 逐个
      对), 逻辑在模块里的 `Connection` / `Txn` / `StoreCore` / `IndexCore` / `CursorCore`, 公开对象用 symbol 键指过去.
      - 请求先排在 `Txn.queue`, 收到 `started` (带键生成器的当前值) 后按顺序发给 Rust. 一条请求的索引集合在**发出请求时**定下 (后面的
        `createIndex` / `deleteIndex` 也按顺序在 Rust 里执行). 往自增的 store 写时, 等这个 store 前面的写都有了结果才发 (`inflight`),
        失败就把生成器退回去 (失败的 `add` 不占号, 和浏览器一样); 生成器的新值随 `put` 写在 SAVEPOINT 里, 一起成功或撤销, 用到
        2^53 以后是 `Infinity` (再生成就是 ConstraintError).
      - `createIndex` 读出已有记录、算索引键、`indexEntries` 写进去, 期间后面的请求停在 JS 里 (`pause`), 保证它们看到填好的索引;
        唯一约束不满足就中止升级事务.
      - 游标不在 Rust 里留状态: 每一步带上位置 (键 + 主键) 和目标重新查. `cursor.update` 只克隆一次.
    - 事务的「活跃」: 新建和派发 success / error / upgradeneeded 时活跃. IndexedDB 的事件用 `fireFromTask` (`core/event.ts`) 派发: 和浏览器
      一样每个监听器之后跑一次微任务检查点 (`host.runMicrotasks`), 再做规范的「清理 IndexedDB 事务」(`hooks.checkpoint`: 这个检查点里
      `db.transaction()` 新建的事务失活; 升级事务不算). 派发完马上失活; 任务末尾 (`endTask` → `idbTaskEnded`) 再看没有没回来的请求
      就自动提交. 所以用 promise 包 IndexedDB 的库 (idb 等) 和浏览器行为一样. 监听器抛错 (`listenerThrows`; `handleEvent` 不能调用也算)
      在事务还活跃时中止它; 显式 `commit()` 之后的 error 事件没被取消仍然中止 (Chromium 的行为), 抛错不中止. `commit()` 等所有请求的
      事件都派发完才发给 Rust. `versionchange` 的回答 (`acknowledge`) 也放到任务末尾, 监听器里 `await` 之后再 `close()` 不算 blocked.
      升级事务 `abort()` 之后同步看还在跑, 到发 `abort` 事件的那个任务才清掉 (`complete` 之前清掉).
    - 值的克隆: `host.serialize` / `deserialize` (V8 ValueSerializer). 平台对象是宿主对象 (acde-dom `core/clone.ts` `serializable`: 原型上的
      `CLONE` 方法给出 `[种类, 字段…]`, `host.cloneHooks` 登记): Blob、File、FileList、ImageData、DOMPoint(ReadOnly)、DOMRect(ReadOnly)、
      DOMQuad、DOMMatrix(ReadOnly). V8 反序列化时不许跑 JS, 所以 `read_host_object` 先放占位对象, 读完后交给 JS `reviveAll` 换成真的
      对象 (Map / Set 也照顾到). `EventTarget` / `Event` (含 DOM 节点、window) 克隆抛 `DataCloneError`; 克隆时 getter 抛的错原样抛出
      (`cloneFailure`). `structuredClone` / `postMessage` 也走这条路, `transfer` 里的 ArrayBuffer 克隆后 detach (MessagePort 照旧不转移).
    - 测试: `cargo test --release -p acdert storage` (调度、版本变更、回滚、游标), acde-dom `test/idb-keys.test.ts` (键序)、
      `test/storage.test.ts`. WPT 的 IndexedDB 用临时的包 (`apps` 的 entry 指向生成的页面, assets 挂 `/resources/`、`/IndexedDB/`、
      `/common/`、`/interfaces/`; `/resources/WebIDLParser.js` 是 `resources/webidl2/lib/webidl2.js`), `acdert app` headless 逐个跑
      (`.any.js` 只跑 window 那份, 有 `variant` 的用 `history.replaceState` 换查询串), 页面用 `add_completion_callback` 把结果打进日志后
      `window.close()`. 2026-11-09 macOS: 1332 / 1384 个子测试过. 没过的都是别的缺口: Web Worker、跨 realm (iframe)、bfcache、
      SharedArrayBuffer (要 COOP / COEP)、Storage Buckets、要 WPT 服务器的 (`content.py`); `idlharness` 112 / 207, 没过的是整个 acde-dom
      的绑定形状 (方法和属性不可枚举、事件处理属性在原型上读不抛 TypeError、构造函数的 `length`), 不是 IndexedDB 自己的.
  - **配额**: IndexedDB 和私有文件系统现在不按 `estimate()` 的 quota 拒绝写入 (不会抛 `QuotaExceededError`), 是缺口; 页面本来就能经
    `sys` 写任意文件 (见 `docs/runtime/webs.md`「沙箱」), 所以这不是安全边界的问题, 是和浏览器行为的差别.
  - **cookie** (`document.cookie`, 同源请求的 `Cookie` / `Set-Cookie`): 解析在 acde-dom `core/backend.ts`, 存在分区的 `CookieJar`
    (src/storage `cookies.rs`): 同一应用的窗口共用; 有 `Expires` / `Max-Age` 的写进 `cookies.sqlite3`, 会话 cookie 只在进程里
    (acdert 退出就没了, 和浏览器关掉一样); 按路径长、再按创建先后排; 脚本碰不到 `HttpOnly`.
  - **Cache API** (`caches`, `Cache` / `CacheStorage`, acde-dom `storage/cache.ts`, src/storage `caches.rs`, src/host `caches.rs`):
    `caches.sqlite3`, 每个缓存一行 (按创建先后), 条目存请求的 URL (含 `#片段`)、方法、请求头, 回应的 status / statusText / type / url /
    redirected (JSON)、回应头和整段 body. 匹配在 Rust 里照规范的「request matches cached item」: 比的时候去掉片段, `ignoreSearch` 再去掉
    查询串; `Vary` 逐个头比请求头 (`*` 不匹配); 非 GET 除非 `ignoreMethod`. `put` / `add` / `addAll` / `delete` 是一批原子操作 (先删同一请求
    的旧条目再插). `caches.delete` 只把缓存标成作废 (名字清空): 已经打开的 `Cache` 对象照旧能用, 下次打开分区时清掉. 每个 web 的 Cache API
    调用经同一条 `acde-caches` 线程按调用顺序执行 (`offload::Queue`), 所以 `Promise.all` 里开的几个缓存按调用顺序排. 页面 `addAll` 里重复的
    请求 (同 URL、方法, 按后者的 `Vary`) 是 `InvalidStateError`; 206、`Vary: *` 拒绝. 内存分区在内存的 SQLite 里.
    WPT `service-workers/cache-storage` 2026-11-09 macOS: 133 / 157; 没过的要 WPT 服务器 (`fetch-status.py`、`vary.py`、跨源)、Worker、
    iframe、Storage Buckets、Service Worker.
  - **`navigator.storage`** (`StorageManager`, acde-dom `storage/manager.ts`): `estimate()` 的 usage 是分区目录下文件的总大小
    (内存分区加上 localStorage), quota 是磁盘的 60% (和 Chromium 一样; `quota.rs`, unix `statvfs`, Windows `GetDiskFreeSpaceExW`);
    `persist()` / `persisted()` 都是 true (Dopame OS 不回收应用的存储). `getDirectory()` 是私有文件系统: `<分区>/opfs/` (内存分区在临时
    目录, 分区放掉时删), 经 `natives.storage.privateDirectory()` 登记进这个 web 的 `PickedFolders` 和 `pagecalls.ts` 的文件夹表,
    返回名字是 `''` 的可读写 `FileSystemDirectoryHandle`. 内存分区的私有文件夹在 `$XDG_RUNTIME_DIR` (没有时用临时目录) 下新建的
    `acde-storage-<pid>-<随机>` (0700, 已有同名的就换名字, 不复用), 进程退出时删, 读写和 `showDirectoryPicker` 的同一套. `createSyncAccessHandle` 只在 worker 里, 没有.
