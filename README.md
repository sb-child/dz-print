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
- 源代码里有很多未使用的垃圾，是逆向初期的残留，哪些是新的、哪些是旧的见下面的代码结构

```bash
# 打印一张图片的示例代码
cargo run --bin dzprint

# 打印 Typst 文档的示例代码
cargo run --bin dzprint_typst

# TODO: 更完善的 CLI
cargo run --bin dzcli

# 真机 USB 测试平台（需要连接德佟打印机，可传 `型号-序列号` 选择设备）
cargo run --bin test_usb
cargo run --bin test_usb -- DP27P-Y4094C023
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
  - `test_usb` `[新]` 真机 USB 测试平台：按设备名选择、HID 报告收发、命令区切帧、响应队列，覆盖查询 / 流控 / `0x80` 激活 / `0x7f` 探测
- `bitmap.rs` `[新]` 以 bit 为单位的位图行类型，供 RLE 和命令层使用
- `rle.rs` `[新]` 位图行的行程编码（RLE）：RLE4X / RLE5X / RLE6X 码表、`Run` 类型，以及按位 / 按字节两种切 run 方式
- `command/` 通讯协议
  - `mod.rs` 模块入口；机型版本 `ModelVersion`（`V0` / `V1` / `V2`）；校验码计算 `calculate_checksum`
  - `envelope.rs` `[新]` 命令载荷的封包类型：自动校验和 / 固定 `0x88` / 手动打包 / 裸字节
  - `high_command.rs` `[新]` `0x80` 的激活模式 `HighCommandMode`（`Enable` / `Disable` / `EnableAlt` / `Disabled`）
  - `req/` `[新]` 请求对象：实现 `ReqTrait`，按机型编码成一组 `Envelope`
  - `req_template/` `[新]` 每个请求的字节布局模板（deku），同一命令的不同机型写法各一个模板
  - `resp/` `[新]` 响应对象：实现 `RespTrait`，把模板解析结果按机型归一化
  - `resp_template/` `[新]` 每个响应的字节布局模板（deku），保留 `magic` 做命令头校验
  - `varint.rs` `[新]` 协议里的 4 种变长整数编解码
  - `settings.rs` `[新]` 打印设置（速度、浓度、纸张间隔类型）枚举和字符串解析
  - `status.rs` `[新]` `0x70` 打印机状态码 `PrinterStatus`（25 个线上值）
- `derive_alias.rs` `[新]` deku derive 的别名 `CmdTemplate` / `RespTemplate`
- `backend/` `[旧]` `rusb` 底层通讯实现，`lib.rs` 里已注释掉
- `image_proc/` `[旧]` 图像处理
  - `mod.rs` 位图类型和灰度 / 抖动转换
  - `cmd_parser.rs` 直接把位图拼成打印字节命令
- `info.rs`、`param.rs` `[旧]` 照抄官方 SDK 的设备信息和参数结构体
- `planner/` `[新]` 命令组装和传输层封包
  - `assembler.rs`：`Assembler`，按机型把一组 `ReqTrait` 编码成命令区，再切帧解码成 `RespTrait`
  - `usb.rs`：USB 外层 `1e <长度>` 的打包 / 解包，以及按 64 字节 HID 报告切片
- `frontend/` 新架构预留，目前是空文件（接口规划见 `TODO.md` 的「frontend / 设备线程 / transport」）
- `scheduler/` 打印任务和设置类型的占位（`Job`、`Scheduler` 都还是空的）

## TODO

- 优化流控，精确控制命令打包和发送过程
- dzcli, CLI 和 Web 界面，集成查改设置，~~打印位图和 Typst 功能~~
- handle 多设备，设备断连和故障处理
- 蓝牙！https://github.com/sb-child/dz-print/issues/3

## License / 许可证

MPL-2.0, see [LICENSE](./LICENSE)

- `src/asset` 中的文件可能是从别的地方复制过来的, 请查看[这个](src/asset/README.md)
