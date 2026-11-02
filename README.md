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
- `--print-timeout` 是等打印结束的总超时，`--recv-timeout` 是单条响应的等待超时。
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

### 真机测试：`test_usb`

```bash
cargo run --bin test_usb
cargo run --bin test_usb -- DP27P-Y4094C023 --phase device-info
cargo run --bin test_usb -- --phase print --rle bits5x --rows 20000
```

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

  feature 有 `bin-dzcli` / `bin-image` / `bin-typst` / `bin-test-usb`，`bins` 是它们的合集。
  只开 `bin-image` 时不会编译 Typst 和 `dzprint_typst` 的那套依赖。

## 代码结构

`src/`

- `asset/`：内置字体，供 `dzprint_typst` 使用
- `bin/`
  - `common/` 两个打印 bin 共用：clap 参数、发送设置、打印任务和退出顺序
  - `dzprint_image` 打印 jpg / png：查打印头点数、按需缩放、抖动成位图
  - `dzprint_typst` 编译 Typst 文档、逐页渲染打印，支持 `<print-settings>` 按页覆盖
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
