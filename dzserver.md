# dzserver：gRPC 打印服务器

一台打印机 + 一个 gRPC 前端，负责接收图片 / Typst 文档、渲染成位图、串行打印。

服务定义在 [proto/dzsrv.proto](proto/dzsrv.proto)：

- `Print.PrintImage` / `Print.PrintTypst` 提交任务，返回 `JobHandle`；
- `Mgmt.Status` 查询状态，`Mgmt.Cancel` 取消。

打印本身的行为（状态、等待完成、卡住后的恢复）见 [print-status.md](print-status.md)；
流控见 [flow-control.md](flow-control.md)。

## 启动

```bash
cargo run --bin dzserver -- /var/lib/dzserver 127.0.0.1:50051 \
  --device DP27P-Y4094C023 --render-workers 2
```

需要 `protobuf-compiler`：`build.rs` 会用 `tonic-build` 编译 proto。

## 状态目录

第一个位置参数是持久化目录，下面两个子目录都由服务器自己创建：

- `jobs/`：每个任务的临时文件（上传的图片、解包后的 Typst 文档），启动时清理
  残留的 `jobs/*`；
- `typst-packages/`：Typst 包缓存，跨任务复用。

## 流水线

```text
gRPC 入口 → 入口队列（--queue-capacity）
          → N 个渲染 worker（--render-workers）
          → 打印队列（--print-queue-capacity）
          → 1 个打印 worker
```

- 渲染、tar 解包、Typst 包下载都在 blocking task 里，不占 tokio worker；
- 打印是串行的：同一时刻只有一个 job 占着打印锁；
- 渲染 worker 需要打印头点数才能排版，所以打印 worker 会先连一次设备、发布
  `PrinterProfile`，连不上就一直重试，任务留在入口队列里；设备断开时 profile
  会清空，打印 worker 在没有 job 时也会主动重连并重新发布，避免渲染 worker
  等 profile、打印 worker 等渲染结果互相卡死；排队期间如果换了一台打印头宽度
  不同的打印机，已经渲染好的任务会因为宽度不一致而失败，需要重新提交；
- Typst 文档里的 `<print-settings>` 逐页生效，打印 worker 只在这一页的
  gap / darkness / speed 真的变化时才重发设置。

## 任务状态

`Status.state` 依次是 `JOB_STATE_QUEUED` → `RENDERING` →
`WAITING_FOR_PRINTER` → `PRINTING`，然后进三个互斥的终态之一：
`SUCCEEDED` / `FAILED` / `CANCELLED`。终态不会再变。

成功和失败是分开的，客户端不需要再从 `printer_status` 反推：

```text
SUCCEEDED  → 纸已经打完
FAILED     → failure { stage, reason, message } 说明为什么
CANCELLED  → 被取消，不算失败
```

`printer_status` 只是设备状态的快照：等待打印机阶段会持续刷新（能看出是在等
开盖还是等装纸），打印阶段不轮询，取的是开始打印前读到的最后一个值。

失败原因的分层：

- `stage`：`RENDER`（输入问题）、`CONNECT`（找不到设备）、
  `WAIT_FOR_PRINTER`（设备环境一直不正常）、`PRINT`（发送或等页完成）；
- `reason`：机器可判的具体原因，例如 `FAILURE_REASON_PRINTER_SILENT`
  （设备不再应答 USB，任务作废）、`FAILURE_REASON_PRINTER_STATUS`
  （设备明确报错，具体码在 `printer_status`）；
- `message`：给人看的完整错误，格式不保证稳定。

`reason` 来自库里的 `Stalled.cause`，`printer_status` 来自 `Stalled.outcome`
（重置之后读回的结果），两者可能不一致：设备先沉默、reset 之后才读到
`PS_COVER_OPENED`。

终态任务在内存里保留 `--job-ttl`（默认 `1h`，`humantime` 写法），之后由 GC
回收；回收之后 `Mgmt.Status` 返回 `NOT_FOUND`。

字段含义、状态机和“失败之后要不要重试”都写在
[proto/dzsrv.proto](proto/dzsrv.proto) 里，客户端只看那份注释就够。

## 取消

`Mgmt.Cancel` 是幂等的，返回 `CancelReply { accepted, state }`：

- 接受了 → `accepted = true`，任务会走到 `JOB_STATE_CANCELLED`；
- 已经是终态或之前取消过 → `accepted = false`，`state` 说明当时的状态；
- 任务不存在 → `NOT_FOUND`。

排队、渲染、等打印机就绪时立刻生效；正在打印时页与页之间检查取消，当前这一页
会打完（协议没有“中止当前页”的命令），之后任务标记为 `CANCELLED`，并丢弃设备
连接、下一个任务重连。

## 打印机掉了怎么办

打印中开盖、缺纸、USB 拔出都会让当前 job 作废，打印 worker 丢掉设备，
下一个 job 重新连接；打印机长时间回不到可打印状态时，等
`--print-timeout` 之后作废当前 job，避免后面排队的任务被堵死。

设备重置的语义见 [print-status.md](print-status.md)。

## 参数

和 `dzprint_image` 相同的纸张 / 浓度 / 速度参数都用 `PrintArgs`，见 `--help`。
服务器自己的参数：

| 参数 | 默认 | 说明 |
| --- | --- | --- |
| `--render-workers` | CPU 数 - 1，夹在 1..=4 | 渲染 worker 数量 |
| `--queue-capacity` | 64 | 入口队列容量 |
| `--print-queue-capacity` | 8 | 打印队列容量 |
| `--max-job-bytes` | 64 MiB | 单个请求体上限，同时是 gRPC 解码上限 |
| `--max-repeats` | 100 | 请求里 `repeats`（打印份数 - 1）的上限，超过直接 `INVALID_ARGUMENT` |
| `--job-ttl` | `1h` | 终态任务保留多久 |
