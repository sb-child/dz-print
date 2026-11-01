# TODO

[返回](./README.md)

当前主线：resp（设备→主机响应）层实现；设备线程 / frontend 见下方「规划」。命令实现状态见 [cmd-impl-status.md](./cmd-impl-status.md)。

## 进行中

- [x] `varint.rs` 的 `DekuReader` 按 `VarintMode` 解码
  - 之前四个类型都走 Auto 的 1/2 字节逻辑：`VarFixed3` 丢 `lo`，`VarC0Be16` 把 `hi` 当低字节
  - 现在按模式决定读取长度，再复用 `VarintMode::decode`
  - 新增 `deku_reader_uses_mode` 测试；`cargo test --lib`: 77 passed
  - 顺手删了未使用的 `use std::ops::Range;`

- [x] `settings.rs` 的取值空间统一
  - `SpeedSetting` / `DarknessSetting` 显式 `Min = 0`，discriminant = `v2`/`v1` 线上值
  - `TryFrom<u8>` 改成线上值（`0..=4` / `0..=14`）
  - 新增 `user_value` / `from_user`（用户面 `1..=5` / `1..=15`）
  - 新增 `to_wire(mv)` / `from_wire(v, mv)` 处理 `v0` 的 `+1`
  - `FromStr` 继续按用户面解析；加 `PartialEq, Eq`
  - 5 个用例；`cargo test --lib`: 82 passed

- [x] P0 响应层基础设施（未提交）
  - `src/command/mod.rs` 加 `pub mod resp;`
  - `src/command/resp/mod.rs`：`RespTrait` / `ParseRespError` / `decode_resp`
  - `decode_resp` 用一个 `DekuContainerRead` 模板解析，并要求完整消费 payload；
    magic 缺失落到 `Decode`，多余字节落到 `TrailingBytes`
  - 4 个用例；`cargo test --lib`: 86 passed

- [x] P1 darkness 样板（未提交）
  - `resp_template/darkness.rs`：`_tail` 改为 `Vec<u8>` + `read_all`，3 个字节级用例
  - `resp_template/mod.rs`：新增 `test_toolkits`（`hexdec` / `eq` / `dec_tl`）
  - `resp/darkness.rs`：`Darkness { darkness }`，用 `DarknessSetting::from_wire` 统一
    `v2`/`v1`（`0..=14`）与 `v0`（`1..=15`）
  - `cmd-impl-status.md`：`0x43` resp 列改为 `DarknessV2V1V0Tl`
  - 7 个用例；`cargo test --lib`: 93 passed

- [x] P2 `0x42` / `0x44` / `0x45`
  - `resp_template/speed.rs`：`1f 44 <speed>` + `read_all` 尾部（DP305P 载荷 12 字节）
  - `resp/speed.rs`：`Speed { speed }`，走 `SpeedSetting::from_wire`
  - `resp_template/gap_type.rs`：`1f 42 <gap_type>` + `read_all` 尾部
  - `resp/gap_type.rs`：`GapType { gap_type }`，`GapTypeSetting::from_wire`（全机型一致）
  - `settings.rs`：`GapTypeSetting` 补 `PartialEq, Eq` / `TryFrom<u8>` / `to_wire` / `from_wire`
  - `resp_template/gap_length.rs`：只收原始字节，不做变长解码
  - `resp/gap_length.rs`：按 `raw.len()` 选 `Auto` / `Fixed2` / `Fixed3`，长度越界报
    `BadLength`，缺 `0xC0` 标志报 `InvalidValue`
  - `cmd-impl-status.md`：`0x42` / `0x44` / `0x45` resp 列补类型名
  - `protocol.md`：补 `0x45` 响应按长度解码的说明
  - 24 个用例；`cargo test --lib`: 117 passed

- [x] P2 req domain 补齐
  - 新建 `req/darkness.rs` / `req/speed.rs` / `req/gap_type.rs` / `req/gap_length.rs`
  - 结构与 `req::EndPage` / `req::FeedLine` 一致：选 `EnvelopeKind` → 模板 `to_bytes`
  - 设置值统一通过 `settings.rs` 的 `to_wire` 处理版本差异
  - `GapLength` 只在 `v2` 发送，`v1`/`v0` 返回 `PackReqError::UnsupportedUsage`

