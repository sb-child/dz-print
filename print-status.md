# 打印状态

[返回](./README.md)

`0x70` 响应 `payload[0]` 是状态码。SDK 注释：`0x00..=0x0b` 视为可继续打印，
`0x0c` 及以上是任务取消、环境或硬件错误。

响应模板：[StatusV2V1V0Tl](../src/command/resp_template/status.rs)。

| 值     | 十进制 | Rust 枚举          | 含义                                 |
| ------ | ------ | ------------------ | ------------------------------------ |
| `0x00` | 0      | `Printable`        | 当前可以打印                         |
| `0x01` | 1      | `Printing`         | 当前正在打印                         |
| `0x02` | 2      | `Rotating`         | 当前正在转动马达                     |
| `0x0a` | 10     | `NoJob`            | 当前没有打印任务                     |
| `0x0b` | 11     | `PageNotReady`     | 有打印任务，但页面数据还没有接收完全 |
| `0x0c` | 12     | `JobCanceled`      | 当前打印任务被取消                   |
| `0x14` | 20     | `EnvNotReady`      | 当前不具备打印环境                   |
| `0x1e` | 30     | `VolTooLow`        | 打印电压太低                         |
| `0x1f` | 31     | `VolTooHigh`       | 打印电压太高                         |
| `0x20` | 32     | `TphNotFound`      | 没有检测到打印头                     |
| `0x21` | 33     | `TphTooHot`        | 打印头温度太高                       |
| `0x22` | 34     | `CoverOpened`      | 打印机盖子打开                       |
| `0x23` | 35     | `NoPaper`          | 未检测到纸张                         |
| `0x24` | 36     | `RibbonCanOpened`  | 碳带盒未锁紧                         |
| `0x25` | 37     | `NoRibbon`         | 未检测到碳带                         |
| `0x26` | 38     | `UnmatchedRibbon`  | 不匹配的碳带                         |
| `0x27` | 39     | `TphTooCold`       | 环境温度过低                         |
| `0x28` | 40     | `UsedupRibbon`     | 用完的碳带                           |
| `0x29` | 41     | `UsedupRibbon2`    | 用完的色带                           |
| `0x2a` | 42     | `NoLabel`          | 未检测到标签纸卷                     |
| `0x2b` | 43     | `UnmatchedLabel`   | 不匹配的标签纸卷                     |
| `0x2c` | 44     | `UsedupLabel`      | 用完的标签纸卷                       |
| `0x2d` | 45     | `NoRibbon2`        | 未检测到色带                         |
| `0x2e` | 46     | `UnmatchedRibbon2` | 不匹配的色带                         |
| `0x32` | 50     | `LabelCanOpened`   | 标签盒未锁紧                         |

其余值（`0x03..=0x09`、`0x0d..=0x13`、`0x15..=0x1d`、`0x2f..=0x31`、`0x33..=0xff`）
不再当作解析错误，而是保留为 `PrinterStatus::Unknown(raw)`：

- `raw <= 0x0b` 仍然按“可继续打印”处理；
- `raw >= 0x0c` 仍然按错误处理。

这样新固件的错误码不会让 `0x70` 响应解析失败、卡住响应队列。

## 响应帧

```text
1f 70 <len> <状态码:1> <PageKey:2 BE> <tail...> <校验和>
```

- `PageKey` 仅在 `payload.len() >= 5` 时有意义
- `tail` 长度随固件变化：DP27P 共 8 字节载荷，DP305P 共 28 字节
- `tail` 的字段含义未确认，只应使用 `[0]` 判错

## 实测样本

```text
正常         1f 70 08 00 01 01 00 00 35 05 10 3b
纸仓盒盖打开 1f 70 08 22 01 01 00 00 35 05 10 19
未检测到纸张 1f 70 08 23 01 01 00 00 35 05 10 18
正在走纸     1f 70 08 02 01 01 00 00 35 05 12 37
正在打印     1f 70 08 01 01 02 00 00 0f 05 11 5e

             1f 70 08 00 01 02 00 00 63 05 10 0c
             1f 70 08 00 01 01 00 00 36 05 10 3a
```

## 等待打印完成

`EndPage`（`0x28`）只是把这一页的数据交给固件，写入返回时电机可能还在走。
主机如果立即退出并释放 USB 句柄，纸可能只走一半。

可靠的做法是在**最后一个 `EndPage` 之后**轮询 `0x70`：

- `Printing`（`0x01`）/ `Rotating`（`0x02`）/ `PageNotReady`（`0x0b`）视为还在忙
- 看到忙之后，连续两次 `Printable`（`0x00`）或 `NoJob`（`0x0a`）认为已经完成
- `0x0c` 及以上是错误状态，直接中止
- 如果一直没看到忙，**不能直接当完成**：固件从 `EndPage` 到电机真正启动
  可能超过 1 秒（DP27P 实测见过 1.4 秒）。`print_start_grace`（默认 1 秒）
  之后读一次 `0x73`：
  - `PrintPages` / `PrintLines` 有增长：页面已经打印，只是太快没采样到忙
  - 仍然没有增长：继续等，直到 `print_start_timeout`（默认 3 秒）返回
    `FrontendError::PrintDidNotStart`
- `0x73` 查询失败或 `print_verify = false` 时退回旧行为：过了宽限期就接受空闲

DP27P 回一条 `0x70` 大约要 100ms，所以短于一两个采样周期的错误状态可能看不到。
瞬态错误只能靠页末的 `0x73` 增量兜底；`0x73` 的四个计数器语义不完全可靠，
这里只使用“零增量 / 有增量”这个方向，不拿具体数值对账。

不要在逐行发送数据的过程中查 `0x70`，数据流中断会在热敏纸上留下白带；
页末查询不受影响。实现见 `frontend::Device::wait_for_print_done`。

打印完成后进程退出前还要调用 `Device::shutdown(timeout)`：它取消 worker 并等
阻塞线程真正返回，USB 句柄释放后进程才结束。只 drop `Device` 不会等线程退出。

## 打印机沉默：job 作废，reset 是唯一清理方式

开盖、缺纸或固件卡住时，打印机可能直接停止消费 USB 数据。此时它的 parser
被前面没读完的命令堵住，再发多少 `0x70` 也只会排在队尾，**不会得到响应**。
主机只能靠连续超时判定：

- `print_silent_limit` 默认 `2`；`--recv-timeout 500ms` 时约 1 秒，
  默认 3 秒时约 6 秒；
- 判定沉默后当前 job 直接作废，不要补 `EndPage`、也不要重试当前页；
- 如果 `0x77` 已经超时、下一轮 `0x70` 也不回，发送循环调用一次
  [`Device::reset`](src/frontend/mod.rs)；
- `Device::reset` 的语义是**彻底重置连接**：worker 丢弃当前 transport
  （释放接口、把内核驱动接回去），再用原来的选择器重新 open 一个新的
  libusb context / DeviceHandle，claim 之前做 `handle.reset()`。它不等价于
  在旧 handle 上调用 `libusb_reset_device()`，实测后者救不回被堵住的 parser；
- reset 完成后只读一次 `0x70`：读到 `CoverOpened` / `NoPaper` 就报具体
  状态，否则保留 `PrinterSilent`。job 仍然作废；
- `dzserver` 失败后走 `drop_device()` + 下一次任务重连，重连本身也是重新
  open；`wait_ready` 有 `print_done_timeout` 上限，不会无限堵住队列。
