# rustdog 命令使用手册

这份文档专门讲 `rdog` 怎么用。

如果你只想先跑起来,看下面这几条就够了:

```bash
# 开一个最普通的监听
rdog listen 55600

# 连到目标主机,并把 bash 绑定过去
rdog connect -s /bin/bash 192.168.1.10 55600

# 在 Linux 上用配置文件启动常驻模式
rdog daemon --config ./rdog_linux.toml

# 在 Windows 上额外启用隐藏常驻模式
rdog hidden-daemon

# 用本地 stdio 作为 agent 控制桥
rdog control 127.0.0.1 5555

# 在当前目录生成 3 份平台配置模版
rdog config init
```

`rdog` 当前有 6 个主命令:

- `listen`: 开一个监听,等别人连进来。
- `connect`: 主动连到目标主机,把本地 shell 挂过去。
- `control`: 作为智能体控制端去连接远端 shell 服务。
- `daemon`: 按配置长期运行,适合持续重试和持续监听。
- `hidden-daemon`: 仅 Windows 可用的额外隐藏常驻入口,不会替换普通 `daemon`。
- `config`: 管理 `daemon` 相关配置文件,比如生成 `rdog_win.toml`、`rdog_macos.toml`、`rdog_linux.toml`。

如果你喜欢少敲字,也可以用别名:

- `rdog l`
- `rdog c`
- `rdog control`
- `rdog d`
- `rdog hd`
- `rdog cfg`

## 1. 先认清 3 种运行方式

有个边界最好先说清楚,不然后面很容易用错:

- `listen` 是一次性监听。它当前只接受 1 次连接。那次连接结束后,命令本身也会结束。
- `connect` 也是一次性连接。连上后跑完这次 session 就结束。
- `daemon` 才是常驻模式。它会按配置持续工作,遇到失败会重试,不会因为一次连接结束就自己退出。

如果你要的是"一直挂着,失败后自己再试",直接用 `daemon`。不要拿 `listen` 或 `connect` 去硬撑这个场景。

## 2. 参数写法约定

`listen` 和 `connect` 都使用位置参数传地址信息,程序内部支持两种写法:

```bash
# 只写 1 段
rdog listen 55600

# 写 2 段
rdog listen 0.0.0.0 55600
```

规则是这样的:

- 只写 1 段时,这一段会被当成端口,host 默认补成 `0.0.0.0`。
- 写 2 段时,按 `host port` 解释。

这个规则对 `listen` 很顺手。
对 `connect` 来说,虽然内部也是这么解析,但实际使用时还是建议你老老实实写完整的 `host port`,不然很容易把目标地址写偏。

## 2.1 `config` 用法

如果你想先在当前目录生成 3 份平台模板,最直接的命令就是:

```bash
rdog config init
```

这个命令会把仓库当前维护的 3 份模版写到当前目录:

- `rdog_win.toml`
- `rdog_macos.toml`
- `rdog_linux.toml`

默认不会覆盖已经存在的同名文件。

如果你就是想强制重写,再加 `--force`:

```bash
rdog config init --force
```

这个入口很适合下面两种场景:

- 你第一次用 `daemon`,不想手抄配置字段。
- 你想拿到和当前代码、当前文档一致的 3 份平台模版,再按现场改。

## 3. `listen` 用法

命令形式:

```bash
rdog listen [OPTIONS] [HOST]...
```

最常见的监听:

```bash
rdog listen 55600
```

这等价于:

```bash
rdog listen 0.0.0.0 55600
```

如果你只想绑定某一块网卡,就把 host 写出来:

```bash
rdog listen 127.0.0.1 55600
```

### `listen` 的几个常用选项

#### `-i`, `--interactive`

```bash
rdog listen -i 55600
```

这是交互模式。
当前实现里,这个模式只在 Unix 平台可用。
如果你在非 Unix 平台上用它,程序会直接报不支持。
它也不能和 `-l` 一起用,两者是互斥的。

#### `-l`, `--local-interactive`

```bash
rdog listen -l 55600
```