- [x] `status.rs` 重写
  - `PrinterErrorCode` 改为 `PrinterStatus`，按线上值映射 25 个状态码
  - 补 `to_wire` / `from_wire` / `TryFrom<u8>` / `is_printable` / `is_error`
  - 修正 `36 = RibbonCanOpened`（原 `TphOpened` 有误）
  - `print-status.md` 同步为完整状态表和实测帧
  - 3 个用例；`cargo test --lib`: 120 passed

- [x] `0x70` req / resp
  - `req_template/status.rs`：`StatusV2V1V0Tl`，空载荷（`1f 70`）
  - `req/status.rs`：`Status {}`，按机型选 `EnvelopeKind`
  - `resp_template/status.rs`：`StatusV2V1V0Tl`，`状态码:1 + PageKey:u16BE + read_all 尾部`
  - `resp/status.rs`：`Status { status, page_key, tail }`，状态码走 `PrinterStatus::from_wire`
  - DP27P 实测帧用作模板和 domain 用例
  - `cmd-impl-status.md`：`0x70` req / resp 列改为 `StatusV2V1V0Tl`
  - `protocol.md` / `print-status.md`：补 PageKey 位置和响应模板链接
  - 9 个用例；`cargo test --lib`: 129 passed

- [x] RLE 编码 + req 层
  - `rle.rs`：`RleBitStream`（4/5/6 位，X/D）和 `RleByteStream`（`0x29`），尾部白色游程不编码
  - 游程拆分用 DP 求最少符号；长游程先取最大码本值再查表
  - `planner/print.rs`：`RleMode` 非 `Off` 时编码，X/D 取小，必须比普通行小才采用，否则回退 `Line`
  - `req/print_line_rle.rs`：`PrintLineRle` + `RleKind`，只在 `v2` 发送，`repeats` 用 `0x2e` 展开；`v1` / `v0` 返回 `UnsupportedUsage`
  - 新增编码器 roundtrip 测试；`cargo test --lib`: 300 passed

- [x] Transport trait + USB 实现
  - `backend/mod.rs`：`Transport`（`write` / `read_chunk` / `reset`）、`WriteOutcome`、`ReadStatus`、`TransportError`
  - `write` 允许返回 `Blocked`，给 BLE 的 `ff03` 信用留口子，传输层不阻塞等读
  - `backend/usb.rs`：`UsbTransport` / `UsbSelector`（设备名 / VID+PID / Auto），端点发现、声明接口、复用 `planner::usb` 报告切片
  - `reset` 后等 1 秒重新枚举
  - 对象安全用例；`cargo test --lib`: 313 passed

- [x] `test_usb` 换用 `UsbTransport` + `FlowPlanner`
  - 删除自己维护的 `USBSelector` / 端点发现 / `write_cmd_area`，改用 `backend::usb::UsbTransport`
  - 新增 `TestFrontend::print_bitmap_page`：`StartPage` + `TotalLines` + 按预算分块 + `EndPage`
  - `TEST_USB_PRINT=1` 时打一张 384x64 测试图，默认跳过避免误走纸
  - `cargo check --bin test_usb` / clippy 干净

- [x] RLE 帧改成 `ManualPack`（DP27P 实测定位）
  - 现象：`RleMode::Byte` 图形右移、左侧拖影；`Off` 正常
  - 原因：`PrintLineRle` 用 `AutoPackWithChecksum`，`Assembler` 在协议字段 `<RleByteLen>` 前又补了一层 `<len>`；设备把 `RleByteLen` 当成第一个 RLE 数据字节
  - 修复：`PrintLineRle` 改用 `ManualPack`，模板保留 `<RleByteLen>` / `<符号数>`，校验和手动追加
  - 复测：字节级 `0x29` 正常；位级 `0x2c` / `0x3c` 也正常
  - `0x3a`（RLE4X）仍空白；X / D 混用仍会断层，真机用 `Bits5XOnly` / `Bits6XOnly`
  - `cargo test --lib`: 313 passed

