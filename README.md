# dz-print

德佟印立方（Detonger / dothantech）标签打印机的第三方 USB / 蓝牙驱动。

官方 SDK 不开源，这里的协议来自逆向、抓包和真机测试。

## 状态

- [x] USB
- [ ] 蓝牙：协议已确认，后端未实现，见 [issue #3](https://github.com/sb-child/dz-print/issues/3)
- 已测试：DP27P，其它机型欢迎反馈

相关文档：

- [protocol.md](protocol.md)：协议和命令
- [print-status.md](print-status.md)：打印机状态类型
- [flow-control.md](flow-control.md)：缓冲区感知的分块发送
- [rle.md](rle.md)：RLE 编码和真机实测结论
- [TODO.md](TODO.md)：开发进度和已知限制

## 环境

- 依赖 `dbus-devel`
- 需要 udev 规则，例如：

  ```udev
  SUBSYSTEM=="usb", ATTRS{idVendor}=="3533", ATTRS{idProduct}=="5c15", MODE="0666"
  ```

## 使用

### 打印图片：`dzprint_image`

```bash
cargo run --bin dzprint_image -- --image /tmp/print.png
cargo run --bin dzprint_image -- --image /tmp/print.png \
  --device DP27P-Y4094C023 --raster floyd-steinberg \
  --gap-type gap --gap-length 2.5 --darkness normal --speed max
```

- 行数据默认不压缩。RLE 在 DP27P 上打真实照片会在纸中间插入空白，只建议给简单线稿实验用。
- `--print-timeout` 是等打印结束的总超时，`--recv-timeout` 是单条响应的等待超时；
  两者都用 `humantime` 写法，例如 `600s` / `10m` / `500ms`。
- `--silent-limit` 是 `EndPage` 之后连续多少次状态查询无响应就判定打印失败，
  默认 2。它只用来容忍固件偶尔漏答，调大只会推迟判定。
- 只要卡住（读不到状态、整页超时、打印没开始），当前 job 一律作废，程序会
  重置一次连接（断开 2 秒再重连）并读回真实状态：`cover is open` 这类原因会
  直接报出来；如果读回的是“可打印”（比如盖子松了一下又合上），也会在错误
  里说明打印机已经能接下一个任务。
- `RUST_LOG=debug` 打开细节日志。

### 打印 Typst 文档：`dzprint_typst`

```bash
cargo run --bin dzprint_typst -- /tmp/doc.typ
cargo run --bin dzprint_typst -- /tmp/doc.typ --device DP27P-Y4094C023 \
  --font /usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc
```

文档里可以逐页覆盖命令行参数：

```typst
#metadata((gap-type: "adhesive", gap: 2.5, darkness: 12)) <print-settings>
```

覆盖从该页开始生效，后续页沿用；需要恢复就在后面的页里再写一次。

### 打印服务器：`dzserver`

```bash
# 需要 protobuf-compiler（build.rs 会编译 proto/dzsrv.proto）
cargo run --bin dzserver -- /var/lib/dzserver 127.0.0.1:50051 \
  --device DP27P-Y4094C023 --render-workers 2
```

- gRPC 服务定义在 [proto/dzsrv.proto](proto/dzsrv.proto)：`Print` 提交图片 / Typst tar，
  `Mgmt` 查询或取消任务。
- 第一个参数是持久化目录：`jobs/` 放每个任务的临时文件，`typst-packages/` 缓存
  Typst 包；启动时会清理残留的 `jobs/*`。
- 流水线是「入口队列 → N 个渲染 worker → 1 个打印队列 → 1 个打印 worker」。
  渲染和 tar / 包下载都在 blocking task 里，打印任务串行执行。
- Typst 文档里的 `<print-settings>` 也会逐页生效；打印 worker 只在这一页的
  gap / darkness / speed 变化时重发设置。
- 打印中开盖、缺纸、USB 拔出会作废当前 job，等打印机重新就绪后继续打印下一个；
  排队中的 job 可以用 `Mgmt.Cancel` 取消。
- 连接成功后会输出设备名、硬件版本、软件版本、DPI、纸宽和打印头点数；查询失败的
  展示字段只记 `warn!`，不影响打印。
- 常用参数：`--render-workers`、`--queue-capacity`、`--max-job-bytes`、
  `--max-repeats`、`--job-ttl`，以及和 `dzprint_image` 相同的纸张 / 浓度 / 速度参数。
  `--job-ttl` 用 `humantime` 写法，例如 `1h` / `30m`。

### 真机测试：`test_usb`

```bash
cargo run --bin test_usb
cargo run --bin test_usb -- DP27P-Y4094C023 --phase device-info
cargo run --bin test_usb -- --phase print --rle bits5x --rows 20000

# 复现短页面打完前后掉打印头：连续打 3 张 32 行的图，
# 把 0x70 轮询压到 1ms，并用 0x73 统计对账
cargo run --bin test_usb -- --phase short-print \
  --short-rows 32 --short-pages 3 --poll 1ms --print-timeout 20s
```

`short-print` 会逐页记录 `print_bitmap_job` 的返回值、`0x73` 统计增量和打印结束后
队列里的残留帧。`--poll` / `--grace` / `--start-timeout` / `--verify-interval`
直接对应 `DeviceConfig` 里等待打印完成的参数，都用 `humantime` 写法（例如
`1ms` / `250ms` / `3s`）；短页面想抓到转瞬即逝的状态时把 `--poll` 调到
`1ms`~`5ms`。注意实际采样率还受固件响应速度限制：DP27P 回一条 `0x70` 大约要
100ms，所以即使 `--poll 1ms`，真正能看到的间隔也只有 ~10Hz。`0x73` 计数器在
DP27P 上并不完全可靠，日志里的结论只能当参考，最终仍要看纸面。

`print_bitmap_job` 现在会在一直看不到 `Printing` / `Rotating` 时用 `0x73`
确认页面是否真的打印过：到 `--grace` 后统计没有增长就继续等，直到
`--start-timeout` 返回 `PrintDidNotStart`，避免固件启动慢时提前判完成。

### 卡住恢复实验：`probe_end_page`

这个 bin 用真机验证「打印机已经沉默后还发 `EndPage` 是否会让恢复变慢」。
它会真的打印并走纸。

```bash
# 主实验：同一个完整页，唯一区别是沉默后发不发 EndPage
cargo run --bin probe_end_page -- --mode late \
  --rows 1418 --prompt-at 80 --recv-timeout 500ms
cargo run --bin probe_end_page -- --mode late-skip \
  --rows 1418 --prompt-at 80 --recv-timeout 500ms

# 辅助实验：在同一个中途位置，只切换 EndPage
cargo run --bin probe_end_page -- --mode early \
  --rows 1418 --prompt-at 50 --probe-every 2000 --recv-timeout 500ms
cargo run --bin probe_end_page -- --mode early-end-page \
  --rows 1418 --prompt-at 50 --probe-every 2000 --recv-timeout 500ms
```

模式：

- `early`：探测到沉默就停，不发 `EndPage`；
- `early-end-page`：同样停，但仍然发一条 `EndPage`；
- `late`：不理会沉默，发完整页 + `0x25` + `0x73` + `EndPage`（复现失败场景）；
- `late-skip`：发完整页；`0x73` 和 `0x70` 都失败时不发 `EndPage`。

`late` / `late-skip` 建议 `--prompt-at 80`：失败抓包就是“数据快发完时开盖，
主机仍把剩下的数据和 EndPage 写进去”。`early` / `early-end-page` 用 50 左右，
在同一个中途位置只切换 EndPage。默认 `--pace 50ms` 大致对应真机的发送速度；
如果 80% 还反应不过来，把 `--rows` 调大（例如 3000）。

程序打印到 `--prompt-at` 指定的百分比时会打出 `>>> OPEN THE COVER NOW <<<`，
看到后打开盖子。结束后打印：

```text
RESULT mode=early rows=1418 sent_bytes=... sent_lines=... drop_elapsed=... prompt=... first_silence=... first_error=... endpage=... stats_ok=... answer=... from_silence=... from_endpage=...
```

`--settle` 控制断开到重连的等待（默认 `1.3s`），`--retries` / `--retry-gap`
控制重连后的探测。`--no-wait` 跳过开始前的回车确认。

`dzcli` 目前只有 `clap` 参数骨架，功能还没接。

## 开发

- 公开模块的 `//!` 文档都带 `# 用法` 示例，可以 `cargo doc --no-deps --open` 查看，
  用 `cargo test --doc` 跑其中的示例
- 每个 bin 对应一个 feature，默认 `bins` 全部开启；只要其中一个时关掉默认特性：

  ```bash
  cargo build --no-default-features --features bin-image
  cargo build --no-default-features --features bin-typst
  cargo build --no-default-features            # 只要库
  ```

  feature 有 `bin-dzcli` / `bin-image` / `bin-typst` / `bin-test-usb` / `bin-probe` / `bin-dzserver`，
  `bins` 是它们的合集。`dzprint_typst` 和 `dzserver` 共用 `typst-support`，
  只开 `bin-image` 时不会编译 Typst 那一套依赖。

## 代码结构

`src/`

- `asset/`：内置字体，供 `dzprint_typst` / `dzserver` 使用
- `bin/`
  - `common/` 各 bin 共用：clap 参数、打印流程；`image.rs` 图片解码 / 缩放 / 抖动，
    `typst.rs` Typst world / 编译 / 渲染 / 安全解包
  - `dzprint_image` 打印 jpg / png：查打印头点数、按需缩放、抖动成位图
  - `dzprint_typst` 编译 Typst 文档、逐页渲染打印，支持 `<print-settings>` 按页覆盖
  - `dzserver/` gRPC 打印服务器：`main.rs` 入口，`api.rs` 接口，`job.rs` 任务表，
    `pipeline.rs` 渲染 / 打印队列，`render.rs` 阻塞渲染，`printer.rs` 设备生命周期与重连
  - `dzcli` CLI 雏形，只有参数骨架
  - `test_usb` 真机测试平台：`--phase` / `--rle` / `--rows`
- `bitmap/` 以 bit 为单位的位图，行视图供 RLE 和命令层借用
- `rle/` 行程编码：`bit.rs`（RLE4 / RLE5 / RLE6 码表和位级 X/D）、`byte.rs`（字节级）
- `command/` 通讯协议
  - `mod.rs`：`ModelVersion`（`V0` / `V1` / `V2`）和校验码
  - `envelope.rs` 命令载荷封包：自动校验和 / 固定 `0x88` / 手动打包 / 裸字节
  - `high_command.rs` `0x80` 的激活模式 `HighCommandMode`
  - `req/` 请求对象：实现 `ReqTrait`，按机型编码成一组 `Envelope`
  - `req_template/` 每个请求的 deku 字节布局，同一命令的不同机型各一个模板
  - `resp/` 响应对象：实现 `RespTrait`，把模板解析结果按机型归一化
  - `resp_template/` 每个响应的 deku 字节布局，保留 `magic` 做命令头校验
  - `varint/` 协议里的 4 种变长整数编解码
  - `settings/` 速度 / 浓度 / 纸张间隔类型枚举和字符串解析
  - `status.rs` `0x70` 状态码 `PrinterStatus`，未定义值保留在 `Unknown(raw)`
- `derive_alias.rs` deku derive 的别名 `CmdTemplate` / `RespTemplate`
- `backend/` 传输层
  - `mod.rs`：`Transport` trait（`write` / `read_chunk` / `reset`），只搬字节
  - `usb.rs`：`UsbTransport` / `UsbSelector`，`rusb` 中断端点读写
  - `usb_frame.rs`：USB 外层 `1e <长度>` 的打包 / 解包，按 64 字节 HID 报告切片
- `raster.rs` 灰度图 / `tiny-skia` 画布 → `bitmap::Bitmap` 的抖动转换
- `planner/` 命令组装和传输层封包
  - `assembler/`：`Assembler` 按机型编码请求、解析响应
  - `flow.rs`：`FlowPlanner`，`0x77` 预算感知的分块发送计划，不做 IO
  - `print/`：位图 → 打印计划；`mod.rs` 放 `RleMode` / 分页 / `into_reqs`，`rle.rs` 放 `select_rle`
- `frontend/` 设备 frontend
  - `mod.rs`：`Device` / `DeviceConfig` / 错误类型，批量发送、按类型接收、`wait_for_print_done`、`shutdown`
  - `worker.rs`：阻塞线程里的 USB 读写和切帧，由 `mytm::TaskManager` 托管
  - `print.rs`：`print_bitmap_job`，分页 + 流控 + 页末等待打印完成