这个模式会在本地侧提供一个 `>> ` 提示符。
输入历史和补全体验来自 `rustyline`。
如果你更喜欢在本地一行一行地下命令,这个模式会比普通透传更顺手。

#### `-b`, `--block-signals`

```bash
rdog listen -b 55600
```

这个选项会尝试屏蔽 `CTRL-C` 这类退出信号。
当前实现里,它也是 Unix 专属能力。
另外,它不能和 `-l` 同时使用。

#### `-e`, `--exec <EXEC>`

```bash
rdog listen -e whoami 55600
```

这里有个很容易误会的地方:

- `-e` 不是在本地执行命令。
- 当前行为是在连接建立后,先向对端发送一行命令。

所以它更像"连接后先帮你敲一条命令",而不是"本地收到连接就执行某个程序"。

### `listen` 常见组合

```bash
# 一边监听,一边用本地交互提示符操作
rdog listen -l 55600

# 监听后进入交互模式
rdog listen -i 55600

# 监听后先给对端发一条命令
rdog listen -e 'uname -a' 55600
```

## 4. `connect` 用法

命令形式:

```bash
rdog connect --shell <SHELL> [--mode <MODE>] [HOST]...
```

这个命令会主动连到目标主机,然后把你指定的 shell 绑到那条连接上。

这句话非常关键:

- `connect` 暴露的是"你当前这台机器上的本地 shell"。
- 它不是一个"交互式 TCP 客户端"。
- 所以如果远端已经是 `daemon inbound` 暴露出来的 bind shell,不要再用 `rdog connect -s ...` 去接,否则会变成"shell 连 shell",人的终端并没有被挂进去。

### `connect` 的 4 种模式

`connect` 现在支持 4 种 I/O 模式:

- `interactive`: 默认模式。面向人类交互。
- `stdio`: 非交互原始字节流。更适合普通程序桥接。
- `agent`: 面向智能体的 JSON 命令模式。
- `control`: 显式 `@...` 行级控制协议接收模式。

如果你是人直接连进去敲命令,用默认的 `interactive`。
如果你是程序或智能体要稳定控制,优先考虑 `stdio` 或 `agent`。

Unix 下最常见的是:

```bash
rdog connect -s /bin/bash 192.168.1.10 55600
```

也可以用 `sh`:

```bash
rdog connect -s /bin/sh 192.168.1.10 55600
```

Windows 下可以这样写:

```bash
rdog connect -s cmd.exe 192.168.1.10 55600
```

或者:

```bash
rdog connect -s powershell.exe 192.168.1.10 55600
```

程序桥接场景可以显式指定 `stdio`:

```bash
rdog connect --mode stdio -s /bin/sh 192.168.1.10 55600
```

如果是给编程智能体控制,建议直接用 `agent`:

```bash
rdog connect --mode agent -s /bin/sh 192.168.1.10 55600
```

如果你要让远端把 `@key` / `@paste` / `@script` 控制命令发到当前主机执行,用:

```bash
rdog connect --mode control -s /bin/sh 192.168.1.10 55600
```

### `connect` 的注意点

- `--shell` 是必填项。
- `--mode` 不传时默认是 `interactive`。
- 实际使用时,请明确写出 `host port` 两段。
- 这是一次性会话命令,不是自动重试命令。
- 它的职责是"主动把本地 shell 推给远端控制端",不是"作为 bind shell 的交互客户端"。
- `control` 是显式控制 lane。
  它不是“默认在 interactive 会话里偷偷拦截 `@...`”。

如果你要连的是 `daemon inbound` 暴露出来的 shell,请直接用普通 TCP 客户端,例如:

```bash
nc 127.0.0.1 5555
```

如果你要的是"启动就连,连不上过几秒再试",那不是 `connect` 的工作,而是 `daemon` 的工作。

### `interactive` / `stdio` / `agent` 的区别

#### `interactive`

- 目标用户是人。
- Unix / macOS 下会走 PTY。
- 有 prompt、回显、终端控制语义。
- 适合 bash、zsh 这类真正交互式 shell。

#### `stdio`