- [x] 位级 RLE 的 X / D 混用问题（DP27P 实测）
  - `Bits4` / `Bits5`（自动 X/D）图像中间出现周期性断层；`Bits5XOnly` 打印正常
  - 位流本身自洽，例如 `1f 2c 04 ff ff c0` 解码正好是 384 个黑点
  - 结论：DP27P 不把 X 行写进 D 的“上一行”基准，X / D 不能混用；`RleMode::Bits5` 暂时不要用于真机
  - 已加 `RleMode::Bits4XOnly` / `Bits5XOnly` / `Bits6XOnly`，对应 `TEST_USB_RLE=bits4x|bits5x|bits6x`
- [x] `0x3a`（RLE4X）在 DP27P 上打不出内容
  - `bits4x` 日志确认 payload 正确：`1f 3a 06 ff ff ae 13`
    - `FF FF AE` = `1111 1111 1111 1111 1010 1110` → 88×4 + 3 + 29 = 384 点
    - 符号顺序由 DP 选出（3 后 29），总长与行宽一致
  - 命令格式和位流都没有问题，因此判断为 DP27P 固件不认 `0x3a`，或 RLE4 码本与 5 位不同
  - `0x3c`（RLE6X）随后复测正常
  - 目前可用的位级方案：`Bits5XOnly`（`0x2c`）/ `Bits6XOnly`（`0x3c`）

- [x] 长位图自动拆页
  - `0x26 TotalLines` 在 v2 上的上限是 `16383`，`print_bitmap` 只描述一页
  - 新增 `planner::print::print_bitmap_pages` / `PrintPage`：按 `MAX_LINES_PER_PAGE`
    切页，每页第一行按全白基准，D 模式不跨页引用上一页
  - `test_usb::print_bitmap_job` 逐页发 `StartPage` / `TotalLines` / 行命令 / `EndPage`，
    `page_idx` 递增，页间保持 `Continuous`；`FlowPlanner` 每页新建
  - `TEST_USB_ROWS` 可覆盖测试图行数，用来验证多页
  - 多页在 DP27P 上的纸面行为（是否连续、有无额外间距）待实测
  - `cargo test --lib`: 318 passed

- [x] 长页流控死循环（`TotalLines = 16383`）
  - 现象：日志每隔 50 ms 发一次 `1f 70 ... 1f 77 ...`，预算一直是 24500，但从不发送行数据
  - 原因：`FlowPlanner::step` 用「`budget < remaining`」判断要查询。长页的 `remaining`
    永远大于单次预算，于是拿到预算后立刻又进入查询分支
  - 修复：改成「当前预算花完（`budget == 0`）再查 `0x77`」；补回归测试
    `spends_budget_before_querying_again`
  - `flow-control.md` 的发送循环同步改成先发完当前预算

- [x] 等打印真正完成再退出
  - 现象：`EndPage` 之后程序立即结束，纸还在走，USB 句柄一释放打印就中断
  - `EndPage` 只是把数据交给固件，电机是异步驱动的
  - 新增 `TestFrontend::wait_for_print_done`：轮询 `0x70`
    - `Printing` / `Rotating` / `PageNotReady` 视为还在忙
    - 连续 2 次 `Printable` / `NoJob` 才认为完成
    - 短任务用 1 秒启动宽限期兜底，避免在固件真正启动前误判
    - `0x0c` 及以上错误状态立即返回
  - 只在最后一个 `EndPage` 后调用；行数据中间不要插 `0x70`
  - 默认超时 10 分钟

## 规划：frontend / 设备线程 / transport

目标：把「命令怎么编码」和「字节怎么送到设备」拆开。调用方只面对 `frontend`，
不关心 USB/BLE，也不直接碰 `Assembler`。

```text
用户代码
  │  同步或 async 调用
  ▼
frontend（选择后端 / 连接参数 / ModelVersion）
  │  mpsc::Sender<Request>
  ▼
设备 worker 线程（每个设备一个）
  │  编码 + 写 transport + 读响应
  ▼
transport（USB HID / BLE 串口透传）
```

- `frontend`
  - 入口：选择 backend（USB / BLE）+ 连接参数 + `ModelVersion`
  - 结构体持有到 worker 的 channel，不直接持有 USB/BLE 句柄
  - 对外方法返回 `Result<T, FrontendError>`
  - 设备断开后：worker 把状态置为不可用，后续调用立即返回错误，不再排队
