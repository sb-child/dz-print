# dz-print

Third-party USB/Bluetooth Driver for Detonger(detonger.com) Printing Machines

Motherboard produced by DothanTech(dothantech.com)

德佟印立方打印机的第三方USB/蓝牙驱动，主板由道臻信息技术有限公司生产

## Communication / 通讯方式

- [x] USB
- [ ] 蓝牙（协议已确认，后端未实现，见 [issue #3](https://github.com/sb-child/dz-print/issues/3)）

## Tested On / 已测试

- [x] DP27P
- [ ] ... (help wanted)

## Notice / 说明

由于 SDK 代码并不开源，本驱动的底层开发完全基于对官方 SDK 的逆向、抓包、说明文档和上机测试。

> 官方 SDK 简直太烂了... 错别字先不提, 代码里不知道写了多少封装类, 然后 PC 版重要的功能也 todo 了, Android 版倒是 2024 年还有更新...

## Protocol / 协议

- [点击查看](protocol.md)当前分析出的打印机协议和命令
- [点击查看](print-status.md)当前已知的打印机状态类型
- [点击查看](flow-control.md)buffer 容量感知的分块发送流控机制
- [点击查看](TODO.md)当前开发进度和 resp 层实现计划

## Development and Usage / 开发和使用

- 依赖 `dbus-devel`，记得安装
- 记得设置并重载 udev 规则，类似 `SUBSYSTEM=="usb", ATTRS{idVendor}=="3533", ATTRS{idProduct}=="5c15", MODE="0666"`
- 公开模块的 `//!` 文档都带 `# 用法` 示例（`bitmap` / `rle` / `raster` / `backend` /
  `planner` / `frontend` / `command` 等），可以 `cargo doc --no-deps --open` 直接看，
  用 `cargo test --doc` 跑其中的示例
- 源代码里有很多未使用的垃圾，是逆向初期的残留，哪些是新的、哪些是旧的见下面的代码结构

```bash
# 打印一张图片的示例代码
cargo run --bin dzprint

# 打印 Typst 文档的示例代码
cargo run --bin dzprint_typst

# TODO: 更完善的 CLI
cargo run --bin dzcli

# 真机 USB 测试平台（需要连接德佟打印机）
cargo run --bin test_usb
cargo run --bin test_usb -- DP27P-Y4094C023 --phase device-info
cargo run --bin test_usb -- --phase print --rle bits5x --rows 20000
```

## Code Layout / 代码结构

> `new-standard` 分支正在重写命令层，所以 `src/` 里新老两套代码并存：
> **`[新]`** 是重写后的实现，**`[旧]`** 是 `master` 留下来的逆向初期代码，
> 基本没有接入（有的连模块声明都被注释了），看代码时注意别用错。

`src/`

- `asset/` 字体等资源文件，供示例代码使用
- `bin/` 可执行的示例
  - `dzprint` 打印图片、`dzprint_typst` 打印 Typst 文档：`[旧]` 实现整体被注释，目前 `main` 是空的
  - `dzcli` CLI 雏形（只有 `clap` 参数骨架，功能还没接）
  - `test_usb` `[新]` 真机 USB 测试平台：clap 参数选择设备 / `--phase` / `--rle` / `--rows`，覆盖查询 / 流控 / `0x80` 激活 / `0x7f` 探测 / 打印测试
- `bitmap/` `[新]` 以 bit 为单位的位图（`mod.rs`），行视图供 RLE 和命令层使用；测试在 `tests.rs`
- `rle/` `[新]` 位图行的行程编码：`bit.rs`（RLE4 / RLE5 / RLE6 码表和位级 X/D）、`byte.rs`（字节级）、`mod.rs`（游程拆分 `runs_bits` / `runs_bytes`）
- `command/` 通讯协议
  - `mod.rs` 模块入口；机型版本 `ModelVersion`（`V0` / `V1` / `V2`）；校验码计算 `calculate_checksum`
  - `envelope.rs` `[新]` 命令载荷的封包类型：自动校验和 / 固定 `0x88` / 手动打包 / 裸字节
  - `high_command.rs` `[新]` `0x80` 的激活模式 `HighCommandMode`（`Enable` / `Disable` / `EnableAlt` / `Disabled`）
  - `req/` `[新]` 请求对象：实现 `ReqTrait`，按机型编码成一组 `Envelope`
  - `req_template/` `[新]` 每个请求的字节布局模板（deku），同一命令的不同机型写法各一个模板
  - `resp/` `[新]` 响应对象：实现 `RespTrait`，把模板解析结果按机型归一化
  - `resp_template/` `[新]` 每个响应的字节布局模板（deku），保留 `magic` 做命令头校验
  - `varint/` `[新]` 协议里的 4 种变长整数编解码（`mod.rs` + `tests.rs`）
  - `settings/` `[新]` 打印设置（速度、浓度、纸张间隔类型）枚举和字符串解析（`mod.rs` + `tests.rs`）
  - `status.rs` `[新]` `0x70` 打印机状态码 `PrinterStatus`（25 个已知线上值 + `Unknown(raw)`）。`Unknown(0x0c..)` 仍然按错误处理
- `derive_alias.rs` `[新]` deku derive 的别名 `CmdTemplate` / `RespTemplate`
- `backend/` `[新]` 传输层
  - `mod.rs`：`Transport` trait（`write` / `read_chunk` / `reset`），只搬字节，不解析命令
  - `usb.rs`：`UsbTransport` / `UsbSelector`，`rusb` 中断端点读写
  - `usb_frame.rs`：USB 外层 `1e <长度>` 的打包 / 解包，按 64 字节 HID 报告切片（原 `planner/usb.rs`）
- `raster.rs` `[新]` 图像 → 位图的抖动转换：灰度图 / `tiny-skia` 画布 → `bitmap::Bitmap`
- `planner/` `[新]` 命令组装和传输层封包
  - `assembler/`：`Assembler`，按机型把一组 `ReqTrait` 编码成命令区，再切帧解码成 `RespTrait`（`mod.rs` + `tests.rs`）
  - `flow.rs`：`FlowPlanner`，`0x77` 预算感知的分块发送计划（9000 字节分块、50 ms 间隔、超时退化），不做 IO
  - `print/`：位图 → 打印计划。`mod.rs` 放 `RleMode` / `PrintCommand` / 分页 / `into_reqs`，`rle.rs` 放 `select_rle`，测试在 `tests.rs`
- `frontend/` `[新]` 设备 frontend
  - `mod.rs`：`Device` / `DeviceConfig` / `RecvError` / `FrontendError`，批量发送、按类型接收、`recv_matching`、`wait_for_print_done`、`shutdown`
  - `worker.rs`：阻塞线程里的 USB 读写、命令区切帧、原始 payload 队列；worker 由 `mytm::TaskManager` 托管，可取消、可等待线程真正退出。三个通道都用 `crossfire`：写任务 `mpmc::bounded_async_blocking(8)`（异步发送端、阻塞接收端）、payload `spsc::unbounded_async`（阻塞发送端、异步接收端）、单条写任务 ack `crossfire::oneshot`
  - `print.rs`：`Device::print_bitmap_job`，分页 + `FlowPlanner` 流控 + 页末等待打印完成

`Device` 的生命周期：

```rust
let tm = TaskManager::new();          // 整个程序共用一个，放在最外层
let dev = Device::new(&tm, config);   // worker 挂到 tm 下面
dev.ready().await?;                   // 等 worker 打开设备（TaskState::Running）
// ... 收发 / 打印
dev.shutdown(timeout).await?;         // 取消 worker 并等阻塞线程返回
tm.close();                           // 退出前统一关闭所有任务
```

- `shutdown(timeout)` 返回后 USB 句柄才释放
- 直接 drop 只会关闭通道，worker 会在下一次读取超时后自行退出，但不等它结束
- `TaskManager::close()` 之后再 spawn 的任务会直接变成 `Cancelled`

## TODO

- 优化流控，精确控制命令打包和发送过程
- dzcli, CLI 和 Web 界面，集成查改设置，~~打印位图和 Typst 功能~~
- handle 多设备，设备断连和故障处理
- 蓝牙！https://github.com/sb-child/dz-print/issues/3

## License / 许可证

MPL-2.0, see [LICENSE](./LICENSE)

- `src/asset` 中的文件可能是从别的地方复制过来的, 请查看[这个](src/asset/README.md)
