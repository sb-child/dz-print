# dz-print

Third-party USB/Bluetooth Driver for Detonger(detonger.com) Printing Machines

Motherboard produced by DothanTech(dothantech.com)

德佟印立方打印机的第三方USB/蓝牙驱动，主板由道臻信息技术有限公司生产

## Communication / 通讯方式

- [x] USB
- [ ] 蓝牙 (TODO)

## Tested On / 已测试

- [x] DP27P
- [ ] ... (help wanted)

## Notice / 说明

由于 SDK 代码并不开源，本驱动的底层开发完全基于对官方 SDK 的逆向、抓包、说明文档和上机测试。

> 官方 SDK 简直太烂了... 错别字先不提, 代码里不知道写了多少封装类, 然后 PC 版重要的功能也 todo 了, Android 版倒是 2024 年还有更新...

## Protocol / 协议

- [点击查看](protocol.md)当前分析出的打印机协议和命令
- [点击查看](print-status.md)当前已知的打印机状态类型

或者直接看[源码](src/command/mod.rs)

## Development and Usage / 开发和使用

- 依赖 `dbus-devel`，记得安装
- 记得设置并重载 udev 规则，类似 `SUBSYSTEM=="usb", ATTRS{idVendor}=="3533", ATTRS{idProduct}=="5c15", MODE="0666"`
- 源代码里有很多未使用的垃圾，是逆向初期的残留，请参考示例以避免用错

```bash
# 打印一张图片的示例代码
cargo run --bin dzprint

# 打印 Typst 文档的示例代码
cargo run --bin dzprint_typst

# TODO: 更完善的 CLI
cargo run --bin dzcli
```
## Code Layout / 代码结构

`src/`
- `asset/` 资源文件，目前被示例代码使用
- `backend/` 底层通讯实现
- `bin/` 可执行的示例代码
  - 看上面
- `command/` 通讯协议
  - `checksum.rs` 校验码计算
  - `mod.rs` 命令列表和单命令编解码
  - `packager.rs` 命令打包
  - `variable_bytes.rs` 某种妙妙编解码
- `image_proc/` 图像处理相关
  - `mod.rs` 位图类型和转换
  - `cmd_parser.rs` 打印命令生成

## TODO

- 优化流控，精确控制命令打包和发送过程
- dzcli, CLI 和 Web 界面，集成查改设置，打印位图和 Typst 功能
- handle 多设备，设备断连和故障处理
- 蓝牙！

## License / 许可证

MPL-2.0, see [LICENSE](./LICENSE)

- `src/asset` 中的文件可能是从别的地方复制过来的, 请查看[这个](src/asset/README.md)