- 设备 worker
  - 从 channel 取请求，按顺序编码发送
  - 需要响应的请求进 FIFO，每项带一个 `oneshot` 和响应解码器
  - 响应按到达顺序交给队首；半条帧留在缓冲区，等下次数据继续解析
  - 写失败 / 读线程退出：清空 FIFO，所有等待者收到 `DeviceUnavailable`
  - 不做业务判断；流控计划用 `planner::flow`，重试策略放上层
- `transport`
  - 只负责字节读写，不解析命令
  - USB：HID Interrupt + `planner::usb::encode_reports` / `decode_report`
  - BLE：`ff02` 写、`ff01` 响应、`ff03` 的 `01 01` 信用
- 待定
  - 对外 API 用同步还是 async（tokio 已经在依赖里，默认 async？）
  - frontend 是否用 `Disconnected` / `Connected` 类型状态区分
  - 混合响应的分派方式：每请求自带 decoder，还是中央 `Resp` enum
  - 大量无响应打印命令是否单独走批量队列，避免和查询命令抢 FIFO
- 倾向
  - worker 只做「切帧」，把原始 payload 交回 frontend；frontend 的泛型方法再
    `T::parse`。这样不用把 `RespTrait` 擦成 `Any`，也避免跨线程传泛型闭包。
  - 为此需要给 `Assembler` 暴露一个返回 `(Vec<u8>, consumed)` 的公开切帧接口。

- [x] 定义 `Transport` trait（write / read / reset）
- [x] `Assembler` 暴露切帧接口：`decode_frame -> Option<(Vec<u8>, consumed)>`
- [x] USB transport 接 `planner::usb`
- [ ] 定义 `frontend::Device` 对外接口和错误类型
- [ ] 设备 worker 线程 + 请求 FIFO + 响应配对
- [ ] BLE transport（`btleplug` + `ff03` 信用）
- [ ] 断连 / 写失败时让所有 pending 请求返回错误
- [ ] 示例：连接 → 读 DPI / 宽度 → 打印一张图

## resp 层约定

- `payload` = `命令组 + 命令类型 + 数据`，不含长度字段和校验和，与 `Envelope.payload` 对称
- 模板保留 `#[deku(magic = ...)]`，头校验交给 deku 的 `DekuRead`；`resp/` 层不再存 `HEADER`
- `RespTrait::parse(payload, mv)`：版本差异（如 v0 的 `darkness + 1`）在 `resp/` 的 domain 层归一化
- 模板只解析**一条**响应；多条响应由帧层按长度字段切分
- 长度不固定的尾部用 `#[deku(read_all)] _tail: Vec<u8>`
- `0x45` 这类 1/2/3 字节变长字段按 `payload.len()` 用 `VarintMode::decode` 选解码器，不用 `VarFixed3` 字段
- 设置类枚举的 discriminant = `v2`/`v1` 线上值；用户面值走 `user_value`/`from_user`，跨版本走 `to_wire`/`from_wire`
- 中央 `Resp` enum 暂不做；调用方静态指定响应类型

## 阶段

### P0 基础设施

- [x] `src/command/mod.rs` 加 `pub mod resp;`
- [x] 新建 `src/command/resp/mod.rs`
  - `RespTrait`
  - `ParseRespError`（`Decode` / `TrailingBytes` / `InvalidValue` / `BadLength`）
  - `decode_resp`（要求模板完整消费 payload；4 个用例）
- [x] `resp_template/mod.rs` 加 `test_toolkits`（`hexdec` / `eq` / `dec_tl`）
- [x] `settings.rs`：`Min = 0`，discriminant = `v2`/`v1` 线上值，补 `user_value` / `from_user` / `to_wire` / `from_wire`

### P1 darkness 样板

- [x] `resp_template/darkness.rs`：`_tail` 改成 `Vec<u8>` + `read_all`，补字节级用例
- [x] 新建 `resp/darkness.rs`：`Darkness { darkness: DarknessSetting }`
  - v2/v1：原始值 `0..=14` → `+1`
  - v0：原始值 `1..=15` → 直接使用
- [x] `cmd-impl-status.md`：`0x43` resp 列改成 `DarknessV2V1V0Tl`

