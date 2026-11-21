# 网页应用 (清单, 浏览器接口)

网页存储见 `docs/web/storage.md`, 应用后端和请求路由见 `docs/web/backend.md`.

工程、构建和 `acdev dev` 是 acdev 仓库的事 (`acdev new / check / build / install / dev`, 框架识别、模板、listen preload). 本仓库对外只发 SDK
`@acde/api`. 运行时只认构建出来的包: 清单 `package.json`、`web/`、`backend/server.js` 和 `backend/acde-listen.js` (把每个
`listen(P)` 换成 `<sockets>/P.sock`).

**校验只有一份**: acde-api `src/validate.ts` 的 `Manifest`: `Manifest.app` 是 Broker 用的, `Manifest.check` 另加工具才查的 (资源目录、entry 和图片
落不落在 assets 下、id 重复、不认识的 capability 和 icon).

- **入口**: `entry` 是 HTML (当文档加载, 地址 `http://acde.local/`, 应用目录同时挂在 `/`), 或以 `/` 结尾 (后端渲染: 装好后端传输后
  `fetch(init.url)` 取 HTML; `page.ts` `PageSource.served`). `backend: {entry, port?}` (默认 3000).
- **脚本**: classic `<script>` (含 `src`, 先读本地资源, 没有再经后端取; 非 async 的按文档顺序在 DOMContentLoaded 前执行)、`type="module"`、
  `type="importmap"` (`modules.rs` 解析说明符时用: `imports` / `scopes`, 精确或 `/` 结尾前缀匹配, 多个合并先到先得).
- **相对地址**: 都按文档的地址解析, 和浏览器一样: acde-dom 的 `env.url` 一变 (加载、`pushState` / `replaceState`、前进后退) 就经 DomNative
  `setBaseUrl` 交给 render (blitz `set_base_url`), 所以 HTML 里写的、JS 设的属性 (`link.href`、`img.src`、`insertAdjacentHTML`)、内联样式和样式表里的
  `url()` / `@import` 都对 (样式表里的相对文档时按样式表自己的地址). 页面源码原样交给解析器, `getAttribute` 读到的是写的原文 (2026-11-19 以前
  `PageSource.html` 用正则把静态的 `src` / `href` / `poster` 改成绝对路径, 子目录页面里别的写法都按根目录解析). 还没有: `<base href>`.
