# dz-print

[WIP] Working in progress... 正在编写, 尚不可用

Third-party USB/Bluetooth Driver for Detonger(detonger.com) Printing Machines

Motherboard produced by DothanTech(dothantech.com)

德佟印立方打印机的第三方USB/蓝牙驱动, 主板由道臻信息技术有限公司生产

## Notice / 说明

由于 SDK 代码并不开源, 本驱动的底层开发完全基于对官方 SDK 的逆向, 抓包, 说明文档和上机测试。

> 官方 SDK 简直太烂了... 错别字先不提, 代码里不知道写了多少封装类, 然后 PC 版重要的功能也 todo 了, Android 版倒是 2024 年还有更新...

## Tested On / 已测试

- [x] DP27P

## Protocol / 协议

[点击查看](protocol.md)当前分析出的打印机协议和命令

## 开发和使用 / Development and How to use

- 依赖 `dbus-devel`
- 记得设置并重载 udev 规则, 类似 `SUBSYSTEM=="usb", ATTRS{idVendor}=="3533", ATTRS{idProduct}=="5c15", MODE="0666"`

```bash
cargo run --bin dzprint
cargo run --bin dzprint_typst
cargo run --bin dzcli
```

## License / 许可证

MPL-2.0, see [LICENSE](./LICENSE)
