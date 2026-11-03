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
  `PrinterProfile`，连不上就一直重试，任务留在入口队列里；
- Typst 文档里的 `<print-settings>` 逐页生效，打印 worker 只在这一页的
  gap / darkness / speed 真的变化时才重发设置。

## 任务状态

`Status` 依次是 `IN_QUEUE` → `RENDERING` → `PRINT_PENDING` → `PRINTING` →
`PRINT_COMPLETED`；失败 / 取消是 `RENDER_FAILED` / `CANCELLED`。

终态任务在内存里保留 `--job-ttl`（默认 `1h`，`humantime` 写法），之后由 GC
回收。

`PRINT_COMPLETED` 只表示“这个 job 结束了”，是否真的印出来了要看
`printer_status`：proto 没有独立的“打印失败”终态，失败时仍然是
`PRINT_COMPLETED` + 具体错误码（例如 `PS_COVER_OPENED`），USB 层错误是
`PS_IO_ERROR`。

## 取消

`Mgmt.Cancel` 是幂等的；对已经到终态的任务返回 `cancelled=false`。

- 排队、渲染、等打印机就绪：立刻生效；
- 正在打印：页与页之间检查取消，当前这一页会打完（协议没有“中止当前页”的
  命令），之后任务标记为 `CANCELLED`，并丢弃设备连接、下一个任务重连。

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