### P2 固定长度查询批（14）

- [x] `0x42` 纸张类型
- [x] `0x44` 速度
- [x] `0x45` 纸间隔（1/2/3 字节，按长度解码）
- [x] `0x42` / `0x43` / `0x44` / `0x45` 的 req domain（`src/command/req/`）
  - `Darkness` / `Speed` 分别用 `DarknessSetting::to_wire(mv)` / `SpeedSetting::to_wire(mv)`
  - `GapType` 用 `GapTypeSetting::to_wire()`（全机型一致）
  - `GapLength` 校验 `50..=4194303`，`v1`/`v0` 返回 `UnsupportedUsage`
- [ ] `0x48` 自动关机（读回 1B；写 1/2B BE）
- [ ] `0x52` 支持纸张类型（4B：`02 03 04 00`）
- [ ] `0x57` 支持电机模式（1B：`0C`）
- [ ] `0x59` 支持语言（2B：`61 01`，语义待确认）
- [ ] `0x5b`（2B：`00 00`，语义待确认）
- [ ] `0x66` 纸张类型（DP27P USB 上裸读 / v1 校验 / 带参数多变体均无响应）
- [x] `0x71` DPI（u16BE）
- [x] `0x73` 统计（4 × u32BE）
- [ ] `0x7a` 硬件版本（5B：`26 01 04 01 04`）
- [x] `0x7f` 设备信息（DP27P 回 16B 全 `00`；参数、激活状态都不影响，语义未确认，原样保留）
- [x] `0x80` 激活 / 结束 `> 0x80` 命令（`0x7f` / `0x00` / `0x49`）

> 以上未勾选项的字节布局都在 DP27P（软件版本 `3.1.20230620`，USB）上实测过；
> `0x66` 除外，多种读取变体均无响应。

### P3 变长 / 字符串批（6）

- [x] `0x70` 打印机状态（状态码 1B + `PageKey` u16BE + tail）
- [x] `0x72` 打印宽度 / 纸宽（u16BE ×2 + tail）
- [x] `0x75` 制造商（NUL 结尾字符串，GBK → `Box<str>`）
- [x] `0x79` 设备名（NUL 结尾字符串 → `Box<str>`）
- [x] `0x7c` 软件版本（NUL 结尾字符串 → `Box<str>`）
- [x] `0x7d` 蓝牙 MAC（`0x10 <6B>` + `0x14 <6B>`）
- [x] GBK 方案：`encoding_rs`；`resp_template` 保留 `Vec<u8>`，`resp` 输出 `Box<str>`
- [x] `status.rs` 重写为 `PrinterStatus`（25 个线上值 + `TryFrom` / `to_wire` / `from_wire` / `is_printable` / `is_error`）

### P4 流控（2）

- [x] `0x77`：`size: VarAuto` + `status: i32 LE` + `syncToken: i16 LE` + tail
- [x] `0x7b`：`size: VarAuto`（只有固件上报，主机不发请求）
- [x] 用 [flow-control.md](./flow-control.md) 第 3 节示例做用例

### P5 帧层

- [x] `src/planner/assembler.rs`：`Assembler::encode` / `decode_one`（命令头 → 变长长度 → data → 校验和）
- [x] `decode_one(buf) -> Option<(resp, consumed)>`：尾部半条返回 `None`，调用方保留缓冲区
- [x] 用例：两条拼接、半条、坏校验、`0x88` 固定校验、2 字节长度字段
- [x] USB `1e <len>` 入口：`src/planner/usb.rs`（封包 / 解包 / 64 字节 HID 报告切片）

### P6 读请求补齐

- [x] `0x70` / `0x71` / `0x72` / `0x73` / `0x75` / `0x77` / `0x79` / `0x7c` / `0x7d` 的读取请求
- [x] `0x7b` 只有固件上报，没有请求
- [ ] `0x52` / `0x57` / `0x59` / `0x66` 的读取请求（等响应格式确认）
- [ ] 决定用 `ReadRequest { group, type_ }` 还是每命令一个空结构体

### P7 后端集成

> 具体接口拆分见上面「规划：frontend / 设备线程 / transport」。