- 目标用户是普通程序。
- 不走 PTY。
- 不加交互式 `-i` 语义。
- 输出会比 `interactive` 干净很多,但本质上仍然是“原始 shell 字节流”。
- 你仍然需要自己判断一条命令何时结束。

#### `agent`

- 目标用户是编程智能体或自动化控制器。
- 不保留一个长期交互 shell。
- 控制端一行一个 JSON 对象。
- `rdog` 每收到一个 JSON 命令对象,就执行一次并返回一个 JSON 结果对象。

### `agent` 模式协议

请求格式:

```text
{"command":"pwd"}
```

也兼容:

```text
{"cmd":"uname -a"}
```

响应格式:

```text
{"exit_code":0,"stdout":"/tmp\n","stderr":""}
```

心跳:

```text
{"type":"ping"}
```

响应:

```text
{"type":"pong"}
```

要点:

- 同一条 TCP 连接上可以连续发多条命令。
- 一行 JSON 请求对应一行 JSON 响应。
- 连接建立后可以先长时间空闲,不会因为“短时间没发首包”就被自动降级成普通交互 shell。
- 这个模式很适合智能体做“发命令 -> 收结果 -> 判断下一步”的循环。

## 5. `control` 用法

命令形式:

```bash
rdog control [HOST]...
```

这个命令不是暴露 shell。
它的职责刚好相反:

- 本地 `stdin/stdout` 作为控制通道
- 远端显式 `control` lane 作为执行端
- 本地写什么,就往远端原样发什么
- 远端回什么,就原样写回本地 stdout

最常见的用法就是:

```bash
rdog control 127.0.0.1 5555
```

如果目标是 Zenoh control plane,最短写法是:

```bash
rdog control mac.lab
```

这条命令会把单个非端口位置参数优先解释成 `--target-name mac.lab`,并推断走 Zenoh。
旧 TCP 写法仍然保留:

- `rdog control 5555`: TCP 端口简写,host 补成 `0.0.0.0`
- `rdog control 127.0.0.1 5555`: TCP host/port
- `rdog control --transport tcp 127.0.0.1 5555`: 显式 TCP

如果你想把当前仓库里的 `mac.lab` 本机链路整条烟测一遍,现在有一个固定脚本入口:

```bash
./scripts/mac_lab_live_smoke.sh
```

这个脚本会按下面的顺序做:

- 默认先 `cargo build --quiet`
- 先探测当前是否已经有可用的 `mac.lab` daemon
- 如果已有实例可用,直接复用,不会停它
- 如果当前不可用,临时用 `rdog_macos.toml` 拉起一个本地 daemon
- 跑 4 组固定检查:
  - `@ping`
  - 裸 shell 行 `printf MACLAB_LITERAL_SHELL_OK`
  - `rdog control mac.lab --pty -- ...`
  - 真实 TTY 下 `@ping` 是否显示成 `pong`
- 如果 daemon 是脚本自己拉起的,退出时再自动清理

几个调试开关:

- `RDOG_SKIP_BUILD=1`: 跳过脚本内的 `cargo build`
- `RDOG_KEEP_SMOKE_TMP=1`: 保留临时目录和 daemon 日志
- `RDOG_BINARY=/abs/path/to/rdog`: 覆盖默认 `target/debug/rdog`
- `RDOG_CONFIG=/abs/path/to/rdog_macos.toml`: 覆盖默认配置路径

### `control` 和 `connect` 的根本区别

- `connect`: 把本机 shell 暴露给远端
- `control`: 把本地 stdio 变成“远端 control lane 的文本控制桥”

一句话记忆:

- 要暴露 shell,用 `connect`
- 要控制 daemon inbound,用 `control`

### `control` 的协议

首版控制协议是**整行文本规则**:

- 一整行以 `@` 开头 -> 解释成控制指令
- `@@...` -> 转义,表示把字面 `@...` 发给 shell
- 其他文本 -> 走普通 shell 命令执行路径

首版内建控制指令:

- `@key:"F11"`
- `@key:"ctrl+v"`
- `@key:"right-control"`
- `@key:"right-control+right"`
- `@key#7:{key:"right-control",hold_ms:200,mode:"press_release"}`
- `@paste:"hello"`
- `@script:"printf READY"`
- `@cmd:"printf READY"`
- `@script#42:"printf READY"`
- `@cmd#42:"printf READY"`
- `@pty:"codex"`
- `@pty:{cmd:"codex",args:[],cols:80,rows:24}`
- `@pty-close:{session_id:"..."}`
- `@pty-detach:{session_id:"..."}`
- `@pty-attach:{session_id:"..."}`
- `@key#7:"right-control"`
- `@ping`

如果你在 macOS 上,当前还额外支持:

- `@key:"right-option"`
- `@key:"right-command"`

如果你要看完整协议规则,包括 request id、`@cmd#id` 和裸 shell 行共存边界,请先读:

- `specs/control-line-protocol.md`

返回格式现在仍然是明确的请求/结果风格,但已经不再强制“永远只有一条 `@response`”:

- 大多数请求会以一条最终 `@response ...` 收口
- 文件型结果现在可能先发 `@savefile ...`,再发最终 `@response ...`
- `rdog control` 会继续保持连接,不会因为单条响应就退出
- `@response` 后面承载的是 JSON 风格值:
  - 数字: `@response 0`
  - 字符串: `@response "READY"`
  - 复杂结果: `@response {"exit_code":1,"stdout":"","stderr":"..."}`
- `@savefile` 当前承载的是“请接收端直接保存成文件”的结果:
  - 当前 `rdog control` 会把它保存到 `./rdog_downloads/`
  - 然后在终端输出一条本地提示 `saved file: ...`
- 显式 `@` 请求还支持可选 request id:
  - `@key#7:"right-option"`
  - `@key#7:{key:"right-option",hold_ms:200,mode:"press_release"}`
  - `@script#42:"printf READY"`
  - `@cmd#42:"printf READY"`
  - `@savefile#7:{filename:"shot.jpg",mime:"image/jpeg",encoding:"base64",data:"..."}`
  - `@screenshot#7`
- 带 id 的成功响应会包成:
  - `@response {"id":42,"value":"READY"}`
  - `@response {"id":7,"value":0}`
- 人类直接在 TTY 里运行 `rdog control` 时,简单成功响应会做本地显示优化:
  - `@response "AGENTS.md\nCargo.toml\n"` 会显示成真实多行文本
  - `@response 0` 会显示成 `0`
  - 错误对象、复杂结果对象、带 request id 的对象仍原样显示
- pipe / redirect / 程序 stdio 场景仍保留原始 `@response ...`,方便脚本解析
- `@pty` 不走 `@response` 承载输出,而是进入 PTY frame 流:
  - `@pty-ready {"session_id":"..."}`
  - `@pty-output {"session_id":"...","encoding":"base64","data":"..."}`
  - `@pty-exit {"session_id":"...","exit_code":0,"reason":"process_exit"}`
  - `@pty-closed {"session_id":"...","reason":"force_close"}`
  - `@pty-detached {"session_id":"..."}`
  - `@pty-attached {"session_id":"..."}`
- `@pty` 当前支持两种 payload 形态:
  - 最短写法: `@pty:"codex"`
    - 语义等价于 `cmd="codex"` 且 `args=[]`
  - 人类手输带参数写法: `@pty:"codex resume 019e02de-8814-72a2-ab0c-b06263cc0fba"`
    - 会按常见 shell-style 规则切成 `cmd="codex"` 和 `args=["resume","019e02de-8814-72a2-ab0c-b06263cc0fba"]`
  - 完整写法: `@pty:{cmd:"codex",args:["--profile","fast"],cols:120,rows:40}`
- 对象写法仍是程序和智能体更适合生成的 canonical 写法
  - 如果要显式指定 `cols/rows`,继续用对象写法
- 真实 TTY 下会读取本地 winsize,进入 PTY 后用 `@pty-resize` 同步尺寸变化
- 人类入口是 `rdog control TARGET --pty -- COMMAND ...`
- PTY 期间输入流是透明的:
  - `@key` 不会被本地解析
  - `@script` 不会被本地解析
  - `~.` 不会被截获
  - `Ctrl-C` / `Ctrl-D` 默认发给远端 PTY 程序