- **样式表和别的子资源** (`<link rel=stylesheet>`、`<img>`、`@font-face`、CSS 背景图): 应用包里的文件 render 直接读盘; 读不到的 http / https /
  `blob:` 地址 render 的 `Loader` 发到页面线程, 经页面的网络栈取 (acde-dom `core/resources.ts` `loadResource`: 和 `fetch` 同一套路由、代理、
  证书、重定向, `acdev network` 里是 `image` / `stylesheet` / `font`). 规则按 destination 照浏览器: 图片和样式表 no-cors (不带 `Origin`, body 照用),
  字体 cors; 样式表要 2xx 的 `text/css`, 图片不看状态码; 失败在 console 报 `Failed to load resource: <url>: <原因>`. `<link crossorigin>` 是 CORS
  请求 (空值和别的值不带凭据, `use-credentials` 带), 没过 CORS 检查算失败; 改 `crossorigin` 和改 `href` / `rel` 一样重取.
  `document.fonts` 列的是各表的 `@font-face` (acde-dom `fontFaceTexts`, 内部读 render 的规则, 跨源 (opaque) 的表也算, 像浏览器), 页面读这种表的 `cssRules` 仍是 `SecurityError`.
  - `<link>` 的 `load` / `error`: blitz fork 给每个 link 记它当前这一次取 (`SheetLoad`, 连同表里 `@import` 的表, 都答了才算完), render 的
    `take_loaded` 把取完的 link 和图片一起报, `sheetState` 说成败. 成了 (取到并进了 stylist) 派 `load`; 取不到、状态码或类型不对、`@import` 的表有
    一个没取到派 `error` (已取到的规则照样生效, 和 Chrome 一样). `href` 又变了或 link 离开了文档, 先前那次的答复丢掉, 不派事件; 换 `href` 时旧表留到新的
    答复来. 页面这边由 `deliverLoads` (`dom/media.ts`) 派: 每帧一次, 另外 link 接进文档、这三个属性变了、解析的 HTML 里有 `<link`、远程样式表答了
    之后各排一个 0 ms 定时器 (`dom/env.ts` `loads.expect`), 不等帧 (没人看的窗口不出帧). 没有包装对象的 link 只在有 `onload` / `onerror` 属性时才建.
    `<img>` 接进文档时同样排一次 (#204): 克隆体插入时取图 (常是命中缓存, 插入当时就结束), 任务结束只提交不出帧, 不排就等不到它的 `load`.
  - `link.sheet` / `document.styleSheets`: 取完才有 `CSSStyleSheet` (`ownerNode`、`href`、`media.mediaText`), 每次取完是新对象; 在取和失败时
    `sheet` 是 null. 响应对页面不透明的表, `cssRules` / `rules` / `insertRule` / `deleteRule` 抛 `SecurityError`. 透不透明由取这张表的那次
    请求定, 不看 link 现在的属性 (改了 `crossorigin` / `href`, 新表到之前手上还是旧表, 旧表的结论不变): `loadResource` 在响应头到的时候判,
    不是 CORS 请求而且请求地址或重定向后的最终地址有一个不同源就是不透明 (中间跳过别的源、首尾都同源的不算), 随 `loaded(id, bytes, opaque)`
    交给 render (`Load::finish_opaque` → `NetHandler::opaque`), blitz fork 记在这张表上 (`sheet_sources`, 连同取它的地址即 `sheet.href`, 和带来
    它的那次请求的 id). `CSSStyleSheet` 对象建的时候按这个 id (`sheetSerial`) 绑定到这一张表: `link.sheet` 在 id 变了时给新对象; 旧对象和从它
    拿走的 `cssRules` 每次读都核对 id, link 换了表之后读到的是空 (不会读到后来那张表的规则), 往里 `insertRule` 只改它自己. 包里的文件、`data:`、`blob:`、同源后端和过了 CORS 检查的可读写. `replaceSync` / `replace` 只给
    `new CSSStyleSheet()` 造的表, `<style>` / `<link>` 的表是 `NotAllowedError`.
  - `<link>` / `<style>` 的 `media` 属性是表的 media list: 不匹配的表照取、照派 `load`、`sheet` 照有, 只是不生效; 改属性不重取.
  - 没做: `window` 的 `load` 不等样式表和图片; `link.disabled`、备用样式表 (`title`)、`CSSImportRule.styleSheet`、`MediaList` 的成员 (只有
    `mediaText` / `length`); `<img crossorigin>` 仍按 no-cors 取.
- **本地资源的类型** (acde-dom `core/net/index.ts` `TYPES`): 应用包里的文件经 `fetch` / XHR 读到时按扩展名给 `Content-Type`, 只列页面加载要的类型
  (HTML、脚本、样式、JSON、wasm、字体、图片、音视频), 固定、和系统无关; 别的 (文稿、表格、压缩包、`.md` …) 是 `application/octet-stream` (#106).
  用户文件的类型不走这张表, 走宿主系统 (`env.fileType`, #98).
- **浏览器接口 → 窗口** (acde-host `webapp.ts` / `pagecalls.ts` / `downloads.ts`): `window.close()` / `location.reload()` 关闭 / 重启这个 web;
  `localStorage` / `sessionStorage` 见 `docs/web/storage.md`「网页存储」; `alert` 画在窗口里 (`confirm` / `prompt` 返回 false / null; acdev dev 里能事先答复, 见
  `docs/dev/dev-ipc.md`「acdev dev」的 `dialog`. 页面可以等用户回答的模态对话框还没有);
  外部链接交给浏览器应用 (宿主窗口里要 slots 的浏览器应用跑不了, 不打开, 见 `docs/web/default-apps.md`「默认应用」), 下载存进下载目录 (桌面收 `on('download')`); 打开 / 保存文件的选择器见 `docs/web/fs.md`「打开和保存文件」; 隐藏 / 显示是 `visibilitychange`;
  `screen.*` 是主输出的逻辑尺寸; `navigator.userAgent` / `platform` 在所有宿主上都报 Linux (`X11; Linux x86_64` / `Linux x86_64`, Broker 的 `UserAgent`,
  经 `WebInit.navigator`), 产品段是 `acde/<真实版本>`. 运行时对应用呈现统一的 Linux 环境: 交互语义一律照 Linux (编辑快捷键是 Ctrl;
  页面若看到 `MacIntel`, 编辑器库会把快捷键改绑到 Command, 同一窗口里就有两套规矩), 跨宿主一致.
  需要宿主系统信息的应用以后由 `acde:` 模块单独提供, 不从 `navigator` 猜.
- **`<select>`** (https://html.spec.whatwg.org/#the-select-element): 选中状态在 render (`dom/select.rs`), 每个 option 自己记 selectedness
  和 dirtiness (`Form.selected` / `dirty`, 没设过时跟 `selected` 属性), 照规范: 没有 `multiple` 时选中一个就清掉别的、插入选中的 option
  也清掉别的、两个以上留最后一个, 显示大小为 1 时一个都没选就选第一个没禁用的 (插入、删除、`selected` / `multiple` / `size` 属性变了、
  `option.selected = …` 之后); `selectedIndex = …` 不再补选; 表单 reset 回到属性. list of options 只算直接的 option 和 optgroup 里的
  (`dom.options`). option 的 `:checked` 跟 selectedness. 显示: 下拉框 (显示大小 1) 画第一个选中项的文字和箭头, 宽是最宽的选项;
  列表框 (`multiple` 或 `size` > 1) 每个 option 和 optgroup 标签一行 (组里的缩进), 高 `size` 行 (`multiple` 默认 4), 能滚,
  选中行底色 (有焦点蓝底白字, 没有灰底); 列表框的 option 有盒子 (`getBoundingClientRect` 是它那一行, `elementFromPoint` / 鼠标事件
  落在 option 上, 和 Chrome 一样), 下拉框的 option 是 0×0. 交互 (acde-dom `dom/select.ts`): 下拉框按下 (mousedown 没被取消) 打开
  选项列表, 拖到一项松开或再点一项选中、发 `input` 和 `change`, 点外面或 Esc 关掉不改; 列表打开时指针和键盘只给它 (Tab 选中当前项再
  照常移焦点), 上下 / Home / End / PageUp / PageDown 移高亮, Enter / 空格选中, 打字跳到开头相同的项. 关着有焦点时上下左右 / Home /
  End / PageUp / PageDown / 打字直接改选中项, 空格、Alt+上下、F4 打开. 列表框按下选中那行, `multiple` 时 Ctrl 切换、Shift 从锚点选到
  这行, 键盘同理 (Shift 扩展, Ctrl+上下只移光标, Ctrl+空格切换). 焦点移走、窗口失活、隐藏、改大小时关掉列表. `showPicker()`: 禁用时
  `InvalidStateError`, 下拉框打开列表 (不查 transient activation: acde-dom 还没有用户激活的模型). 选项列表由 render 画在帧的最上面 (不在
  文档里, `Engine.picker`, 每帧按 select 的位置重排: 下面放得下放下面, 否则放空间大的一边并截短、能滚), 所以出不了窗口; 滚轮在列表上
  滚列表, 在外面先关掉列表.
- **`<input type=range>`** (https://html.spec.whatwg.org/#range-state-(type=range)): 值的净化在 render (`dom/values.rs` `RangeValue`, `value` 和
  `set_value` 都过它): min 默认 0、max 默认 100 (比 min 小时取 min)、step 默认 1 (`any` 不对齐)、无效值取中点, 夹到范围里再按 step 从 min 起
  四舍五入对齐 (按 min / step 的小数位数用整数算, 和浏览器的十进制结果一样, 不出 1.8000000000000003), 超出 max 时取不超过 max 的最大一档;
  `min` / `max` / `step` 改了时重新净化已有的值. 画 (blitz-paint `draw_range`, `SpecialElementData::RangeInput(比例)`): 灰色轨道、左边一段和
  圆形滑块用文字颜色 (和复选框、单选框同一个 `accent_color`), 默认 `appearance: auto` (render 的 UA 样式表),
  `appearance: none` 时不画; 默认 129 × 16. 交互 (acde-dom `dom/rangeinput.ts`): 按下 (没被取消) 按指针位置设值并跟着拖, 每次变发 `input`, 松开时和按下时不同发 `change`;
  有焦点时 左 / 下 减一档、右 / 上 加一档、PageUp / PageDown 十分之一、Home / End 到两头, 各发 `input` 和 `change`.
- **XML 文档和命名空间** (render `xml.rs` / `serialize.rs` 的 `XmlSerializer`, acde-dom `dom/namespaces.ts`): `DOMParser` 的 `application/xml`、
  `text/xml`、`application/xhtml+xml`、`image/svg+xml` 用自己的 XML 解析器 (XML 1.0 + Namespaces 1.0): 良构检查 (标签配对、只有一个根、属性
  不重复 (按原名和按命名空间 + 本地名)、名字字符、`--` / `]]>`、未声明的前缀、`xml` / `xmlns` 前缀的约束), 实体 (五个预定义的、字符引用、
  内部子集的一般实体 (替换文本按内容再解析, 不许递归, 展开上限 16 MB / 32 层)、外部实体不读; DOCTYPE 的公共标识是 HTML 规范列的那几个
  XHTML / MathML 的就认 HTML 的命名字符引用; 有外部子集或参数实体引用且不是 standalone 时未声明的实体跳过, 否则是错), 换行规范化, 属性值
  空白规范化. 不良构时文档只有一个 `parsererror` 元素 (命名空间 `http://www.mozilla.org/newlayout/xml/parsererror.xml`, 文字是原因和行列).
  结果是 `Document` (接口照规范, `createDocument` 的才是 `XMLDocument`), `contentType` 是给的类型, URL 是页面的. 节点有 `CDATASection` 和
  `ProcessingInstruction` (render 里是带标记的文本 / 注释节点, `Ids.cdata` / `Ids.pis`, 同 doctype 的做法). 元素和属性带任意命名空间和前缀
  (`Ns::Other`, `namespace_uri` / `prefix` / `attributes_ns` / `*_attribute_ns`), `createElementNS` / `setAttributeNS` / `createAttributeNS`
  照规范 validate and extract (`InvalidCharacterError` / `NamespaceError`), `lookupNamespaceURI` / `lookupPrefix` / `isDefaultNamespace` 照
  locate a namespace. 每个节点有所属文档 (`Ids.owners`, 只记不是页面文档的, `Ids.documents` 记 DOMParser / `createDocument` /
  `createHTMLDocument` / `new Document()` 的文档和类型): 插入时 adopt 到父节点的文档, `importNode` / `adoptNode` 改, `ownerDocument` 跟着;
  属性名和 `getElementsByTagName` 只在 HTML 文档里的 HTML 元素上转小写, `tagName` 同理. XML 文档里元素的 `innerHTML` / `outerHTML` 用 XML
  序列化 (require well-formed, 不良构抛 `InvalidStateError`), 赋值用 XML 片段解析 (上下文元素的命名空间声明都在作用域里, 出错 `SyntaxError`).
  `XMLSerializer` 照 DOM Parsing 的 XML 序列化 (命名空间前缀表、`ns1` 生成前缀、空元素 `/>`), 和 WPT 记的浏览器行为一致的几处: 去掉没有命名空间的
  `xmlns` 属性、命名空间相同的元素也保留显式的 `xmlns=""`、属性的命名空间没声明过时总是生成新前缀. WPT 2026-11-09 macOS: `domparsing`
  147 → 277 / 2369 (`DOMParser-parseFromString-xml-parsererror` 23 / 23、`XMLSerializer-serializeToString` 29 / 33、实体、CDATA、往返全过;
  剩下的大多是 `innerHTML` 的 Trusted Types、`createContextualFragment`、`positional-methods` 那批和 iframe); `dom/nodes` 3066 → 4114 / 6112
  (62 个文件变好, 没有变差的; `createElementNS`、`createDocument`、`*NS` 的属性和集合、处理指令、选择器大小写).
- **XML 页面** (https://html.spec.whatwg.org/#read-xml, acde-dom `dom/page.ts` `loadPage`, render `Dom::parse_xml_document`): 从服务器取的页面
  (`PageSource.served`) 带着响应的 `Content-Type` 交给 `loadPage`; 类型的本质 (去掉参数、转小写) 是 XML MIME 类型 (`text/xml`、
  `application/xml` 或子类型以 `+xml` 结尾: `.xht` / `.xhtml` 的 `application/xhtml+xml`、`image/svg+xml` …) 时用上面的 XML 解析器解析成页面,
  别的类型和没有类型时照旧按 HTML. 页面这时是 XML 文档 (`Ids.page`, `document.contentType` 是那个类型): 页面里的节点 (之后建的、克隆的、
  `innerHTML` 解析的也是) 带 `IN_XML_DOCUMENT`, 属性名、`tagName`、选择器的类型名都区分大小写, `innerHTML` 用 XML 片段解析; `<style>` 里的
  CDATA 段就是样式文字, 照常渲染 (WPT `css/` 两千多个 reftest 的参考页 `ref-filled-green-100px-square.xht` 靠它画出绿方块). 不良构时页面只有
  一个 `parsererror` 元素. 按 HTML 再解析时页面回到 HTML 文档.
- **按名字和序号取成员** (WebIDL 的 legacy platform object, acde-dom `dom/legacy.ts` 的 `legacy(shape)` 出 Proxy 的 handler, 照规范的
  [[GetOwnProperty]] / [[DefineOwnProperty]] / [[Delete]] / [[OwnPropertyKeys]]): 序号是可枚举的自有属性, 名字是不可枚举的
  (`Object.keys` 只有序号, `in` 和 `getOwnPropertyNames` 有名字), 没有 setter 的写入失败 (严格模式抛 TypeError), `preventExtensions` 失败.
  - `HTMLCollection` (`document.forms.login`、`getElementsByTagName('input').title`、`children`、`select.options`、表格的 `rows` / `cells` /
    `tBodies`, 这三个现在是活的 `HTMLCollection`): 按文档顺序第一个 id 等于名字、或 HTML 元素的 `name` 等于名字的元素 (`namedItem` 同一段,
    一遍扫完); 自有属性和原型上的成员优先 (`length`、`item` 还是自己的).
  - `HTMLFormControlsCollection` (`form.elements`, 同一个对象; 不含 `<input type=image>`): 一个匹配给元素, 几个给活的 `RadioNodeList`
    (现在是 `NodeList` 的子类, `value` 读写选中的单选钮). `fieldset.elements` 是 `HTMLCollection`.
  - `Window` (named access, #170): `Window.prototype` 和 `EventTarget.prototype` 之间是一个 Proxy (`window.ts` `namedProperties`, 规范的
    WindowProperties): id 等于名字的元素 (原生 id 表, `getElementById`), 没有再找 `name` 等于名字的 `embed` / `form` / `img` / `object`.
    `window.box` 和裸名字 `box` 都能取到, 每次现查 (删除、改 id 立刻生效); 全局对象的自有属性、`Window.prototype` 的成员和 `let` / `const`
    声明优先; 不可枚举. 几个同名元素时只给第一个 (规范给 `HTMLCollection`), 不管 iframe 的 `name`.
  - `HTMLFormElement` (`[LegacyOverrideBuiltIns]`): `form[0]`、`form.title`. 先找它的 listed 元素 (不含图片按钮), 没有再找它里面的 `img`,
    都没有再查 past names map (元素改名后旧名字仍然指它, 元素不再属于这个表单时去掉); 名字盖过原型上的成员 (`<input name="submit">` 之后
    `form.submit` 是那个 input), 只有自有属性排在名字前面. 写入照普通对象走原型上的 setter (`form.action = …` 仍然改属性).
    表单的包装对象是 Proxy (`realm.ts` `exotic` 在构造时换上, 登记进包装表的就是它), 所以 **acde-dom 自己的代码不许从可能是表单的
    包装对象上按名字读成员**: 用 id 调原生接口, 或 `realm.ts` 的 `inner(node, 'parentNode')` (从原型读, receiver 还是这个对象; 页面加的
    自有属性也挡不住它). 测试 `test/named.test.ts` 最后一条给表单放上和每个成员同名的控件, 和没重名的表单逐项对照.
    每次按名字读表单是一次查表: 文档有改动 (`version` 变了) 后第一次读某个名字做一次 `querySelectorAll` (只在这个表单里; 页面出现过
    `form` 属性之后 (`flags.formAttributes`, 和 `inlineHandlers` 一样从标记和 `setAttribute` 记下) 才另查整个文档里 `form=<id>` 的控件),
    之后同一个名字不再调原生接口 (`test/cost.test.ts`: React 挂 500 个 input 的表单, 每次 append 一次查询).
  - `HTMLSelectElement` / `HTMLOptionsCollection`: `select[i]`、`select[i] = option` (null 是删掉, 超出长度先补空的 option, 不是
    `HTMLOptionElement` 抛 TypeError)、`length = n`.
  - 还没有: `document.<name>` 和 `window.<id>` (Document / Window 的具名属性)、`element.attributes.<name>` (`NamedNodeMap`)、
    `classList[0]` (`DOMTokenList`)、`map.areas`; 解析器给的 form owner (错位嵌套时 `img` 和控件属于哪个表单) 不记, 一律按最近的
    祖先 `form` 和 `form` 属性算.
- **通知**: `Notification` 转给桌面 (`on('notification')`); 桌面没在显示时 (调试窗口) 由宿主窗口发系统通知 (hostwin `system/notify.rs`). 页面给的文字
  当不可信数据: macOS 用 `osascript -l JavaScript -e <固定脚本>`, 文字只经环境变量 `ACDE_NOTIFY_*` 传 (放进 argv 会被当成选项); Linux 走
  `org.freedesktop.Notifications` (body 只在服务端报 `body-markup` 时转实体). Windows 没有系统通知 (宿主 `Shell` 不绑定 `notify`): 平台表 win32
  `standalone` 的 unsupported 里有 `notifications` (SDK `WEB_CAPABILITIES`: 不用在清单里声明的网页能力, `acde:capabilities` 照样列出, 不能当服务名),
  这时页面的 `Notification.permission` 是 `denied`、构造后发 `error` (`PageCalls` 不装 `env.notify`), Broker 的 `notify` 回 501 `unsupported`.
  Windows 完整桌面照常转给桌面.
- **声音**: `Audio` / `AudioContext` (acde-dom `src/audio/`, src/host `audio/`): 解码 (symphonia) 和混音 (每个 web 一个 48 kHz 立体声 f32
  `Mixer`, 线程 `acde-audio`) 到处一样. 输出: Linux PipeWire (`output.rs`; libacde 的三处 PipeWire 连接, 即这里、`acde:audio` 的 runtime `sound.rs` 和语音输入 `voice/mic.rs`, 都经 `runtime::pw_context` 建, 带 `module.rt = "false"` 不加载 module-rt: 没有 RTKit 时它把整个进程的 RLIMIT_RTTIME 设成 0 且收不回来, 之后任何实时线程一次没睡满就被内核 SIGKILL (daily-run3 桌面被杀); 512 帧的延迟不靠实时调度也够); macOS / Windows cpal 默认设备 (`output_cpal.rs`, 用设备当前的采样率,
  Hermite 重采样, 设备要 f32). 3 秒没声音关流; 没有设备 (或 `ACDE_AUDIO=null`) 按墙上时间渲染丢掉.
  - **`OfflineAudioContext`** (2026-11-19): 两种构造写法, 采样率 3000–768000 Hz 照给的 (`sampleRate` 是 context 自己的, `decodeAudioData` 也重采样到它),
    声道 1 或 2 (3–32 以 `NotSupportedError` 拒绝, 还没有), 状态从 `suspended` 开始. 它的图不进混音器: 建节点、连线、参数都落在页面线程上一个只给
    离线 context 的 `Mixer` 里 (src/host `audio/mod.rs` `offline_cmd`), 不开声音设备; `startRendering()` 把整张图交给 `acde-offline-audio` 线程池 (4 条)
    一次算完 `length` 帧 (最后一个 quantum 截断, 不钳到 ±1). 一次最多 2^28 个样本 (帧数乘声道, 1 GB; 构造时超了是 `NotSupportedError`), 一个页面的声音内存 (src/host `audio/budget.rs`:
    在算的离线渲染、延迟线、卷积核、振荡器波表) 加起来也最多这么多 (离线渲染超了 promise 以 `RangeError` 拒绝), 页面没了时它还在算的渲染停下, 结果存成 AudioBuffer: 先发结束的音源的 `ended`, 状态变 `closed`, 兑现 promise, 下一个任务
    发 `complete` (`OfflineAudioCompletionEvent.renderedBuffer`). 第二次 `startRendering` 是 `InvalidStateError`; `suspend(time)` 还没有
    (`NotSupportedError`), `resume()` 是 `InvalidStateError`. `currentTime` 是已经渲染到的帧 (按 quantum). 渲染开始后再对这张图做的改动不生效.
  - **空间化** (`PannerNode` / `AudioListener`, src/host `audio/spatial.rs`): 方位角、仰角、三种距离模型、声锥和 `equalpower` 照规范
    §6 的算法 (参数在一个 quantum 里变时逐样本算, 不变时每 quantum 算一次), 单元测试对着 Chrome 的 `OfflineAudioContext` 输出 (2e-5 内).
    和规范伪代码不同、照 Chrome 的一处: 声锥用「声源指向听者」的方向 (正对听者的声源增益是 1). 监听者是每个 context 一个隐藏的图节点
    (`Kind::Listener`, 9 个参数, 第一次读 `context.listener` 时建), 排序时每个 panner 排在它后面; 还没建时用规范的默认值.
    `HRTF` 不是实测的冲激响应 (仓库里没有 HRTF 数据库): 球形头模型 (Brown & Duda 1998), 每只耳朵一个分数延迟 (Woodworth 公式, 头半径
    0.10 m; 8 抽头加窗 sinc, 多 4 个样本的固定延迟) 和一个头影滤波器 (一极一零), 外加一个后方衰减; 各方向的宽带电平和耳间时差按 Chrome
    (IRCAM 数据) 的白噪声测量标定 (`EAR_LEVELS` / `REAR_LEVELS`, 测试里 ±0.05、±3 个样本). 没有耳廓的仰角线索. 照规范 HRTF 下参数按 k-rate,
    每 quantum 算一次, 增益、延迟和滤波系数在 quantum 内线性过渡 (不出拉链声); 立体声输入左进左耳、右进右耳 (和 Chrome 一样不交叉).
    输入变静时清掉延迟线和滤波器状态.
  - **振荡器** (`OscillatorNode` / `PeriodicWave`, src/host `audio/wave.rs`): 四种基本波形和自定义 `PeriodicWave` 一样是规范的傅里叶级数,
    照 Chromium 的 `PeriodicWave` 做成限带波表: 表长按采样率 (≤ 24 kHz 2048, ≤ 88.2 kHz 4096, 以上 16384), 每八度 3 档, 每档砍掉会混叠的
    分量, 归一化 (峰值 1) 按第一档算; 播放时表内和相邻两档之间各线性插值. 波表在页面线程建好再交给混音线程: 基本波形按 (形状, 采样率)
    缓存 (弱引用, 没有振荡器用了就释放), 自定义波形在构造 `PeriodicWave` 时建; 都记进页面的声音内存, 放不下时抛 `RangeError`.
    对 Chrome: 四种波形各三个频率和指数扫频、负频率、detune、22.05 / 96 kHz、自定义 (归一化与否) 每样本差 5e-5 内.
  - **效果节点** (2026-11-19, src/host `audio/` 下各一个文件, 在 `graph.rs` 的 `Effect` 里): 都在混音线程按 quantum 算, 页面只发设置 (`NodeOp`).
    - `BiquadFilterNode` (`biquad.rs`): 系数照 Chromium `Biquad` (f64, 含频率 0 / 奈奎斯特和 Q ≤ 0 的边界), 参数在 quantum 里变时逐样本重算;
      `getFrequencyResponse` 在页面线程用当前参数算 (超出 0–奈奎斯特的频率给 NaN).
    - `DelayNode` (`delay.rs`): 每声道一条环形延迟线, 线性插值; `maxDelayTime` 0–180 s (以外 `NotSupportedError`). 延迟线在页面线程
      分配、记进页面的声音内存, 放不下时构造抛 `RangeError` (和分配不出的 `ArrayBuffer` 一样), 不在混音线程分配. 在环路里的延迟节点
      先读后写, 最少 128 帧 (规范的要求); 环路里没有延迟节点时整个环静音 (`muted`).
    - `WaveShaperNode` (`shaper.rs`): 曲线映射和 2x / 4x 过采样的上下采样滤波器照 Chromium (128 / 256 抽头核, 4x 是两级 2x).
    - `DynamicsCompressorNode` (`compressor.rs`): 移植 Chromium 的 `DynamicsCompressorKernel` (BSD), `reduction` 每 quantum 回填.
    - `ConvolverNode` (`convolver.rs`, realfft): 三级非均匀分块的重叠保留 (128 / 1024 / 8192 帧一块), 没有额外延迟;
      `normalize` 照规范的算法, 只是校准增益照 Chrome 和 Firefox 用 -58 dB (规范写成四舍五入的 0.00125, 差 0.7%). 设 `buffer` 时在页面线程
      算好各块的频谱 (和 Chrome 一样同步), 单声道冲激响应的左右两路共用一份频谱, 记进页面的声音内存, 放不下时抛 `RangeError`. 冲激响应 1、2 声道 (4 声道是 `NotSupportedError`, 还没有), 采样率要和 context 相同.
    - `AnalyserNode` (`analyser.rs`): 混音线程把输入的单声道下混记进 32768 帧的环 (`Shared.tap`); `get*Data` 在页面线程取最后 `fftSize`
      帧, Blackman 窗、FFT、平滑和分贝照规范 (同一批样本只算一次).
    - 尾巴 (`Node::tail`): 输入静了以后节点还会出声的帧数 (延迟 = `maxDelayTime`, 压缩器 1.625 s, 卷积 = 冲激响应长度, 滤波器到输出低于 1e-7),
      尾巴没完的节点不回收、算作还活着.
    - 对 Chrome: 同一段脚本 (`OfflineAudioContext`, 每种节点) 在 Chrome 和 acdev 里各跑一遍, 结果对比见 libacde 提交信息.
- **视频** (src/media `acde-media`): 用系统的框架解码, 不随包带编解码器 (用户定, 2026-11-09; libacde 闭源, 不链接 GPL 的 ffmpeg).
  - `Player::open(bytes)` 探出 `Info {duration, width, height, audio, video}`, 每个轨一条读线程 (`acde-video` / `acde-video-audio`) 各自往
    有界队列里解: 视频 RGBA 帧 (领先 4 帧), 音频 48 kHz 立体声 f32 交错 (领先半秒). 两个轨分开读, 不会一个轨满了卡住解封装器让另一个轨饿死.
    `seek(t)`: 清队列, 两条线程都停下后由最后一条重启解码器. 消费方拉: `take_audio` (混音器线程)、`frame_at(t)` (取到时间的最后一帧,
    更早的丢掉)、`first_frame` (等第一帧, 给海报帧和尺寸).
  - Linux `gst.rs`: 先按开头的字节认容器 (MP4 / QuickTime 的 `ftyp`、Matroska / WebM 的 EBML、Ogg), 自己放解封装器
    (`qtdemux` / `matroskademux` / `oggdemux`), 不让 GStreamer 自己挑: 自动挑会把 HLS / DASH 播放列表之类交给会去取别的 URL (含 `file://`)
    的元素. 再保险一层: 管道里加进任何 Source 类或读 URI 的元素 (`deep-element-added`) 就整个拒绝. `giostreamsrc` 读内存里的字节
    (`GMemoryInputStream`, 能随机读) → 解封装器 → 一个 `multiqueue` (一个轨空了它会给另一个轨多存, 两个轨交错远也不卡死) → 每个轨一个
    `decodebin3` (只剩解析和解码) → 视频 `videoconvert ! videoflip video-direction=auto ! videoconvert` 成 RGBA (旋转标签在这里转正), 音频
    `audioconvert ! audioresample` 成 F32LE 48 kHz 立体声, 各进一个 `appsink` (不按时钟, 最多 2 个缓冲). 解封装器发 `no-more-pads`、
    预滚完、要的轨都接上才报 Info. seek 是 FLUSH | ACCURATE, 目标前的帧和样本丢掉. 运行时要 gst-plugins-base / gst-plugins-good (MP4、WebM 解封装, Opus, VP8 / VP9) 和
    gst-plugin-va (硬解); H.264 / AAC 的软解是 gst-libav (经 Arch 的 GPL ffmpeg), Arch 包里是可选依赖.
  - macOS `apple.rs`: `AVURLAsset` 带 `AVURLAssetReferenceRestrictionsKey = ForbidAll` 开 (文件里引用别的文件或 URL 一律不跟), 不许走
    网络; 数据由 `AVAssetResourceLoader` 代理 (`AcdeMediaLoader`) 从内存按字节范围给, `AVAssetReader` 解出
    BGRA 帧 (拷出时换成 RGBA, 按轨道的 `preferredTransform` 转 90 / 180 / 270 度) 和 LPCM; 每个输出各自一把锁; seek 是取消旧的
    reader 按时间范围新建. AVFoundation 只认 MP4 / QuickTime: WebM 在 macOS 上打不开 (Safari 自己解析 WebM, 这里没有).
  - `<video>` (acde-dom `dom/media.ts` `HTMLVideoElement`, src/host `audio/mod.rs` `openVideo` / `present`, `audio/video.rs`): 和
    `<audio>` 共用 `HTMLMediaElement` 和混音器. `openVideo` 在线程 `acde-video` 里开 `Player`、等第一帧, 再把 `VideoSound` 交给这个 web 的
    混音器 (`Cmd::AddMedia`, 混音器的 `MediaSource`): 有声音时媒体时间跟着放出去的声音走 (解码跟不上就停在那里等), 没声音或声音轨先完了
    按混音器的时间走, 到 `duration` 就结束 (`ended`). 帧在页面的动画帧里交 (`presentVideos`, 在播或刚 seek 的视频才要帧): `present`
    取到媒体时间为止的最后一帧 (seek 后先交新位置的第一帧, 看 `Player::epoch`), 写进每个视频一张的 `GpuImage` (`Gpu::write`, 渲染时
    vello 按 generation 拷), 交给 render `Dom::video_image`: 节点的画面 (帧像素坐标, blitz 按 `object-fit` / `object-position` 放进内容框,
    UA 样式 `object-fit: contain`) 加固有尺寸 (`set_video_size`, 变了重排; 没有帧时是 300×150). `videoWidth` / `videoHeight` 在
    `loadedmetadata` 时就有; 之后帧尺寸变了发 `resize` (`readyState` 是 `HAVE_NOTHING` 时不发, 照标准). `canPlayType` 按 `acde_media::TYPES`
    (平台能开的容器). 换源 / 去掉 `src` 清掉画面 (`clearVideo`); 播放器释放 (`Player` 的 drop) 在单独线程里停解码器, 不卡混音器和 JS.
  - 解析出来的 (`loadPage`、`innerHTML` / `outerHTML` / `insertAdjacentHTML`) 和克隆出来的 (`cloneNode` / `importNode`) `<video>` /
    `<audio>` 有 `src` 或 `<source>` 就自己开始加载 (标准: 设了 `src` 就跑加载算法), `preload="none"` 且没有 `autoplay` 时发 `loadstart` 和
    `suspend` 后停在 `NETWORK_IDLE`, 等 `play()` 再取. 还没有: 海报图 (`poster`)、`seeked` 等到新位置的帧画出来再发、文字轨、MSE.
  - 帧现在在 CPU 内存里, 送进 GPU 要上传一次; 硬解的零拷贝 (Linux dmabuf、macOS IOSurface) 还没有.
- **设备的标准接口** (acde-dom `src/device/`, src/host `device.rs`, 每个 web 都有的原生 `device`): 系统的状态只经 web 标准接口给网页,
  改设置的不在这里 (标准里没有, 那些归 `acde:power` 等模块). `navigator.getBattery()` (Battery Status): Linux 上读 UPower 的 DisplayDevice
  (runtime `battery.rs`), 属性变了经 PropertiesChanged 推给同一个 `BatteryManager` 并发 `chargingchange` / `levelchange` / `chargingtimechange` /
  `dischargingtimechange`; 没有电池、没有 UPower 或别的平台按规范给接着电源的默认值 (charging true, level 1, chargingTime 0, dischargingTime Infinity).
  每个 navigator 只有一个 BatteryManager, `getBattery()` 回同一个 promise.
  `navigator.wakeLock.request('screen')` (Screen Wake Lock, 只在 Linux; 别处没有 `navigator.wakeLock`): runtime `wakelock.rs` 拿 logind 的
  `idle` inhibitor (acdebin-system 的电源策略认它) 外加会话总线上 `org.freedesktop.ScreenSaver.Inhibit` (GNOME / KDE 宿主桌面认它), 两个都拿不到就
  NotAllowedError; 页面不可见时请求被拒, 变成不可见时放掉所有锁并发 `release`.
  `IdleDetector` (Idle Detection, 只在 Linux): runtime `idle.rs` 读 logind 会话的 `IdleHint` / `IdleSinceHintMonotonic` / `LockedHint` 并跟着
  PropertiesChanged; `userState` 是 idle 当且仅当会话标了空闲且已空闲到 threshold (不到就定时补算), `screenState` 看 LockedHint. 空闲提示由桌面写给
  logind (acdebin-system 的合成器、宿主桌面的会话管理器), 所以比 threshold 更早的空闲要等桌面先标. 授权是审批能力 `idleDetection` (清单声明,
  `IdleDetector.requestPermission()` 弹 acdev 的批准提示, `permissions.query({name: 'idle-detection'})` 看它); threshold 小于 1 分钟 TypeError.
- **摄像头和麦克风** (`getUserMedia`; src/media `capture/`, src/host `capture.rs` / `audio/live.rs`, acde-dom `src/media/stream.ts`):
  - 权限: 清单 `capabilities` 写了 `camera` / `microphone` 的应用才拿得到 (Broker `#grants` → web 的 grant →
    `natives.capture` 只装给有的 web, 桌面两个都有); 没写的: `getUserMedia` `NotAllowedError`、`enumerateDevices` 不列这一类、
    `navigator.permissions.query` 是 `denied`. 写了的第三方应用还要用户批准 (见 `docs/runtime/webs.md`「批准」), **标准接口照浏览器的样子带出这一步**
    (`broker/access/approval/permits.ts` `Permits.capture(native, access)`, web 起来时包在 `natives.capture` 外面; acde-dom 只认
    `CaptureNative` 上两个可选成员 `request(kind)` / `onPermission(cb)`, 不知道批准是什么):
    - `navigator.permissions.query({name: 'camera'})`: 批准了 → 系统的状态 (`granted` / `prompt` / `denied`); 还没问或正在问 (`not-asked` /
      `asking`) → `prompt`; 被拒、撤销、平台没有这项能力 (`WebInit.tier.capabilities` 里是 `unsupported`) → `denied`. 批准状态一变
      (`Permits.set`, 不管是 `getUserMedia`、`acde:capabilities` `request()` 还是桌面 / `acdev allow` / `revoke` 引起的) `PermissionStatus` 发 `change`.
    - `getUserMedia`: 先照旧选设备和模式 (`denied` 当场 `NotAllowedError`; 选不出来 `NotFoundError` / `OverconstrainedError`, 都不问人), 选好后对要的每一类
      调 `request`: 状态是 `prompt` 时经 rpc `capabilities.request` 弹批准提示 (和 `acde:capabilities` `request()` 同一个, `reason: 'request'`, 已经在问的并进去),
      答应了才开设备, 拒绝 (或问不成) 是 `NotAllowedError: Permission denied`, 不开设备. 音频和视频都要时先问 `microphone` 再问 `camera`, 一个一个来,
      前一个被拒就不问后一个. 必需的 `camera` / `microphone` 启动前已经问过, 到这里是 `granted`, 不再问.
    - `enumerateDevices`: 没批准的一类只给一个空 id / 空 label 的条目 (和系统没答应时一样, `Permits.#listed`); `getUserMedia` 内部选设备时看得到全部.
    - 没批准时 `open` 被拒 (撤销之后的 `applyConstraints` 重开设备会走到): 错误消息以 `NotAllowedError: ` 开头 (页面得到 `NotAllowedError`), 仍带
      `code` / `capability`.
    - 还没有: 撤销之后已经在用的轨道不会 `ended` (它开着的设备照常出数据), 只是不能再开新的.
    批准之后还有系统的询问: macOS TCC (`AVCaptureDevice requestAccess`, 第一次 `getUserMedia`
    时弹; 运行时程序的 `__TEXT,__info_plist` 段带 `NSCameraUsageDescription` / `NSMicrophoneUsageDescription`, 见 src/host
    `build.rs`; 从终端起时 TCC 记在终端名下); Linux 本机程序直接读 PipeWire / V4L2, 没有系统询问. acdev 自己不画询问框 (批准提示由桌面或 `acdev allow` 答).
    `navigator.permissions` 是真的 `Permissions` / `PermissionStatus`.
  - 设备 (`acde_media::capture`): Linux 用 GStreamer `DeviceMonitor` (`Audio/Source`、`Video/Source`, PipeWire / V4L2 提供者, 去掉
    输出的 monitor), 打开用设备自己的元素 (`create_element`), 摄像头按选中的模式 (设备 caps 和宽高帧率的交集) 解成 RGBA, 麦克风转成
    48 kHz 立体声 f32; macOS 用 `AVCaptureDeviceDiscoverySession` 和 `AVCaptureSession` (视频 BGRA → RGBA, 音频 LPCM f32), 会话在自己的
    线程里开关, 每 250 ms 看设备还连着没有. 摄像头只留最新一帧 (带序号), 麦克风写进 2 秒的环, 每个读者 (`Tap`) 从它创建时开始读, 落后太多跳到
    最近. 设备没了 / 出错 (`on_end`) → 这个设备的轨道都 `ended`. `deviceId` / `groupId` 是按应用加盐的散列, 跨应用对不上.
  - 页面 (`stream.ts`): `getUserMedia` 照标准的 SelectSettings 选设备和模式 (必需的 exact / min / max 过滤, `advanced` 依次收窄, ideal 按
    fitness distance 取最小), 都不行 `OverconstrainedError` (带 `constraint`); 选好后才问系统, 再开. 支持的约束: deviceId、groupId、
    facingMode、resizeMode (只有 `none`: 不缩放, 只给设备原生的模式)、width、height、aspectRatio、frameRate、sampleRate (48000)、
    sampleSize (32)、channelCount、echoCancellation / autoGainControl / noiseSuppression (只有 false: 不做处理). 同一设备的轨道
    (clone) 共用一个打开的源, 最后一个 `stop()` 才关; `applyConstraints` 要换模式时重开设备, 只换这个轨道. `enumerateDevices` 返回
    `InputDeviceInfo` (`getCapabilities`), 系统还没答应的一类只给一个空 id / 空 label 的条目 (标准的隐私规则).
  - 播放: `video.srcObject = stream` (`audio` 元素只放声音) → `openStream`: 画面每个动画帧取摄像头的最新一帧 (和 `<video>` 文件一样进
    `GpuImage`), 声音是混音器里的 `LiveSound` (没有声音轨时只走时钟, `currentTime` 照样走); 时长 `Infinity`, 不能 seek, `currentSrc` 是
    空串; 轨道 `enabled = false`: 画面黑、声音静; 轨道结束 / 换源时重开, 流里没有活的轨道了发 `ended`. Web Audio
    `createMediaStreamSource` (图里的 `Kind::Stream` 节点, 读 id 最小的音频轨, 轨道停用 / 结束时断开). 实时读取保持 20 ms 左右的延迟,
    积压超过 100 ms 就丢掉旧的.
  - 测试: `ACDE_CAPTURE=fake` 换成假设备 (像 Chrome 的 fake media device): 一个麦克风 (440 Hz, 0.25) 和一个摄像头 (320x240 / 640x480 /
    1280x720 @30, 蓝底 `#2080e0` 上一条每秒扫过的白条), 同一 group; `acde_media::capture::fake::unplug` 模拟拔掉. Linux 上没有摄像头时可以
    `gst-launch-1.0 videotestsrc is-live=true ! video/x-raw,format=I420,width=640,height=480,framerate=30/1 ! pipewiresink mode=provide
    "stream-properties=props,media.class=Video/Source,media.role=Camera"` 做一个 PipeWire 摄像头.
  - 还没有: `devicechange` (不监视插拔)、`getDisplayMedia` (录屏)、`MediaRecorder`、`createMediaStreamDestination`、回声消除等处理、
    `resizeMode: crop-and-scale`、轨道的 `mute` / `unmute` (系统暂停时).
- **Web Worker** (2026-11-19, 专用 worker; src/host `worker.rs`、`envelope.rs`, acde-dom `core/worker.ts` (页面一侧的 `Worker`) 和 `worker/` (worker 的全局),
  页面胶水 `page/src/webworker.ts` 打成 `acde-runtime:/webworker.js`):
  - 每个 worker 一条线程 (`acde-worker`, 8 MiB 栈) 一个 isolate; 名字 `<web>/worker-<n>[:<name>]` 进日志和 `perf::task`. 线程不阻塞页面地
    起: 消息通道先建好 (`rt::Runtime::with_channel`), 脚本还没到时发的消息在 worker 里排着, 脚本跑完 (模块是开始求值后) 再按顺序派发.
  - 顶层脚本由页面取 (`scriptSource`: 应用的文件、`blob:`、`data:`、后端), 只许同源、`blob:`、`data:` (别的 `SecurityError`); 取不到时 `Worker` 上发
    普通的 `error` 事件, worker 结束. `type: 'module'` 的静态 `import` 在 worker 里经 `WebLoader` 读应用文件 (worker 带着页面的资源目录);
    `blob:` / `data:` 的模块脚本照样要 JavaScript MIME 类型.
  - worker 的全局是 `DedicatedWorkerGlobalScope` (`self`、`name`、`location` (`WorkerLocation`)、`navigator` (`WorkerNavigator`, 取页面的
    userAgent、语言、核数)、`postMessage`、`close`、`importScripts`、`onmessage` / `onerror` …), 加核心接口 (定时器、console、URL、编码、
    `structuredClone`、Blob、事件) 和网络 (`fetch`、XHR、WebSocket、EventSource、流; 应用文件和后端都通, 后端用页面的配置).
  - 消息走 v8 的 ValueSerializer (`Envelope`): `postMessage` 当场序列化, 转移列表先整体检查 (重复、已分离、不可分离如 wasm 内存、不能转移的
    类型都是 `DataCloneError`, 这时什么都不分离), 写完才分离; 转移的 ArrayBuffer 把 backing store 原样交给对方 isolate, 不复制 (发送方的
    isolate 没了也照样能读). 反序列化失败发 `messageerror`.
  - 错误: worker 里没捕获的异常先在 worker 的全局上发可取消的 `ErrorEvent`, 没取消就到页面的 `Worker` 上再发一次 (`message`、`filename`、
    `lineno`、`colno`), 还没取消就按页面的未捕获错误报告. 未处理的 rejection 在 worker 里发 `unhandledrejection`, 不转给页面.
  - `terminate()` 立刻停 (正在跑的 JS, 包括 wasm 死循环, 用 `terminate_execution` 打断), 之后的消息丢掉; `close()` 让当前任务跑完, 已排队的
    任务和定时器都不再跑 (`rt` 的循环在 `exit` 后不再取任务). 页面没了时它的 worker 都停. `Worker` 对象在 worker 结束前由页面一侧持有, 不会因为
    没人引用被回收.
  - 限制: 一个页面同时跑的 worker 最多 64 个, 多出来的排队, 前面的结束了才起 (Firefox 也是排队); 页面和它的 worker 之间在路上 (发出还没送达)
    的消息合计最多 1 GiB, 超了 `postMessage` 抛 `DataCloneError` (和 Chrome 序列化内存不够时一样). 页面有堆上限 (`heapMb`) 时, 它的 worker
    合起来另有一份同样大的堆预算: 每个 worker 起时先领 16 MB (剩的不到 8 MB 就不起, `Worker` 上发 `error`), 堆快满时 (V8 的 near-heap-limit
    回调) 再从预算里领当前的一半 (至少 8 MB), 领不到就只停这个 worker (报 `memory`), 结束时还回去; 所以一个 web 连同 worker 最多用两份 `heapMb`. worker
    线程的 CPU (包括已经结束的) 算进这个 web 的 `cpuMs`.
  - 对 Chrome: 同一页测试 (早到的消息、`name` / `location` / `navigator`、worker 里 `fetch` 和 `importScripts`、Float32Array 转移 300 次来回、错误
    事件、`close`、模块 worker、`blob:` worker 里跑 wasm、`terminate` 打断死循环、各种拒绝) 结果一致; 256 KB 转移来回一次 0.016 ms (Chrome 0.017).
  - 还没有: 把 `MessagePort` 转给 worker (抛 `DataCloneError`)、`SharedArrayBuffer`、克隆 `WebAssembly.Module`、worker 里的
    `requestAnimationFrame` / OffscreenCanvas / IndexedDB / Cache API / cookie、嵌套 worker、`importScripts` 后端的脚本和页面建的 `blob:` URL
    (`NetworkError`)、worker 里的同步 XHR; DevTools 看不到 worker (没有 worker 目标, Network 面板不记它的请求), dev 的 console 记录不含 worker 的输出.
- **MessageChannel / MessagePort** (acde-dom `core/message.ts`): 同一个页面里两端互发, `postMessage` 当场结构化克隆; 没有 `start()` 也没设
  `onmessage` 之前消息在收端排队, `start()` 后按顺序派发. 关闭 (2026-11-21, 全局行为变化, 照 HTML 规范和 Chrome 123 起的做法): 一端
  `close()` 后, 另一端在下一个任务里收到一次 `close` 事件 (`onclose` 也有), 之后两端互发都静默丢弃; 关的那一端自己不收 `close`, 再关一次什么也不做.
- **启动时交给页面的端口** (2026-11-21; 运行时一侧见 `docs/runtime/layout.md` 同名一条): 管家要给页面一条通往别处的连接时 (如 Dopame 的
  插件窗口), 页面在 `load` 事件之后的下一个任务里收到一次标准的 `message` 事件 (发在 `window` 上, `data` 由管家给, `ports[0]` 是一个
  `MessagePort`, `source` 为 null). 页面的约定:
  - 监听器要在 `load` 之前挂上 (同步脚本或模块顶层都行); 错过了就收不到, 不会补发, 一次页面加载只给一个端口; 重载后的新页面再给一个新的.
  - 端口照标准用: `start()` 或设 `onmessage` 之前消息排队; `close()` 后对端断开; 对端断开 (或根本没连上) 时端口收到 `close`.
  - 只走 JSON: `postMessage` 的值要是普通 JSON (对象、数组、字符串、有限数、布尔、null), 不是时当场抛 `DataCloneError`
    (`this port carries JSON only (…)`); 编成 JSON 后超过 262144 字节也抛 `DataCloneError` (`message is N bytes, over the 262144-byte limit of this port`).
    收到的消息不是 JSON 或超长时发 `messageerror`, 端口不关.
  - 全局对象上没有任何私有字段, 也没有额外的 `acde:` 模块.
- **还没有**: shadow root 里的 `<style>` 不隔离 (会作用到整个文档)、XML 文档里的样式表处理指令 (`<?xml-stylesheet?>`)、从文件读的页面 (`PageSource.html`) 只认 `.html` 入口 (`.xhtml` 文件不按 XML 解析)、页面按响应的字符编码解码 (现在总按 UTF-8)、`DecompressionStream` / `CompressionStream`、`speechSynthesis`; `<select>` 的 `appearance: base-select` / `::picker(select)`、`<datalist>`、Storage Buckets、`SharedWorker` 和 Service Worker、`crypto.subtle`、OffscreenCanvas、全屏、`:target`; 从别的主机 `import` ES 模块 (`modules.rs`; `blob:` 和 `data:` 模块可以: `compile_graph` 经 `host.setModuleSource` 登记的 acde-dom `moduleSource` 同步取文字, 只认 JavaScript MIME 类型, 和 fetch 走同一个 `inline`).
- **指针锁定** (https://w3c.github.io/pointerlock/, acde-dom `dom/pointerlock.ts`, 2026-11-19): `Element.requestPointerLock(options?)` 返回 promise,
  `document.pointerLockElement` / `ShadowRoot.pointerLockElement` (和 `activeElement` 一样不往影子宿主换), `document.exitPointerLock()`,
  `pointerlockchange` / `pointerlockerror` 发在 document 上 (同一个任务里先发事件再落定 promise). 拒绝: 元素不在文档里或窗口没有焦点
  `WrongDocumentError`, `unadjustedMovement: true` 和不支持的窗口 `NotSupportedError`, 已有一个请求在等 `InvalidStateError`, 宿主不给
  `NotAllowedError`, 没有用户激活 (见下条) 也 `NotAllowedError`, 除非上一次锁是页面自己 `exitPointerLock` 放开的; 宿主收回 (Esc 等) 之后要比收回更晚的一次激活, 页面不能在
  `pointerlockchange` 里拿锁前那次点击马上再锁回去;
  已经锁着时换元素直接成功, 不问宿主. Esc 由宿主收回, 页面拦不住. 窗口能不能锁看
  surface 的 `pointerLock` (`platform::Window::pointer_lock`, 缺省 false: 会话和 C 接口没有; 宿主窗口见 acdev `docs/hostwin.md`「输入」),
  能锁时 acde-host `host.ts` 把 `env.pointerLock` 接到 `surface.lockPointer(on)` (`SurfaceCommand::PointerLock`). 宿主的回答是
  `pointerlock {code: 1 | 0, title: 'request'}` (每个请求恰好一次), 宿主收回 (Esc、失焦、焦点给了别的窗口) 是 `pointerlock {code: 0, title: 'lost'}`;
  页面自己 `exitPointerLock` 不等回答. 锁着时 `pointermove` 带 `deltaX` / `deltaY` (按帧合并时相加) 成为 `movementX` / `movementY`, 位置停在锁定时的点,
  所有指针事件 (含 click、wheel) 都发给锁定的元素, 不发 over / out / enter / leave. 元素离开文档时解锁 (发 `pointerlockchange`).
- **用户激活** (https://html.spec.whatwg.org/#user-activation, acde-dom `dom/activation.ts`): 真的输入 (acde-dom `Input`) 里按下指针按钮、按下
  Esc 以外的键时记一次激活, 在派发事件之前, 所以这次的处理函数里就有临时激活 (transient activation, 5 秒内有效, 不消耗). `navigator.userActivation`
  是同一个 `UserActivation` (`hasBeenActive` / `isActive`). 页面自己 `dispatchEvent` 的事件不算. 现在只有指针锁定看它.
- **剪贴板** (libacde 单子 11, 形状与 acdev-lead、system-lead 定): 宿主接口按网页 MIME 类型收发字节 (`render/surface.rs`): `SetClipboard(Vec<ClipboardEntry>)`
  写一项的全部表示并整个替换剪贴板, `ClipboardTypes` 列出有哪些类型, `ReadClipboard {mime, reply}` 取一种 (没有时 `None`). 认的类型: `text/plain`
  (UTF-8)、`image/png` (PNG 字节)、`text/uri-list` (UTF-8, CRLF 分行). Wayland 宿主上 `text/plain` 对应 `text/plain;charset=utf-8`、`text/plain`、
  `UTF8_STRING` 三个 MIME, 写入时都给同一份字节; 列类型时按对方宣告的顺序换成网页名字, 不认识的去掉, 重复的合并 (`ClipboardEntry::wayland_mimes` /
  `web_types`); 剪贴板是自己持有时读和列类型直接用本地条目, 不走管道 (免得自己等自己). C 的 ffi 只支持文本: 列出的只有 `text/plain`, 别的类型回空.
  页面 (`webnative.rs` 的 `setClipboard` / `clipboardTypes` / `readClipboard`, 2 s 没回答当作空): `navigator.clipboard.readText` / `writeText`,
  `read()` (一项 `ClipboardItem`, 剪贴板空时空数组) / `write([item])` (只写第一项; 有不认的类型时 `NotAllowedError`), `ClipboardItem`
  (`types`、`getType` (没有时 `NotFoundError`)、`presentationStyle`、`supports()` 认上面三种). `paste` 事件的 `clipboardData` 带上剪贴板的全部类型:
  文字类型照旧 `setData`, 图片是一个 `kind: 'file'`、`type: 'image/png'` 的项 (`files` 里是 `image.png`); 页面自己处理的 `copy` / `cut` 写
  `text/plain` 和 `text/uri-list`. 权限照旧 (`clipboard-read` / `clipboard-write`). 宿主各平台的限制见 acdev `docs/hostwin.md`.
- **密码框** (`<input type=password>`, 2026-11-18): 照成熟浏览器的做法. 显示: 渲染层给 blitz 的编辑器的是掩码 (`engine_state.rs` 的
  `show_form_state`, 每个 UTF-16 码元一个 `•`, 所以页面的选区偏移不用换算; 没改过值时也按 `value` 属性掩码, `type` 改掉后恢复明文),
  无障碍树里本来就只给 `•••`. 复制和剪切: 在密码框里不发 `copy` / `cut` 事件, 也不写剪贴板 (`js/page/src/text.ts` 的 `clipboard`),
  粘贴照常. 输入法: 告诉输入法这是密码框, 关掉预编辑、周边文字和候选面板 (`ime.ts` 的 `focus`, `text.ts` 的 `preedit` / `panel`),
  密码不再以周边文字的形式送给输入法; 输入法直接提交的文字照常插入.
- **语音输入** (`src/runtime/voice/`, Linux): `listen(onEvent)` 从 PipeWire 默认输入录 16 kHz 单声道, 连豆包输入法的流式识别 (非官方接口,
  身份存 `$XDG_STATE_HOME/acde/doubao-ime.json`). 事件 `partial {text}`、结束时恰好一个 `final` 或 `error`; `stop(id)` 等定稿 (最多 10 秒), 60 秒自动停,
  新的 `listen` 取消上一个. 日志 target `acde_runtime::voice` (识别出的文字不进日志). 没有麦克风时用 PipeWire 的 `module-pipe-source` 喂 PCM.
