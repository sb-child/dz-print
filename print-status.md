# 打印状态

[返回](./README.md)

`0x70` 响应 `payload[0]` 是状态码。SDK 注释：`0x00..=0x0b` 视为可继续打印，
`0x0c` 及以上是任务取消、环境或硬件错误。

响应模板：[StatusV2V1V0Tl](../src/command/resp_template/status.rs)。

| 值 | 十进制 | Rust 枚举 | 含义 |
| -- | ------ | --------- | ---- |
| `0x00` | 0 | `Printable` | 当前可以打印 |
| `0x01` | 1 | `Printing` | 当前正在打印 |
| `0x02` | 2 | `Rotating` | 当前正在转动马达 |
| `0x0a` | 10 | `NoJob` | 当前没有打印任务 |
| `0x0b` | 11 | `PageNotReady` | 有打印任务，但页面数据还没有接收完全 |
| `0x0c` | 12 | `JobCanceled` | 当前打印任务被取消 |
| `0x14` | 20 | `EnvNotReady` | 当前不具备打印环境 |
| `0x1e` | 30 | `VolTooLow` | 打印电压太低 |
| `0x1f` | 31 | `VolTooHigh` | 打印电压太高 |
| `0x20` | 32 | `TphNotFound` | 没有检测到打印头 |
| `0x21` | 33 | `TphTooHot` | 打印头温度太高 |
| `0x22` | 34 | `CoverOpened` | 打印机盖子打开 |
| `0x23` | 35 | `NoPaper` | 未检测到纸张 |
| `0x24` | 36 | `RibbonCanOpened` | 碳带盒未锁紧 |
| `0x25` | 37 | `NoRibbon` | 未检测到碳带 |
| `0x26` | 38 | `UnmatchedRibbon` | 不匹配的碳带 |
| `0x27` | 39 | `TphTooCold` | 环境温度过低 |
| `0x28` | 40 | `UsedupRibbon` | 用完的碳带 |
| `0x29` | 41 | `UsedupRibbon2` | 用完的色带 |
| `0x2a` | 42 | `NoLabel` | 未检测到标签纸卷 |
| `0x2b` | 43 | `UnmatchedLabel` | 不匹配的标签纸卷 |
| `0x2c` | 44 | `UsedupLabel` | 用完的标签纸卷 |
| `0x2d` | 45 | `NoRibbon2` | 未检测到色带 |
| `0x2e` | 46 | `UnmatchedRibbon2` | 不匹配的色带 |
| `0x32` | 50 | `LabelCanOpened` | 标签盒未锁紧 |

其余值（`0x03..=0x09`、`0x0d..=0x13`、`0x15..=0x1d`、`0x2f..=0x31`、`0x33..=0xff`）
目前未定义。

## 响应帧

```text
1f 70 <len> <状态码:1> <PageKey:2 BE> <tail...> <校验和>
```

- `PageKey` 仅在 `payload.len() >= 5` 时有意义
- `tail` 长度随固件变化：DP27P 共 8 字节载荷，DP305P 共 28 字节
- `tail` 的字段含义未确认，只应使用 `[0]` 判错

## 实测样本

```text
正常        1f 70 08 00 01 01 00 00 35 05 10 3b
纸仓盒盖打开 1f 70 08 22 01 01 00 00 35 05 10 19
未检测到纸张 1f 70 08 23 01 01 00 00 35 05 10 18
正在走纸    1f 70 08 02 01 01 00 00 35 05 12 37
正在打印    1f 70 08 01 01 02 00 00 0f 05 11 5e

            1f 70 08 00 01 02 00 00 63 05 10 0c
            1f 70 08 00 01 01 00 00 36 05 10 3a
```