- 常规退出方式是远端程序自己退出、`Ctrl-D` 被远端程序消费后退出,或关闭本地 control 连接
- 当另一个控制端已经知道 `@pty-ready` 返回的 session id 时,可用 `rdog control TARGET --pty-close SESSION_ID` 强制关闭活动 PTY
- 如果要保留远端 PTY 进程,可用 `rdog control TARGET --pty-detach SESSION_ID` 解绑当前控制端
- 如果要重新接管 detached PTY,可用 `rdog control TARGET --pty-attach SESSION_ID`

例如:

```text
@response 0
@response "READY"
@response {"id":42,"value":"READY"}
@savefile {"id":7,"filename":"screenshot-123-virtual-desktop.jpg","mime":"image/jpeg","encoding":"base64","data":"..."}
@savefile {"id":7,"filename":"screenshot-123-manifest.json","mime":"application/json","encoding":"base64","data":"..."}
@response {"id":7,"value":{"kind":"screenshot-bundle","layout":"composite","coordinate_space":"os-logical","image":"screenshot-123-virtual-desktop.jpg","manifest":"screenshot-123-manifest.json","display_count":2}}
```

如果控制动作本身失败,也会走同一套协议回传:

```text
@response {"code":64,"error":"首版不支持的 @key 按键: hyper"}
@response {"id":42,"code":64,"error":"首版不支持的 @key 按键: hyper"}
```

注意:

- `@cmd:"..."` 是显式协议里的 shell 请求写法,适合你想给 shell 请求也加上 request id 的场景
- 不带 `@` 的裸 shell 行仍然保留,继续按原来的顺序流模式工作
- `@script:"..."` 是**在接收端主机本地直接执行命令文本**
- 这本质上就是显式远端代码执行能力
- 只有你信任对端时,才应该启用 control lane
- `@key` / `@paste` 当前底层走的是 `enigo`
- `@savefile` 当前已经是正式控制指令,不是 transport 私有返回技巧
- `@screenshot` 当前默认行为是:
  - 所有 active displays 合成为一张 virtual desktop 截图
  - 同时返回 manifest JSON
  - `coordinate_space="os-logical"`
  - `jpeg`
  - `quality=75`
  - 结果先通过两个 `@savefile` 回 JPEG + manifest,再以最终 `@response` 的 `screenshot-bundle` 收口
- 如果只需要主显示器兼容路径,显式使用:
  - `@screenshot#7:{target:"display",display:"primary",layout:"single",format:"jpeg",quality:75}`
- 旧字符串写法 `@key:"..."` 现在默认等价于:
  - `mode="press_release"`
  - `hold_ms=200`
- `@key` 也支持对象 payload:
  - `@key#7:{key:"right-option",hold_ms:200,mode:"press_release"}`
- `@key` 的 `mode` 支持:
  - `press_release`
  - `press`
  - `release`
- 三种模式的行为分别是:
  - `press_release`: 按下后等待 `hold_ms` 毫秒,再自动松开
  - `press`: 只发送按下事件,不自动松开
  - `release`: 只发送松开事件
- `@key` 现在支持一部分 side-specific 修饰键名:
  - `left-control` / `right-control`
  - `left-shift` / `right-shift`
  - macOS 额外支持 `right-option` / `right-command`
- 这比之前依赖 macOS `osascript` 的方案更适合继续往跨平台扩
- 但不同平台是否真的能注入成功,仍然受本机权限和桌面环境限制
- Windows 上如果目标窗口比 `rdog daemon` 权限更高,`@key` / `@paste` 可能被 UIPI 拦截。
  - 这时要让 daemon 与目标窗口处于相同或更高权限级别。

### `daemon inbound` 和 `control` 的配合关系

现在 inbound 端口支持两类客户端:

- 人类客户端: `nc localhost 5555`
- 智能体客户端: `rdog control localhost 5555`

