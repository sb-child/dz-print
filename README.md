# dz-print

德佟印立方（Detonger / dothantech）标签打印机的第三方 USB / 蓝牙驱动。

官方 SDK 不开源，这里的协议来自逆向、抓包和真机测试。

## 状态

- [x] USB
- [ ] 蓝牙：协议已确认，后端未实现，见 [issue #3](https://github.com/sb-child/dz-print/issues/3)
- 已测试：DP27P，其它机型欢迎反馈

## 文档

协议和实现细节都在文档里，README 只留怎么跑：

- [protocol.md](protocol.md)：协议和命令
- [flow-control.md](flow-control.md)：缓冲区感知的分块发送（`0x77` 预算）
- [print-status.md](print-status.md)：状态码、等待打印完成、卡住之后怎么恢复
- [rle.md](rle.md)：RLE 编码和真机实测结论
- [dzserver.md](dzserver.md)：打印服务器的目录、流水线、任务状态
- [cmd-impl-status.md](cmd-impl-status.md)：命令实现进度
- [TODO.md](TODO.md)：开发进度和已知限制

每个 bin 的行为细节写在它自己的模块注释里（源码文件开头），`--help` 只有参数；
库的分层和最小可运行示例在 `src/lib.rs` 的模块注释，`cargo doc --no-deps --open` 可读。

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
  --device DP27P-Y4094C023 --gap-type gap --gap-length 2.5 --darkness normal --speed max
```

行数据默认不压缩；`--rle` 只建议给简单线稿（原因见 [rle.md](rle.md)）。
超时参数和卡住之后的行为见 [print-status.md](print-status.md)。

### 打印 Typst 文档：`dzprint_typst`

```bash
cargo run --bin dzprint_typst -- /tmp/doc.typ --font /usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc
```

文档里可以用 `<print-settings>` 逐页覆盖纸张 / 浓度 / 速度等参数，字段见该 bin 的模块注释。

### 打印服务器：`dzserver`

```bash
# 需要 protobuf-compiler（build.rs 会编译 proto/dzsrv.proto）
cargo run --bin dzserver -- /var/lib/dzserver 127.0.0.1:50051 --device DP27P-Y4094C023
```

接口、目录布局、流水线和参数见 [dzserver.md](dzserver.md)。

### 真机测试：`test_usb`

```bash
cargo run --bin test_usb                                        # 设备信息
cargo run --bin test_usb -- --phase print --rows 20000          # 真打印
cargo run --bin test_usb -- --phase short-print --short-rows 32 --short-pages 3
```

`print` / `short-print` 会真的驱动打印头并走纸，阶段说明见该 bin 的模块注释。

### 卡住恢复实验：`probe_end_page`

真机验证「打印机沉默后还发 `EndPage`」的后果（由 feature `bin-probe` 控制，
默认开启）：

```bash
cargo run --bin probe_end_page -- --mode late
cargo run --bin probe_end_page -- --mode late-skip
```

结论已经落地到库里（沉默时不再发 `EndPage`），这个 bin 留作复现和回归验证；
跑法、`RESULT` 字段含义和注意事项见该 bin 的模块注释。

### `dzcli`

只有 `clap` 参数骨架，功能还没接。

## 开发

- 公开模块的 `//!` 文档都带 `# 用法` 示例：`cargo doc --no-deps --open` 查看，
  `cargo test --doc` 跑其中的示例
- 每个 bin 对应一个 feature，默认 `bins` 全部开启；只要其中一个时关掉默认特性：

  ```bash
  cargo build --no-default-features --features bin-image
  cargo build --no-default-features --features bin-typst
  cargo build --no-default-features            # 只要库
  ```

  feature 有 `bin-dzcli` / `bin-image` / `bin-typst` / `bin-test-usb` / `bin-probe` / `bin-dzserver`，
  `bins` 是它们的合集。`dzprint_typst` 和 `dzserver` 共用 `typst-support`，
  只开 `bin-image` 时不会编译 Typst 那一套依赖。
- 测试：`cargo test --all-targets`，`cargo clippy --all-targets`，`cargo fmt -- --check`
- 所有 bin 的日志走 `RUST_LOG`（`EnvFilter`，默认 `info`），排障时用 `RUST_LOG=debug`

## 发版

发版由 `.github/workflows/release.yml` 完成，只在推送 `v*` tag 时触发。

1. 改 `Cargo.toml` 的 `version`，例如 `0.2.0`。
2. 跑 `cargo test --all-targets`；Cargo 会同时同步 `Cargo.lock` 里的本地包版本。
3. 提交 `Cargo.toml` 和 `Cargo.lock`，先推送 `master`。
4. 确认仓库的 Actions 页面已经出现 `Release` workflow 后，再打 tag 并推送：

   ```bash
   git tag v0.2.0
   git push origin v0.2.0
   ```

不要把分支和 tag 放在同一次推送里：tag 推送时如果默认分支上还没有这个
workflow，GitHub 不会为这个 tag 启动它。

workflow 会先校验 tag 与 `Cargo.toml` 的版本一致，然后在 Ubuntu 22.04 上跑测试、
构建 Linux x86_64 的 `dzserver`、`dzprint_image`、`dzprint_typst`，把
`proto/dzsrv.proto` 和 `LICENSE` 一起打进压缩包，同时上传 `.sha256`，最后创建
release 并自动生成 release notes。

发布包不包含 `dzcli`、`test_usb`、`probe_end_page`；这三个仍然只能从源码构建。
tag 与版本不一致时 workflow 会直接失败，不会发版。