- [x] `lib.rs` 恢复 `pub mod backend;`
- [ ] IN 线程用 `Assembler::decode_one` 替换旧的 `Command::parse_device_command`
- [ ] 队列里放单条 raw payload，调用方用 `RespTrait::parse` 解码
- [ ] 请求/响应 1:1 FIFO 配对
- [ ] 连接后确定 `ModelVersion`：目前只能由调用方指定；SDK `h=0/16/303` 的来源还没确认
- [ ] BLE 后端：`ff02` 写入、`ff01` 响应、`ff03` 的 `01 01` 信用流控（见 [issue #3](https://github.com/sb-child/dz-print/issues/3)）
- [ ] 端到端：连接 → 读 DPI / 宽度 → 打印

### P8 待抓包

- [ ] `0x49`
- [ ] `0x4d`
- [ ] `0x78`（必须读回包，格式不明）
- [ ] `0x7e`（会重启）
- [ ] `0x83`（peripheralFlags）
- [ ] `0x84`（batteryCount / hw / sw flags）
- [ ] `0x88`（打印头温度 / 电池电压，两个子命令）
- [ ] `0x9e`（握手）
- [ ] `0x9f`（芯片 ID / 生产日期 / CRC / stack，四个子命令）

## 待确认 / 风险

- [x] RLE 的 `ManualPack` 修复已复测：字节级 `0x29` 正常，位级 `0x2c` / `0x3c` 正常
- [ ] 「尾部白色不编码、解码端补白」在 X 模式下已验证；「D 模式保留上一行」只在连续 D 时可能成立，X / D 混用已确认会断层
- [ ] `0x3a`（RLE4X）在 DP27P 上仍空白，待换机型或固件验证
- [ ] `cmd-impl-status.md` 的 `0x9e` resp 列现在是「无」，需要修正
- [ ] `0x70` / `0x72` 响应长度随固件变化，只从头部取固定字段，其余放 tail
- [ ] `0x77` 的 2 字节变长在 SDK 读取侧是反的，那是 SDK bug，统一按写入侧约定
- [ ] `0x77` 的 `status` 位语义：`0` 正常、`0x0400_0000` 等待数据、其余错误；尚未拿到干净回包验证
- [ ] `0x73` 两批抓包都不满足 `PrintLines + NullLines == WorkLines`；DP27P 新样本里 `PrintLines` 约是 `WorkLines` 的 2 倍，字段顺序 / 语义待复核
- [ ] `0x66` 在 DP27P USB 上多种变体均无响应；设置 `0x00` / `0x01` / `0x02` 也不改变 `0x42`，写命令假设未证实
- [ ] `0x7f` 在 DP27P 上固定回 16 字节全 `00`；激活状态、1/2 字节子索引、16 字节载荷都不改变回包，字段语义仍未知
- [ ] `0x80` 结束激活时设备回 `0x80` 而不是 `0x00`，`0x49` 的响应还没抓过
- [ ] `0x70` 请求的 `01` 参数（SDK `n1(true)` 分支）尚未确认用途，当前只发空载荷
- [ ] USB 报告的 `1e <长度>` 按旧 `packager` 实现为「本片命令区长度」，不是整段长度；切片的长度语义需要真机确认
- [ ] `ModelVersion` 判定依据：协议只写了 SDK `h=0/16/303`，没有从设备读取 `device_type` / `h` 的实现；DP27P 旧代码走 v1，DP305P 暂按 v2。实测 DP27P（`DP27P-Y4094C023`，`3.1.20230620`）在 v2 编码下能查询 `0x70`~`0x7d`，打印命令还没验证
- [ ] `0x88` 校验歧义：v2 的合法校验和恰好为 `0x88` 时会被当成「不校验」而跳过验证；与 SDK 行为一致，暂不处理
- [ ] 每完成一个阶段，同步更新 `cmd-impl-status.md` 的 resp 列；确认过的格式补到 `protocol.md`
- [x] 新增 `resp/` 后补 `README.md` 的代码结构说明

## 文档同步规则

新增或修改命令模板时：

- [ ] 同步 `cmd-impl-status.md` 的 req / resp 列
- [ ] 同步 `protocol.md` 的字节格式
- [ ] 源码链接保持可点击