### Windows 本地 `cargo run daemon` + `cargo run control` 的注意事项

如果你在 Windows 上:

- 已经用 `cargo run daemon ...` 启动了一个长期运行中的 `rdog.exe`
- 又在同一个工作区里执行 `cargo run control ...`

那只要这期间源码发生过变化,`cargo run` 就会尝试重新生成 `target\debug\rdog.exe`。
而 Windows 不能覆盖正在运行的 exe,于是会报:

```text
failed to remove file ... target\debug\rdog.exe
拒绝访问。 (os error 5)
```

这不是 Zenoh 问题。
这是 Windows 对正在运行可执行文件的锁定行为。

更稳的做法有 2 个:

1. 直接运行已经编好的二进制,不要再走 `cargo run`
   - `.\target\debug\rdog.exe daemon ...`
   - `.\target\debug\rdog.exe control ...`
2. 如果确实还要在 daemon 运行时继续 `cargo run`,给另一边换独立目标目录
   - `$env:CARGO_TARGET_DIR='target-local-control'; cargo run control ...`

也就是说:

- 同一个端口
- 同一份 daemon 配置
- 人和智能体都能接

前提是这条 inbound endpoint 明确配置为:

```toml
[inbound]
mode = "control"
```

如果还是默认 `interactive`,那它就是普通 shell,不会自动拦截 `@...`。
- 所以程序完全可以先建连接,后面按需再发 JSON 命令或 `@` 命令。
- 如果你需要探活,直接发 `{"type":"ping"}` 或 `@ping` 就行。

## 6. `daemon` 用法

命令形式:

```bash
rdog daemon [OPTIONS]
```

当前唯一显式选项是:

```bash
rdog daemon --config ./rdog_linux.toml
```

### `daemon` 会做什么

`daemon` 是配置驱动的。
它可以同时做两件事:

- 主动连出到某个端点,失败后按间隔重试。
- 打开本地监听端口,持续接收外部连接。

也就是说,你可以只开 `outbound`,只开 `inbound`,或者两边一起开。

### 配置来源优先级

当前实现按下面这个顺序叠配置:

1. 程序内建默认值
2. TOML 文件
3. 以 `RDOG_` 开头的环境变量

再说得直白一点:

- 不传 `--config` 时,程序会按当前平台尝试读取默认文件:
  - Windows: `rdog_win.toml`
  - macOS: `rdog_macos.toml`
  - Linux: `rdog_linux.toml`
- 但前提是这个文件真的存在。不存在就跳过,不会因为默认文件缺失而报错。
- 如果你显式传了 `--config ./some.toml`,那这个文件必须存在,不存在会直接启动失败。
- 环境变量优先级最高,可以覆盖 TOML 里的值。
- 升级兼容: 程序仍接受旧的 `rcat_*.toml`、`rcat.toml` 和 `RCAT_` 环境变量作为 fallback。新部署请统一使用 `rdog_*` 文件和 `RDOG_` 环境变量。

### 一个改完后可直接运行的 Linux 配置例子

如果你不想手动复制,可以直接先执行:

```bash
rdog config init
```

然后再按需要修改对应平台那一份文件。
下面这段是一个“改完后可直接运行”的 Linux 例子,不代表 `config init` 生成后 3 份文件都会和它完全一样。

先说一个边界:

- 当前配置系统不会自动按 Windows、macOS、Linux 选不同值。
- 你需要自己把 `shell` 改成对应平台的可执行程序。

平台建议:

- Linux: `/bin/bash` 或 `/bin/sh`
- macOS: `/bin/zsh` 或 `/bin/bash`
- Windows: `powershell.exe` 或 `cmd.exe`

```toml
[daemon]
retry_seconds = 5

[outbound]
enabled = true
host = "127.0.0.1"
port = 4444
shell = "/bin/bash"
mode = "interactive"

[inbound]
enabled = true
host = "0.0.0.0"
port = 5555
shell = "/bin/bash"
mode = "interactive"
```

启动命令:

```bash
rdog daemon
```

或者显式指定文件:

