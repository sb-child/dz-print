# 命令实现进度

[返回](./protocol.md)

- 实现 = “无”: 命令不存在该用法
- 实现 = “todo”: 命令存在该用法但未实现
- 实现 = “?”: 用法是否存在尚未确认

[机型列表](./protocol.md#机型区别对待)

- 兼容 = “v0”/“v1”/“v2”: 命令可以在 v0/v1/v2 机型使用
- 兼容 = “v0,v1”/“v1,v2”/“v0,v1,v2”: 命令可以在多种机型上使用
- 兼容 = “?”: 机型兼容性未确认

---

## 裸命令组 (无前缀)

| 命令号 | req实现       | resp实现 | 兼容 |
| ------ | ------------- | -------- | ---- |
| `0x0c` | `EndPageV1Tl` | 无       | v1   |

源文件(req):

- [end_page.rs](src/command/req_template/end_page.rs)

## 命令组 `0x1a` (仅`v0`机型)

| 命令号 | req实现             | resp实现 | 兼容 |
| ------ | ------------------- | -------- | ---- |
| `0x37` | `PageOffsetV0Tl`    | 无       | v0   |
| `0x38` | `StartPageSeq1V0Tl` | 无       | v0   |
| `0x39` | `StartPageSeq2V0Tl` | 无       | v0   |
| `0x3a` | `StartPageSeq3V0Tl` | 无       | v0   |

源文件(req):

- [page_offset.rs](src/command/req_template/page_offset.rs)
- [start_page_seqs_v0.rs](src/command/req_template/start_page_seqs_v0.rs)

## 命令组 `0x1b`

| 命令号 | req实现          | resp实现 | 兼容  |
| ------ | ---------------- | -------- | ----- |
| `0x40` | `StartPageV0Tl`  | 无       | v0    |
| `0x4a` | `FeedLineV1V0Tl` | 无       | v0,v1 |

> `0x4a` 在 `v2` 上仅 BLE 且 `n <= 255` 时使用，其余情况走 `0x1f 0x22`。

源文件(req):

- [start_page.rs](src/command/req_template/start_page.rs)
- [feed_line.rs](src/command/req_template/feed_line.rs)

## 命令组 `0x1d` (仅`v0`机型)

| 命令号 | req实现         | resp实现 | 兼容 |
| ------ | --------------- | -------- | ---- |
| `0x56` | `EndPageV0Tl`   | 无       | v0   |
| `0x76` | `PrintLineV0Tl` | 无       | v0   |

源文件(req):

- [end_page.rs](src/command/req_template/end_page.rs)
- [print_line.rs](src/command/req_template/print_line.rs)

## 命令组 `0x1f`

| 命令号 | req实现                              | resp实现 | 兼容     |
| ------ | ------------------------------------ | -------- | -------- |
| `0x20` | `StartPageV2Tl`                      | 无       | v2       |
| `0x21` | `PrintLineV2Tl`                      | 无       | v2       |
| `0x22` | `FeedLineV2Tl`                       | 无       | v2       |
| `0x25` | `HeatControlV2V1V0Tl`                | 无       | v0,v1,v2 |
| `0x26` | `TotalLinesV2Tl`<br>`TotalLinesV1Tl` | 无       | v1,v2    |
| `0x27` | `PageWidthV2V1Tl`                    | 无       | v1,v2    |
| `0x28` | `EndPageV2Tl`                        | 无       | v2       |
| `0x29` | `PrintLineRleByteV2Tl`               | 无       | v2       |
| `0x2a` | `PrintLineV1Tl`                      | 无       | v1       |
| `0x2b` | `PrintLineV2V1Tl`                    | 无       | v1,v2    |
| `0x2c` | `PrintLineRle5xV2Tl`                 | 无       | v2       |
| `0x2d` | `PrintLineRle5dV2Tl`                 | 无       | v2       |
| `0x2e` | `RepeatLineV2V1Tl`                   | 无       | v1,v2    |
| `0x3a` | `PrintLineRle4xV2Tl`                 | 无       | v2       |
| `0x3b` | `PrintLineRle4dV2Tl`                 | 无       | v2       |
| `0x3c` | `PrintLineRle6xV2Tl`                 | 无       | v2       |
| `0x3d` | `PrintLineRle6dV2Tl`                 | 无       | v2       |
| `0x42` | `GapTypeV0V1V2Tl`                    | `GapTypeV0V1V2Tl` | v0,v1,v2 |
| `0x43` | `DarknessV2V1V0Tl`                   | `DarknessV2V1V0Tl` | v0,v1,v2 |
| `0x44` | `SpeedV2V1V0Tl`                      | `SpeedV2V1V0Tl` | v0,v1,v2 |
| `0x45` | `GapLengthV2Tl`                      | `GapLengthV2Tl` | v2       |
| `0x47` | todo                                 | 无       | ?        |
| `0x48` | todo                                 | todo     | ?        |
| `0x49` | todo                                 | ?        | ?        |
| `0x4d` | todo                                 | ?        | ?        |
| `0x52` | todo                                 | todo     | ?        |
| `0x57` | todo                                 | todo     | ?        |
| `0x59` | todo                                 | todo     | ?        |
| `0x66` | todo                                 | todo     | ?        |
| `0x70` | `StatusV2V1V0Tl`                     | `StatusV2V1V0Tl` | ?        |
| `0x71` | `DpiV2V1V0Tl`                        | `DpiV2V1V0Tl` | ?        |
| `0x72` | `PrintWidthV2V1V0Tl`                 | `PrintWidthV2V1V0Tl` | ?        |
| `0x73` | `StatisticsV2V1V0Tl`                 | `StatisticsV2V1V0Tl` | ?        |
| `0x75` | `ManufacturerV2V1V0Tl`               | `ManufacturerV2V1V0Tl` | ?        |
| `0x77` | `BufferSizeV2V1V0Tl`                 | `BufferSizeV2V1V0Tl` | ?        |
| `0x78` | todo                                 | todo     | ?        |
| `0x79` | `DeviceNameV2V1V0Tl`                 | `DeviceNameV2V1V0Tl` | ?        |
| `0x7a` | todo                                 | todo     | ?        |
| `0x7b` | 无                                   | `BufferSize2V2V1V0Tl` | ?        |
| `0x7c` | `SoftwareVersionV2V1V0Tl`            | `SoftwareVersionV2V1V0Tl` | ?        |
| `0x7d` | `BluetoothMacV2V1V0Tl`               | `BluetoothMacV2V1V0Tl` | ?        |
| `0x7e` | todo                                 | ?        | ?        |
| `0x7f` | todo                                 | ?        | ?        |
| `0x80` | todo                                 | todo     | ?        |
| `0x84` | todo                                 | todo     | ?        |
| `0x88` | todo                                 | todo     | ?        |
| `0x9e` | todo                                 | 无       | ?        |

> `0x70` / `0x71` / `0x72` 已在 DP27P / DP305P 上实测通过，但机型兼容性还没系统验证，先按 `?` 记。
> `0x45` 协议里写的是 v2 支持、v1/v0 也许支持，表里按已确认的 v2 记。
> `0x7b` 只有固件主动上报，SDK 里没有构造请求的代码，req 列按「无」记。
> 标 `(X)` 的保留命令、以及 0x81~0x9f 中没有语义说明的空槽位没有收录。

源文件(req):

- [start_page.rs](src/command/req_template/start_page.rs)
- [print_line.rs](src/command/req_template/print_line.rs)
- [feed_line.rs](src/command/req_template/feed_line.rs)
- [heat_control.rs](src/command/req_template/heat_control.rs)
- [total_lines.rs](src/command/req_template/total_lines.rs)
- [page_width.rs](src/command/req_template/page_width.rs)
- [dpi.rs](src/command/req_template/dpi.rs)
- [print_width.rs](src/command/req_template/print_width.rs)
- [manufacturer.rs](src/command/req_template/manufacturer.rs)
- [device_name.rs](src/command/req_template/device_name.rs)
- [software_version.rs](src/command/req_template/software_version.rs)
- [bluetooth_mac.rs](src/command/req_template/bluetooth_mac.rs)
- [statistics.rs](src/command/req_template/statistics.rs)
- [buffer_size.rs](src/command/req_template/buffer_size.rs)
- [end_page.rs](src/command/req_template/end_page.rs)
- [print_line_rle.rs](src/command/req_template/print_line_rle.rs)
- [repeat_line.rs](src/command/req_template/repeat_line.rs)
- [gap_type.rs](src/command/req_template/gap_type.rs)
- [darkness.rs](src/command/req_template/darkness.rs)
- [speed.rs](src/command/req_template/speed.rs)
- [gap_length.rs](src/command/req_template/gap_length.rs)
- [status.rs](src/command/req_template/status.rs)