```bash
rdog daemon --config ./rdog_linux.toml
```

### 环境变量覆盖

嵌套字段用双下划线 `__` 连接。

比如:

```bash
export RDOG_DAEMON__RETRY_SECONDS=2
export RDOG_OUTBOUND__PORT=5555
export RDOG_INBOUND__ENABLED=true
export RDOG_INBOUND__HOST=0.0.0.0
export RDOG_INBOUND__PORT=5555
export RDOG_INBOUND__MODE=control
```

如果你平时用 `direnv`,仓库里已经给了一个 `.envrc` 示例,可以直接按那个格式改。

Windows 自带的 `rdog_win.toml` 模版现在默认是:

- `inbound.enabled = true`
- `inbound.host = "0.0.0.0"`
- `inbound.mode = "control"`

这让它开箱即可作为 control receiver 使用。
但也意味着你启动前要先确认网络暴露范围是不是你想要的。

### `daemon` 的行为边界

这部分很重要,因为它决定你是不是选对了模式:

- `outbound` 启用后,程序会在启动时立刻尝试连接。
- 连接失败,或者这次 shell session 结束后,程序会等待 `retry_seconds` 秒,然后再试一次。
- `inbound` 启用后,程序会持续监听配置的地址和端口。
- endpoint `mode` 的通用默认值仍然是 `interactive`,但 Windows 自带 `rdog_win.toml` 模版已经把 inbound 显式改成了 `control`。
- 如果你要让它接收并执行 `@...` 控制协议,要显式改成 `control`。
- 某一次 `inbound` session 失败,不会让整个 daemon 退出。
- 至少要启用 `outbound` 或 `inbound` 其中一个。两个都关掉,程序会直接报错退出。

## 6.1 `hidden-daemon` 用法

这个入口是额外新增的 Windows 专用能力。

先记住 4 个边界:

- 它 **不会** 替换现有 `daemon`
- 它 **不是** `Windows Service`
- 它的目标是“手动启动一次后隐藏常驻”
- 它主要通过日志文件、配置文件、控制端口来运维

命令形式:

```bash
rdog hidden-daemon [OPTIONS]
```

最常见的启动:

```bash
rdog hidden-daemon
```

显式指定 Windows 配置文件:

```bash
rdog hidden-daemon --config ./rdog_win.toml
```

它仍然复用现有 daemon 配置模型。
也就是说:

- `outbound`
- `inbound`
- `retry_seconds`

这些字段照常生效。

额外新增的是:

```toml
[hidden]
log_file = "rdog_hidden.log"
```

这个日志文件路径只给 `hidden-daemon` 模式使用。
普通 `rdog daemon` 不会因为这里存在就改变行为。

## 7. 常见场景示例

### 场景 1: 只想临时开个监听

```bash
rdog listen 55600
```

适合一次性的排查、调试、临时接收连接。

### 场景 2: 只想主动弹回一条 shell

```bash
rdog connect -s /bin/bash 192.168.1.10 55600
```

这是最直接的单次反连写法。

### 场景 3: 启动后立刻连出,失败就自动重试

```toml
[daemon]
retry_seconds = 3

[outbound]
enabled = true
host = "192.168.1.10"
port = 55600
shell = "/bin/bash"

[inbound]
enabled = false
```

```bash
rdog daemon --config ./rdog_linux.toml
```

### 场景 4: 常驻打开本地端口,持续接收连接

```toml
[daemon]
retry_seconds = 5

[outbound]
enabled = false

[inbound]
enabled = true
host = "0.0.0.0"
port = 5555
shell = "/bin/bash"
```

```bash
rdog daemon --config ./rdog_linux.toml
```

另一个终端里这样接入:

```bash
nc 127.0.0.1 5555
```

这里不要使用:

```bash
rdog connect -s /bin/bash 127.0.0.1 5555
```

原因是 `connect -s` 会在客户端本机再启动一个 shell,它适合 reverse shell 场景,不适合拿来操作已经存在的 inbound bind shell。

### 场景 5: 连出和监听一起开

```toml
[daemon]
retry_seconds = 5

[outbound]
enabled = true
host = "10.0.0.10"
port = 4444
shell = "/bin/bash"

[inbound]
enabled = true
host = "0.0.0.0"
port = 5555
shell = "/bin/bash"
```

这就是当前 `daemon` 最完整的玩法。

### 场景 6: Windows 上手动启动一次后隐藏常驻

先准备好 `rdog_win.toml`,确保里面的 shell、端口和模式都是你想要的。

例如:

```toml
[daemon]
retry_seconds = 5

[hidden]
log_file = "rdog_hidden.log"

[outbound]
enabled = false

[inbound]
enabled = true
host = "0.0.0.0"
port = 5555
shell = "powershell.exe"
mode = "control"
```

启动:

```bash
rdog hidden-daemon --config ./rdog_win.toml
```

后续观察和控制:

- 看日志文件 `rdog_hidden.log`
- 用 `rdog control 127.0.0.1 5555` 连控制端
- 或直接改配置后重启隐藏常驻进程

## 8. 选型建议

如果你还在犹豫该用哪个命令,可以直接按这个思路选:

- 只做一次监听: `listen`
- 只做一次主动连接: `connect`
- 要长期运行,要自动重试,或者要同时做连出和监听: `daemon`
- 要在 Windows 上手动启动一次后隐藏常驻,又不想做 Service: `hidden-daemon`

## 9. 安全提醒

`rustdog` 可以用来暴露 bind shell 或 reverse shell。
这不是一个适合随手乱开的工具。

至少先确认这几件事:

- 你知道自己把 shell 暴露给了谁。
- 你知道当前监听地址是不是对外网开放。
- 你知道 `daemon` 模式意味着它会长期挂着,不是跑完一把就自己停。

如果环境不受控,先别开。

## 附录: Zenoh router / serial control plane

当前主路径不再是 peer/peer,而是:

- `daemon = embedded router`
- `control = client`
- `control` 默认通过自动发现加入 router
- ESP32 通过原生 `transport_serial` 接入同一 Zenoh 网络

最小示例:

```bash
rdog daemon --config ./rdog_macos.toml
rdog control mini-a.lab
```

最小配置示例:

```toml
[zenoh]
enabled = true
mode = "router"
namespace = "lab"
daemon_name = "mini-a.lab"
listen_endpoints = [
  "tcp/0.0.0.0:7447",
  "serial//dev/tty.usbserial-0001#baudrate=112500",
]
request_timeout_ms = 3000
startup_guard_window_ms = 1000
```

说明:

- `daemon_name` 仍是人工寻址名,也是当前 profile 的唯一稳定身份
- `rdog control <target-name>` 是 Zenoh control 的短入口,等价于显式写 `--transport zenoh --target-name <target-name>`
- `control` 默认通过 router scouting / autodiscovery 加入网络
- `--entry-point` 是 control 加入 router 的 fallback 入口
- daemon router 必须同时暴露至少一个 **client-reachable 非 serial endpoint** 和一个 **serial endpoint**
- canonical spec 见 `specs/zenoh-control-plane-plan.md`
- `specs/zenoh-peer-peer-lan-profile.md` 仅保留为历史参考

当前 Zenoh control-plane profile 支持:

- `@ping`
- `@cmd#id`
- bare shell lines over Zenoh
- `@key`
- `@paste`
- `@savefile`
- `@screenshot`
- `@pty` / `@pty-close` / `@pty-detach` / `@pty-attach`
- 显式错误响应

明确不支持:

- 不经 `@pty` 的传统 interactive shell over Zenoh
- 把裸 shell 行升级成带 cwd 状态保持的长期 shell

当前 Zenoh 的真实实现口径还要再补一句:

- control queryable 现在主要负责 session bootstrap
- 真正的控制请求和结果 frame 现在通过 session channel 往返:
  - `rdog/<ns>/session/<session_id>/to-daemon`
  - `rdog/<ns>/session/<session_id>/to-control`
- 当前实现已经支持:
  - session open
  - session close
  - 同一 control 进程里复用 session
  - daemon 重启后重建并继续工作
